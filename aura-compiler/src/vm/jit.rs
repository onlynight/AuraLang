//! Aura VM 热点 JIT 编译器（Cranelift，对应 技术方案 §7.2）
//!
//! **仅在 `jit` feature 下编译。** 默认 `aura run` 走解释器（§7.1 的直接线程码分派），
//! 本模块提供可选的热点原生编译层。
//!
//! ## 何时启用
//! 当 `VmOptions::jit == true` 且 `jit` feature 编译时，`Vm::maybe_jit_compile` 在
//! 某用户函数累计调用次数超过 `hotspot_threshold` 后，尝试将其编译为原生代码并缓存；
//! 之后对该函数的 `Call` 经 `Vm::do_call` 直接派发到原生入口（§7.2 的「方法级 JIT」，
//! 即 Patch 替换字节码解释）；编译失败则永久回退解释器（§7.1，5.13 回退机制）。
//!
//! ## ABI
//! ```c
//! void aura_jit_entry(const JitValue* args, JitValue* out, usize argc);
//! ```
//! [`JitValue`] 为 C 布局的 `[tag: i64, payload: i64]` 二元组（tag 0=Int/1=Float/
//! 2=Bool/3=Null；payload 为数值位 / `f64` 位模式 / 布尔 0-1）。解释器在调用边界
//! 处完成 `Value ↔ JitValue` 转换（[`JitValue::from_value`] / [`JitValue::to_value`]）。
//!
//! ## 编译策略（寄存器式 SSA）
//! Cranelift 采用寄存器式 SSA 中间表示：栈式字节码在编译期被翻译为
//! `FunctionBuilder::Variable`（SSA 寄存器），操作数栈映射为显式栈槽，
//! 最终生成原生机器码——解释执行走栈式，JIT 编译走寄存器式（§7.1/§7.2）。
//!
//! ## 编译范围（保守策略）
//! 为保证语义正确且不引入堆/嵌套调用的复杂 ABI，JIT 仅编译 **叶子整数函数**：
//! 仅含 `LoadConst`(Int) / `LoadVar` / `StoreVar`、整数算术与比较、`Jump*` 控制流、
//! `Return`。遇到 `Call`、对象/数组、浮点/字符串或非常量则放弃编译（回退解释器）。
//! 这覆盖了热点数值循环这一典型 JIT 目标（§7.2 优化技术的前提）。

use std::collections::HashMap;

use crate::codegen::opcode::Const;
use crate::vm::value::Value;
use crate::vm::{DecodedFunction, Instr};

/// JIT ABI 值类型：C 布局的 `[tag, payload]` 两个 `i64`
///
/// 解释器的 [`Value`] 是 Rust 枚举（含 `Rc<str>` 等非 POD 变体），不能直接跨越
/// JIT ABI 边界；此处提供显式的 POD 表示与双向转换。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JitValue {
    pub tag: i64,
    pub payload: i64,
}

/// `Value::Int` 对应的 tag
pub const TAG_INT: i64 = 0;
/// `Value::Float` 对应的 tag（payload 为 `f64.to_bits()`）
pub const TAG_FLOAT: i64 = 1;
/// `Value::Bool` 对应的 tag（payload 0/1）
pub const TAG_BOOL: i64 = 2;
/// `Value::Null` / `Value::Str` / `Value::Ref` 对应的 tag（JIT 范围内不出现）
pub const TAG_NULL: i64 = 3;

impl JitValue {
    /// Null 值
    pub fn null() -> Self {
        JitValue {
            tag: TAG_NULL,
            payload: 0,
        }
    }

    /// 解释器值 → JIT ABI 值（Str/Ref 视为 Null；JIT 范围内不会出现）
    pub fn from_value(v: &Value) -> Self {
        match v {
            Value::Int(i) => JitValue {
                tag: TAG_INT,
                payload: *i,
            },
            Value::Float(f) => JitValue {
                tag: TAG_FLOAT,
                payload: f.to_bits() as i64,
            },
            Value::Bool(b) => JitValue {
                tag: TAG_BOOL,
                payload: *b as i64,
            },
            _ => JitValue::null(),
        }
    }

    /// JIT ABI 值 → 解释器值（未知 tag 视为 Null）
    pub fn to_value(self) -> Value {
        match self.tag {
            TAG_INT => Value::Int(self.payload),
            TAG_FLOAT => Value::Float(f64::from_bits(self.payload as u64)),
            TAG_BOOL => Value::Bool(self.payload != 0),
            _ => Value::Null,
        }
    }
}

/// 已编译函数的原生入口（C ABI）
///
/// `dispatch_table` 用于 Fix B：JIT 代码在遇到 `Call` 时从此表查找被调用
/// 函数的入口并间接调用，从而支持递归函数（如 `fib`）的 JIT 编译。
///
/// 注意：dispatch_table 参数使用 `*const ()` 而非 `*const JitEntry`，
/// 以避免类型别名递归（JitEntry 不能包含自身）。在 dispatch helper 中
/// 会将其转回 `*const JitEntry` 进行索引。
pub type JitEntry = unsafe extern "C" fn(*const JitValue, *mut JitValue, usize, *const ());

/// JIT 状态：缓存已编译 / 已跳过编译的函数
pub struct JitState {
    compiled: HashMap<usize, JitEntry>,
    skipped: HashMap<usize, ()>,
    /// 分派表：dispatch_table[i] = 函数 i 的 JIT 入口（未编译时为 None）
    /// 由 JIT 代码在 `Call` 指令处间接查表调用。
    ///
    /// 使用 `Option<JitEntry>` 安全表达可能为 null 的函数指针（None 的位模式
    /// 即 null），与 `*const JitEntry` 可安全互转（Option<fn> 与 fn 布局相同）。
    dispatch_table: Vec<Option<JitEntry>>,
}

impl Default for JitState {
    fn default() -> Self {
        JitState {
            compiled: HashMap::new(),
            skipped: HashMap::new(),
            dispatch_table: Vec::new(),
        }
    }
}

impl JitState {
    /// Ensure the dispatch table has at least `len` slots (filled with None) to keep the underlying pointer stable.
    pub fn ensure_capacity(&mut self, len: usize) {
        while self.dispatch_table.len() < len {
            self.dispatch_table.push(None);
        }
    }
    pub fn new() -> Self {
        JitState::default()
    }
    pub fn is_compiled(&self, idx: usize) -> bool {
        self.compiled.contains_key(&idx)
    }
    pub fn is_skipped(&self, idx: usize) -> bool {
        self.skipped.contains_key(&idx)
    }
    pub fn insert(&mut self, idx: usize, entry: JitEntry) {
        // 确保分派表足够大
        while self.dispatch_table.len() <= idx {
            self.dispatch_table.push(None);
        }
        self.dispatch_table[idx] = Some(entry);
        self.compiled.insert(idx, entry);
    }
    pub fn skip(&mut self, idx: usize) {
        self.skipped.insert(idx, ());
    }

    /// 获取分派表的原始指针（供 JIT 代码在 `Call` 时查表）
    ///
    /// `Option<JitEntry>` 与 `JitEntry` 布局相同（None ≡ null），
    /// 故 `*const Option<JitEntry>` 可直接当作 `*const JitEntry` 使用。
    /// 返回 `*const ()` 以避免 JitEntry 类型的递归定义。
    fn dispatch_table_ptr(&self) -> *const () {
        self.dispatch_table.as_ptr() as *const ()
    }

    /// 调用已编译函数（args 为入参，返回其结果）
    ///
    /// 内部分配单个返回值缓冲并调用原生入口；不安全操作被封装在此处。
    pub fn call(&self, idx: usize, args: &[JitValue]) -> Option<JitValue> {
        let entry = *self.compiled.get(&idx)?;
        let mut out = JitValue::null();
        // Safety: out 指向合法的 JitValue 缓冲；args 长度由调用方保证
        // 与编译时函数签名（param_count）一致
        unsafe { entry(args.as_ptr(), &mut out, args.len(), self.dispatch_table_ptr()) };
        Some(out)
    }

    /// 调用已编译函数（args 为入参，结果写入 `out`）
    ///
    /// # Safety
    /// 调用方须保证 `args`/`out` 指向合法内存且长度足够。
    pub unsafe fn invoke(&self, idx: usize, args: *const JitValue, out: *mut JitValue, argc: usize) {
        if let Some(entry) = self.compiled.get(&idx) {
            // Safety: 调用方已按文档保证指针合法性
            unsafe { entry(args, out, argc, self.dispatch_table_ptr()) };
        }
    }
}

/// JIT 调用分派助手（Fix B）：由 JIT 代码在 `Call` 指令处调用，
/// 从 dispatch table 查找被调用函数的入口并间接调用。
///
/// 签名：void dispatch(const void* table, usize callee_idx, const JitValue* args, JitValue* out, usize argc)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aura_jit_dispatch(
    dispatch_table: *const (),
    callee_idx: usize,
    args: *const JitValue,
    out: *mut JitValue,
    argc: usize,
) {
    let table = dispatch_table as *const Option<JitEntry>;
    let entry = unsafe { *table.add(callee_idx) };
    match entry {
        None => return, // 被调用函数未 JIT 编译（不应发生）
        Some(f) => unsafe { f(args, out, argc, dispatch_table) },
    }
}

/// 判断函数是否「可 JIT 编译」（叶子整数函数 + 递归调用，Fix B）
///
/// 原策略（叶子整数函数）仅允许无 `Call` 的函数，导致 `fib` 类递归热点被拒绝。
/// 新策略允许 `Call` 指令，前提是目标函数本身也可 JIT 编译（递归安全：通过
/// `in_progress` 集合检测循环调用，自递归返回 true）。
///
/// `idx` 是函数在 `funcs` 中的索引，`f` 是对应的函数。
pub fn is_jit_compilable(idx: usize, _f: &DecodedFunction, consts: &[Const], funcs: &[DecodedFunction]) -> bool {
    is_jit_compilable_inner(idx, consts, funcs, &mut std::collections::HashSet::new())
}

fn is_jit_compilable_inner(
    idx: usize,
    consts: &[Const],
    funcs: &[DecodedFunction],
    in_progress: &mut std::collections::HashSet<usize>,
) -> bool {
    if idx >= funcs.len() {
        return false;
    }
    // 递归安全：若已在检查链中（自递归 / 互递归），视为可编译
    if !in_progress.insert(idx) {
        return true;
    }
    let f = &funcs[idx];
    for instr in &f.code {
        match instr {
            Instr::LoadConst(ci) => {
                // 仅支持整数常量参与 JIT
                match consts.get(*ci as usize) {
                    Some(Const::Int(_)) => {}
                    _ => return false,
                }
            }
            Instr::LoadVar(_)
            | Instr::StoreVar(_)
            | Instr::Add
            | Instr::Sub
            | Instr::Mul
            | Instr::Div
            | Instr::Rem
            | Instr::Neg
            | Instr::Eq
            | Instr::Ne
            | Instr::Lt
            | Instr::Gt
            | Instr::Le
            | Instr::Ge
            | Instr::Jump(_)
            | Instr::JumpIfTrue(_)
            | Instr::JumpIfFalse(_)
            | Instr::Return => {}
            // Fix B：允许对 JIT 可编译函数的调用（含自递归）
            Instr::Call(ci) => {
                if !is_jit_compilable_inner(*ci as usize, consts, funcs, in_progress) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// 编译单个函数到原生代码（使用 Cranelift）。返回 `None` 表示放弃（回退解释器）。
///
/// `funcs` 用于 Fix B：检查 `Call` 指令的目标函数是否也可 JIT 编译，
/// 从而支持递归函数（如 `fib`）的编译。
/// `idx` 是函数在 `funcs` 中的索引。
pub fn compile_function(
    idx: usize,
    f: &DecodedFunction,
    consts: &[Const],
    funcs: &[DecodedFunction],
) -> Option<JitEntry> {
    if !is_jit_compilable(idx, f, consts, funcs) {
        return None;
    }
    cranelift_backend::jit_compile_cranelift(f, consts, funcs)
}

// ─────────────────────────────────────────────────────────────────────────────
// Cranelift 后端（寄存器式 SSA）
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "jit")]
mod cranelift_backend {
    use super::{DecodedFunction, Instr, JitEntry, TAG_BOOL, TAG_INT};
    use crate::codegen::opcode::Const;

    use std::collections::HashMap;

    use cranelift::codegen::ir::{
        condcodes::IntCC, immediates::Offset32, types, AbiParam, InstBuilder, MemFlags,
        Signature, StackSlotData, StackSlotKind, TrapCode, Value as IrValue,
    };
    use cranelift::frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
    use cranelift::codegen::ir::Block;
    use cranelift_jit::{JITBuilder, JITModule};
    use cranelift_module::{default_libcall_names, Linkage, Module};

    /// JIT ABI 中单个值的大小（`[tag: i64, payload: i64]`）
    const VALUE_BYTES: i64 = 16;
    /// JIT 内操作数栈容量（叶子整数函数的栈深度很小，256 绰绰有余）
    const MAX_STACK: i64 = 256;

    pub(super) fn jit_compile_cranelift(
        f: &DecodedFunction,
        consts: &[Const],
        funcs: &[DecodedFunction],
    ) -> Option<JitEntry> {
        // JIT 代码运行在宿主进程内，使用宿主 ISA（通过 JITBuilder::with_flags 配置）
        let builder =
            JITBuilder::with_flags(&[("opt_level", "speed")], default_libcall_names()).ok()?;
        let mut module = JITModule::new(builder);

        // 指针类型与调用约定取自宿主目标配置
        let tc = module.target_config();
        let ptr_ty = tc.pointer_type();
        let call_conv = tc.default_call_conv;

        // C ABI：void entry(const JitValue* args, JitValue* out, usize argc, const JitEntry* dispatch_table)
        // dispatch_table 用于 Fix B：JIT 代码在 `Call` 时查表间接调用被编译函数
        let sig = Signature {
            params: vec![
                AbiParam::new(ptr_ty),
                AbiParam::new(ptr_ty),
                AbiParam::new(types::I64),
                AbiParam::new(ptr_ty),
            ],
            returns: vec![],
            call_conv,
        };

        let func_id = module
            .declare_function("aura_jit_entry", Linkage::Export, &sig)
            .ok()?;
        let mut ctx = module.make_context();
        ctx.func.signature = sig;

        // ── 布局：仅为跳转目标（含隐式 fallthrough 点）创建 Block ──
        let targets = compute_targets(f);
        let mut blocks: HashMap<usize, Block> = HashMap::new();

        // 静态统计每个块的前驱总数（用于「前驱齐备即密封」的尽早 seal 策略）
        let mut pred_total: HashMap<usize, usize> = HashMap::new();
        for (idx, instr) in f.code.iter().enumerate() {
            match instr {
                Instr::Jump(t) => {
                    *pred_total.entry(*t).or_insert(0) += 1;
                }
                Instr::JumpIfTrue(t) | Instr::JumpIfFalse(t) => {
                    *pred_total.entry(*t).or_insert(0) += 1;
                    *pred_total.entry(idx + 1).or_insert(0) += 1;
                }
                _ => {}
            }
        }
        let mut pred_declared: HashMap<usize, usize> = HashMap::new();
        let mut sealed: std::collections::HashSet<usize> = std::collections::HashSet::new();

        {
            let mut fbctx = FunctionBuilderContext::new();
            let mut fb = FunctionBuilder::new(&mut ctx.func, &mut fbctx);
            let entry = fb.create_block();
            // 为入口块追加与函数签名一致的块参数（args, out, argc, dispatch_table）
            fb.append_block_params_for_function_params(entry);
            blocks.insert(0, entry);
            for &t in targets.iter().skip(1) {
                // 越界目标（如末尾 Return 的 idx+1）不会生成 Block
                if t < f.code.len() {
                    blocks.insert(t, fb.create_block());
                }
            }
            fb.switch_to_block(entry);

            // ── 导入 JIT 入口签名（用于 call_indirect）──
            let jit_entry_sig = Signature {
                params: vec![
                    AbiParam::new(ptr_ty),      // args
                    AbiParam::new(ptr_ty),      // out
                    AbiParam::new(types::I64),   // argc
                    AbiParam::new(ptr_ty),      // dispatch_table
                ],
                returns: vec![],
                call_conv,
            };
            let jit_entry_sig_ref = fb.import_signature(jit_entry_sig);

            // 入口块若无入边（不可能跳回 0）则立即可密封
            if pred_total.get(&0).copied().unwrap_or(0) == 0 {
                sealed.insert(0);
                fb.seal_block(entry);
            }

            // 局部变量槽 → SSA 寄存器（tag/payload 各一个 Variable）
            let tag_vars: Vec<Variable> = (0..f.locals as usize)
                .map(|i| Variable::from_bits(100 + i as u32 * 2))
                .collect();
            let payload_vars: Vec<Variable> = (0..f.locals as usize)
                .map(|i| Variable::from_bits(100 + i as u32 * 2 + 1))
                .collect();
            for v in tag_vars.iter().chain(payload_vars.iter()) {
                fb.declare_var(*v, types::I64);
                let zero = fb.ins().iconst(types::I64, 0);
                fb.def_var(*v, zero);
            }

            // 显式操作数栈（栈槽）+ 栈指针
            let stack_slot = fb.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                (VALUE_BYTES * MAX_STACK) as u32,
                4, // 对齐 16 字节（log2）
            ));
            let sp = Variable::from_bits(3);
            fb.declare_var(sp, types::I64);
            let zero_sp = fb.ins().iconst(types::I64, 0);
            fb.def_var(sp, zero_sp);

            // ABI 指针：args / out / dispatch_table（argc 暂不使用）
            let args_ptr = Variable::from_bits(0);
            let out_ptr = Variable::from_bits(1);
            let dispatch_table = Variable::from_bits(4);
            fb.declare_var(args_ptr, ptr_ty);
            fb.def_var(args_ptr, fb.block_params(entry)[0]);
            fb.declare_var(out_ptr, ptr_ty);
            fb.def_var(out_ptr, fb.block_params(entry)[1]);
            fb.declare_var(dispatch_table, ptr_ty);
            fb.def_var(dispatch_table, fb.block_params(entry)[3]);

            // 参数按 ABI 布局拷入局部槽 0..param_count
            for i in 0..f.param_count as usize {
                let off = (i as i64) * VALUE_BYTES;
                let base = fb.block_params(entry)[0];
                let addr = fb.ins().iadd_imm(base, off);
                let tag = fb.ins().load(types::I64, MemFlags::new(), addr, Offset32::new(0));
                let payload = fb
                    .ins()
                    .load(types::I64, MemFlags::new(), addr, Offset32::new(8));
                if let Some(tv) = tag_vars.get(i) {
                    fb.def_var(*tv, tag);
                }
                if let Some(pv) = payload_vars.get(i) {
                    fb.def_var(*pv, payload);
                }
            }

            // ── 线性发射：遇到跳转目标点切换 Block ──
            //
            // 密封策略（尽早 seal）：某块的已声明前驱数达到静态统计的前驱总数时
            // 立即 `seal_block`。这对循环至关重要——若统一延迟到 `seal_all_blocks`，
            // 循环体内的 `use_var` 会因 SSA 缓存读到陈旧值（绕过循环头 phi）。
            let mut current = entry;
            let mut terminated = false;
            for (idx, instr) in f.code.iter().enumerate() {
                // 到达跳转目标点：切换到对应 Block
                if let Some(&b) = blocks.get(&idx) {
                    if b != current {
                        if !terminated {
                            // 未终结的当前块：补一条 fallthrough 跳转
                            fb.ins().jump(b, &[]);
                            seal_if_complete(
                                &mut fb, idx, &pred_total, &mut pred_declared, &mut sealed,
                                &blocks,
                            );
                        }
                        fb.switch_to_block(b);
                        current = b;
                        terminated = false;
                    }
                } else if terminated {
                    // 终结指令之后的非目标指令：不可达代码，新开 Block 承接
                    let b = fb.create_block();
                    fb.switch_to_block(b);
                    current = b;
                    terminated = false;
                }

                let term = emit_instr(
                    &mut fb,
                    instr,
                    consts,
                    funcs,
                    &tag_vars,
                    &payload_vars,
                    &blocks,
                    idx,
                    &stack_slot,
                    sp,
                    args_ptr,
                    out_ptr,
                    ptr_ty,
                    jit_entry_sig_ref,
                    dispatch_table,
                );
                terminated = terminated || term;

                // 终结指令刚声明了新的前驱：尝试密封其目标块
                if term {
                    match instr {
                        Instr::Jump(t) => {
                            seal_if_complete(
                                &mut fb, *t, &pred_total, &mut pred_declared, &mut sealed,
                                &blocks,
                            );
                        }
                        Instr::JumpIfTrue(t) | Instr::JumpIfFalse(t) => {
                            seal_if_complete(
                                &mut fb, *t, &pred_total, &mut pred_declared, &mut sealed,
                                &blocks,
                            );
                            seal_if_complete(
                                &mut fb, idx + 1, &pred_total, &mut pred_declared, &mut sealed,
                                &blocks,
                            );
                        }
                        _ => {}
                    }
                }
            }
            // 函数末尾未终结（理论不出现）：返回 out 当前值（Null）
            if !terminated {
                fb.ins().return_(&[]);
            }
            // 所有前驱此时已知，统一 seal
            fb.seal_all_blocks();
            fb.finalize();
        }

        if module.define_function(func_id, &mut ctx).is_err() {
            return None;
        }
        module.clear_context(&mut ctx);
        if module.finalize_definitions().is_err() {
            return None;
        }
        let code = module.get_finalized_function(func_id);
        // JITModule 的代码内存随对象存活；永久泄漏以保持原生代码有效
        std::mem::forget(module);
        Some(unsafe { std::mem::transmute::<*const u8, JitEntry>(code) })
    }

    /// 计算需要创建 Block 的指令索引集合：
    /// - 函数入口 `0`
    /// - 每条跳转指令的目标及其下一条（条件跳转的 fallthrough）
    /// - 每条终结指令（含 Return）之后的下一条（承接后续线性指令）
    fn compute_targets(f: &DecodedFunction) -> Vec<usize> {
        let mut set = std::collections::BTreeSet::new();
        set.insert(0);
        for (idx, instr) in f.code.iter().enumerate() {
            match instr {
                Instr::Jump(t) => {
                    set.insert(*t);
                    set.insert(idx + 1);
                }
                Instr::JumpIfTrue(t) | Instr::JumpIfFalse(t) => {
                    set.insert(*t);
                    set.insert(idx + 1);
                }
                Instr::Return => {
                    set.insert(idx + 1);
                }
                _ => {}
            }
        }
        set.into_iter().collect()
    }

    /// 目标块的前驱声明数达到静态总数时立即密封（尽早 seal，见发射循环注释）
    fn seal_if_complete(
        fb: &mut FunctionBuilder,
        t: usize,
        pred_total: &HashMap<usize, usize>,
        pred_declared: &mut HashMap<usize, usize>,
        sealed: &mut std::collections::HashSet<usize>,
        blocks: &HashMap<usize, Block>,
    ) {
        let d = pred_declared.entry(t).or_insert(0);
        *d += 1;
        if *d == pred_total.get(&t).copied().unwrap_or(0) && sealed.insert(t) {
            if let Some(&b) = blocks.get(&t) {
                fb.seal_block(b);
            }
        }
    }

    /// 发射单条指令；返回 true 表示该指令终结了当前 Block
    #[allow(clippy::too_many_arguments)]
    fn emit_instr(
        fb: &mut FunctionBuilder,
        instr: &Instr,
        consts: &[Const],
        funcs: &[DecodedFunction],
        tag_vars: &[Variable],
        payload_vars: &[Variable],
        blocks: &HashMap<usize, Block>,
        idx: usize,
        stack_slot: &cranelift::codegen::ir::StackSlot,
        sp: Variable,
        _args_ptr: Variable,
        out_ptr: Variable,
        ptr_ty: types::Type,
        jit_entry_sig_ref: cranelift::codegen::ir::SigRef,
        dispatch_table: Variable,
    ) -> bool {
        let i64_ty = types::I64;
        let zero32 = Offset32::new(0);
        let eight32 = Offset32::new(8);

        // 压栈：[tag, payload] 写入栈槽并 sp+1
        // 注：0.116 中 `store` 的参数顺序为 (flags, val, ptr, offset)，语义为 p + Offset；
        // payload 用 Offset32(8) 单次偏移，切勿再对地址做 iadd_imm(8)（会叠成 +16，
        // 覆盖下一槽的 tag——这正是曾导致死循环的 bug）。
        let mut push = |fb: &mut FunctionBuilder, tag: IrValue, payload: IrValue| {
            let cur = fb.use_var(sp);
            let base = fb.ins().stack_addr(ptr_ty, *stack_slot, 0);
            let slot_off = fb.ins().imul_imm(cur, VALUE_BYTES);
            let addr = fb.ins().iadd(base, slot_off);
            fb.ins().store(MemFlags::new(), tag, addr, zero32);
            fb.ins().store(MemFlags::new(), payload, addr, eight32);
            let new_sp = fb.ins().iadd_imm(cur, 1);
            fb.def_var(sp, new_sp);
        };
        // 弹栈：sp-1 并读取 [tag, payload]
        let mut pop = |fb: &mut FunctionBuilder| -> (IrValue, IrValue) {
            let cur = fb.use_var(sp);
            let new_sp = fb.ins().iadd_imm(cur, -1);
            fb.def_var(sp, new_sp);
            let base = fb.ins().stack_addr(ptr_ty, *stack_slot, 0);
            let slot_off = fb.ins().imul_imm(new_sp, VALUE_BYTES);
            let addr = fb.ins().iadd(base, slot_off);
            let tag = fb.ins().load(i64_ty, MemFlags::new(), addr, zero32);
            let payload = fb.ins().load(i64_ty, MemFlags::new(), addr, eight32);
            (tag, payload)
        };

        match instr {
            Instr::LoadConst(ci) => {
                if let Some(Const::Int(v)) = consts.get(*ci as usize) {
                    let tag = fb.ins().iconst(i64_ty, TAG_INT);
                    let payload = fb.ins().iconst(i64_ty, *v);
                    push(fb, tag, payload);
                } else {
                    // 编译期白名单已过滤，防御性不可达
                    fb.ins().trap(TrapCode::unwrap_user(2));
                }
                false
            }
            Instr::LoadVar(s) => {
                let tag = fb.use_var(tag_vars[*s as usize]);
                let payload = fb.use_var(payload_vars[*s as usize]);
                push(fb, tag, payload);
                false
            }
            Instr::StoreVar(s) => {
                let (tag, payload) = pop(fb);
                fb.def_var(tag_vars[*s as usize], tag);
                fb.def_var(payload_vars[*s as usize], payload);
                false
            }
            Instr::Add => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().iadd(x, y)),
            Instr::Sub => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().isub(x, y)),
            Instr::Mul => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().imul(x, y)),
            Instr::Div => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().sdiv(x, y)),
            Instr::Rem => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().srem(x, y)),
            Instr::Neg => {
                let (_t, p) = pop(fb);
                let r = fb.ins().ineg(p);
                let tag = fb.ins().iconst(i64_ty, TAG_INT);
                push(fb, tag, r);
                false
            }
            Instr::Eq => cmp_int(fb, &mut pop, &mut push, IntCC::Equal),
            Instr::Ne => cmp_int(fb, &mut pop, &mut push, IntCC::NotEqual),
            Instr::Lt => cmp_int(fb, &mut pop, &mut push, IntCC::SignedLessThan),
            Instr::Gt => cmp_int(fb, &mut pop, &mut push, IntCC::SignedGreaterThan),
            Instr::Le => cmp_int(fb, &mut pop, &mut push, IntCC::SignedLessThanOrEqual),
            Instr::Ge => cmp_int(fb, &mut pop, &mut push, IntCC::SignedGreaterThanOrEqual),
            Instr::Jump(target) => {
                if let Some(&b) = blocks.get(target) {
                    fb.ins().jump(b, &[]);
                }
                true
            }
            Instr::JumpIfTrue(target) => {
                let (_t, p) = pop(fb);
                let cond = fb.ins().icmp_imm(IntCC::NotEqual, p, 0);
                // brif(条件, 真块, 假块)：真 → 目标，假 → fallthrough
                match (blocks.get(target), blocks.get(&(idx + 1))) {
                    (Some(&tb), Some(&nb)) => {
                        fb.ins().brif(cond, tb, &[], nb, &[]);
                    }
                    (Some(&tb), None) => {
                        fb.ins().jump(tb, &[]);
                    }
                    _ => {
                        fb.ins().trap(TrapCode::unwrap_user(1));
                    }
                }
                true
            }
            Instr::JumpIfFalse(target) => {
                let (_t, p) = pop(fb);
                let cond = fb.ins().icmp_imm(IntCC::NotEqual, p, 0);
                // brif(条件, 真块, 假块)：真 → fallthrough，假 → 目标
                match (blocks.get(target), blocks.get(&(idx + 1))) {
                    (Some(&tb), Some(&nb)) => {
                        fb.ins().brif(cond, nb, &[], tb, &[]);
                    }
                    (Some(&tb), None) => {
                        fb.ins().jump(tb, &[]);
                    }
                    _ => {
                        fb.ins().trap(TrapCode::unwrap_user(1));
                    }
                }
                true
            }
            Instr::Return => {
                // 与解释器语义一致：从操作数栈弹出返回值写入 out
                let (tag, payload) = pop(fb);
                let out = fb.use_var(out_ptr);
                fb.ins().store(MemFlags::new(), tag, out, zero32);
                fb.ins().store(MemFlags::new(), payload, out, eight32);
                fb.ins().return_(&[]);
                true
            }
            // Fix B：调用 JIT 可编译函数——通过 dispatch table 查入口并间接调用
            Instr::Call(callee_idx) => {
                let callee_idx = *callee_idx as usize;
                let callee = &funcs[callee_idx];
                let param_count = callee.param_count as i64;

                // 当前 sp 指向槽上方；args 位于 [sp - param_count, sp)
                let cur_sp = fb.use_var(sp);
                let args_sp = fb.ins().iadd_imm(cur_sp, -param_count);

                // args_ptr = stack_base + (sp - param_count) * 16
                let base = fb.ins().stack_addr(ptr_ty, *stack_slot, 0);
                let args_off = fb.ins().imul_imm(args_sp, VALUE_BYTES);
                let args_ptr_val = fb.ins().iadd(base, args_off);

                // out_ptr = stack_base + sp * 16（返回值写入此槽）
                let out_off = fb.ins().imul_imm(cur_sp, VALUE_BYTES);
                let out_ptr_val = fb.ins().iadd(base, out_off);

                // 从 dispatch table 加载被调用函数的入口地址
                let dt = fb.use_var(dispatch_table);
                let entry_off = fb.ins().iconst(types::I64, callee_idx as i64 * 8);
                let entry_addr = fb.ins().iadd(dt, entry_off);
                let entry_ptr = fb.ins().load(ptr_ty, MemFlags::new(), entry_addr, zero32);

                // 间接调用：call_indirect(jit_entry_sig, entry_ptr, [args_ptr, out_ptr, argc, dispatch_table])
                let argc_val = fb.ins().iconst(types::I64, param_count);
                fb.ins().call_indirect(jit_entry_sig_ref, entry_ptr, &[args_ptr_val, out_ptr_val, argc_val, dt]);

                // 更新 sp：弹出参数（sp -= param_count）
                fb.def_var(sp, args_sp);

                // 从 out_ptr 读取返回值 [tag, payload] 并压栈
                let ret_tag = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, zero32);
                let ret_payload = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, eight32);
                push(fb, ret_tag, ret_payload);

                false
            }
            _ => {
                // 白名单外指令理论不可达
                fb.ins().trap(TrapCode::unwrap_user(2));
                true
            }
        }
    }

    fn bin_int<F>(
        fb: &mut FunctionBuilder,
        pop: &mut dyn FnMut(&mut FunctionBuilder) -> (IrValue, IrValue),
        push: &mut dyn FnMut(&mut FunctionBuilder, IrValue, IrValue),
        f: F,
    ) -> bool
    where
        F: FnOnce(&mut FunctionBuilder, IrValue, IrValue) -> IrValue,
    {
        let (_bt, b) = pop(fb);
        let (_at, a) = pop(fb);
        let r = f(fb, a, b);
        let tag = fb.ins().iconst(types::I64, TAG_INT);
        push(fb, tag, r);
        false
    }

    fn cmp_int(
        fb: &mut FunctionBuilder,
        pop: &mut dyn FnMut(&mut FunctionBuilder) -> (IrValue, IrValue),
        push: &mut dyn FnMut(&mut FunctionBuilder, IrValue, IrValue),
        cc: IntCC,
    ) -> bool {
        let (_bt, b) = pop(fb);
        let (_at, a) = pop(fb);
        let c = fb.ins().icmp(cc, a, b);
        let r = fb.ins().uextend(types::I64, c);
        let tag = fb.ins().iconst(types::I64, TAG_BOOL);
        push(fb, tag, r);
        false
    }
}

#[cfg(not(feature = "jit"))]
fn jit_compile_cranelift(
    _f: &DecodedFunction,
    _consts: &[Const],
    _funcs: &[DecodedFunction],
) -> Option<JitEntry> {
    None
}
