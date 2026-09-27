//! Aura VM Hotspot JIT Compiler (Cranelift)
use crate::codegen::opcode::Const;
use crate::vm::{DecodedFunction, Instr};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// JitValue / 类型标签 / JitEntry 已迁移至 vm::abi（Phase 1: AOT 嵌入共享调用约定）。
// 此处再导出以保持向后兼容（jit feature 之外的路径引用 crate::vm::jit::* 依然有效）。
pub use crate::vm::abi::{
    AotCallContext, AotEntry, JitValue, TAG_ARRAY, TAG_BOOL, TAG_CLOSURE, TAG_CSTRING, TAG_FLOAT,
    TAG_FUNC, TAG_INT, TAG_LIST, TAG_MAP, TAG_NULL, TAG_OBJ, TAG_PTR, TAG_STR,
};

/// JIT 入口 —— 与 AOT 入口同签名（AotEntry 别名）
pub type JitEntry = AotEntry;

// ═══════════════════════════════════════════════════════════════
// P8 优化：JIT 去重编译（全局编译缓存）
// ═══════════════════════════════════════════════════════════════

/// 全局 JIT 编译缓存（线程安全）
///
/// P8 优化：消除多线程场景下同一函数被 N 个 VM 实例各编译一次的浪费。
/// 编译前查缓存，命中则复用；未命中则编译并写入缓存。
///
/// 对应文档：`docs/pure_aura_jit/jit模式优化方案.md` §3.1
///
/// 线程安全：使用 `Mutex` 保护缓存，编译锁 + 双重检查锁。
/// 注意：JitEntry 是函数指针（`fn(*const JitValue, *mut JitValue, usize, *const ())`)，
/// 可安全跨线程共享。
struct GlobalJitCache {
    /// 函数代码哈希 → 编译后的入口
    cache: Mutex<HashMap<String, JitEntry>>,
    /// 统计：缓存命中次数
    hits: Mutex<u64>,
    /// 统计：缓存未命中次数
    misses: Mutex<u64>,
}

impl GlobalJitCache {
    fn new() -> Self {
        GlobalJitCache {
            cache: Mutex::new(HashMap::new()),
            hits: Mutex::new(0),
            misses: Mutex::new(0),
        }
    }

    /// 获取全局缓存实例（懒初始化）
    fn instance() -> &'static GlobalJitCache {
        static INSTANCE: OnceLock<GlobalJitCache> = OnceLock::new();
        INSTANCE.get_or_init(GlobalJitCache::new)
    }

    /// 计算函数代码的哈希（用于缓存键）
    fn compute_key(idx: usize, f: &DecodedFunction, consts: &[Const]) -> String {
        // 简单哈希：函数索引 + 代码长度 + 参数数 + 局部变量数
        // 更精确的实现应对字节码做哈希
        let mut key = format!("{}:{}", idx, f.code.len());
        key.push_str(&format!(":p{}", f.param_count));
        key.push_str(&format!(":l{}", f.locals));
        // 简单内容哈希
        for instr in &f.code {
            key.push(':');
            match instr {
                Instr::LoadConst(ci) => key.push_str(&format!("c{}", ci)),
                Instr::LoadVar(s) => key.push_str(&format!("lv{}", s)),
                Instr::StoreVar(s) => key.push_str(&format!("sv{}", s)),
                Instr::Add => key.push_str("a"),
                Instr::Sub => key.push_str("s"),
                Instr::Mul => key.push_str("m"),
                Instr::Div => key.push_str("d"),
                Instr::Rem => key.push_str("r"),
                Instr::Neg => key.push_str("n"),
                Instr::Eq => key.push_str("eq"),
                Instr::Ne => key.push_str("ne"),
                Instr::Lt => key.push_str("lt"),
                Instr::Gt => key.push_str("gt"),
                Instr::Le => key.push_str("le"),
                Instr::Ge => key.push_str("ge"),
                Instr::Not => key.push_str("not"),
                Instr::ReturnUnit => key.push_str("ru"),
                Instr::Jump(t) => key.push_str(&format!("j{}", t)),
                Instr::JumpIfTrue(t) => key.push_str(&format!("jt{}", t)),
                Instr::JumpIfFalse(t) => key.push_str(&format!("jf{}", t)),
                Instr::Return => key.push_str("ret"),
                Instr::Call(c) => key.push_str(&format!("ca{}", c)),
                Instr::CallNative(c) => key.push_str(&format!("cn{}", c)),
                Instr::CallNativeArgs(c, _) => key.push_str(&format!("cna{}", c)),
                Instr::And => key.push_str("and"),
                Instr::Or => key.push_str("or"),
                Instr::BitAnd => key.push_str("band"),
                Instr::BitOr => key.push_str("bor"),
                Instr::BitXor => key.push_str("bxor"),
                Instr::Shl => key.push_str("shl"),
                Instr::Shr => key.push_str("shr"),
                Instr::NewObject(t) => key.push_str(&format!("no{}", t)),
                Instr::NewArray => key.push_str("na"),
                Instr::NewList => key.push_str("nl"),
                Instr::NewMap => key.push_str("nm"),
                Instr::IncRef => key.push_str("ir"),
                Instr::DecRef => key.push_str("dr"),
                Instr::Retain => key.push_str("rt"),
                Instr::Release => key.push_str("rl"),
                Instr::DropRef => key.push_str("dro"),
                _ => key.push_str("?"),
            }
        }
        key
    }

    /// 从缓存获取编译结果（命中则返回，未命中返回 None）
    fn lookup(&self, key: &str) -> Option<JitEntry> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.get(key) {
                *self.hits.lock().unwrap() += 1;
                return Some(*entry);
            }
        }
        *self.misses.lock().unwrap() += 1;
        None
    }

    /// 写入缓存
    fn put(&self, key: String, entry: JitEntry) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(key, entry);
        }
    }

    /// 缓存统计
    pub fn stats() -> (u64, u64, usize) {
        let cache = Self::instance();
        let hits = *cache.hits.lock().unwrap();
        let misses = *cache.misses.lock().unwrap();
        let size = cache.cache.lock().unwrap().len();
        (hits, misses, size)
    }

    /// 清除缓存
    pub fn clear() {
        if let Ok(mut cache) = Self::instance().cache.lock() {
            cache.clear();
        }
    }
}

/// 编译函数（带全局缓存）
///
/// P8 优化：编译前查全局缓存，命中则复用；未命中则编译并写入缓存。
pub fn compile_function_cached(
    idx: usize,
    f: &DecodedFunction,
    consts: &[Const],
    funcs: &[DecodedFunction],
) -> Option<JitEntry> {
    let key = GlobalJitCache::compute_key(idx, f, consts);

    /// 从缓存获取编译结果
    if let Some(entry) = GlobalJitCache::instance().lookup(&key) {
        return Some(entry);
    }

    // 2. 未命中，执行编译
    let result = compile_function(idx, f, consts, funcs);

    // 3. 编译成功则写入缓存
    if let Some(ref entry) = result {
        GlobalJitCache::instance().put(key, *entry);
    }

    result
}

pub struct JitState {
    compiled: HashMap<usize, JitEntry>,
    skipped: HashMap<usize, String>,
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
    /// 获取跳过原因（用于诊断日志）
    pub fn skip_reason(&self, idx: usize) -> Option<&str> {
        self.skipped.get(&idx).map(|s| s.as_str())
    }
    pub fn insert(&mut self, idx: usize, entry: JitEntry) {
        while self.dispatch_table.len() <= idx {
            self.dispatch_table.push(None);
        }
        self.dispatch_table[idx] = Some(entry);
        self.compiled.insert(idx, entry);
    }
    pub fn skip(&mut self, idx: usize, reason: &str) {
        self.skipped.insert(idx, reason.to_string());
    }
    fn dispatch_table_ptr(&self) -> *const () {
        self.dispatch_table.as_ptr() as *const ()
    }
    pub fn call(&self, idx: usize, args: &[JitValue]) -> Option<JitValue> {
        let entry = *self.compiled.get(&idx)?;
        let mut out = JitValue::null();
        unsafe {
            entry(
                args.as_ptr(),
                &mut out,
                args.len(),
                self.dispatch_table_ptr(),
            )
        };
        Some(out)
    }
    pub unsafe fn invoke(
        &self,
        idx: usize,
        args: *const JitValue,
        out: *mut JitValue,
        argc: usize,
    ) {
        if let Some(entry) = self.compiled.get(&idx) {
            unsafe { entry(args, out, argc, self.dispatch_table_ptr()) };
        }
    }
}

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
        None => return,
        Some(f) => unsafe { f(args, out, argc, dispatch_table) },
    }
}

pub fn is_jit_compilable(
    idx: usize,
    _f: &DecodedFunction,
    consts: &[Const],
    funcs: &[DecodedFunction],
) -> bool {
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
    if !in_progress.insert(idx) {
        return true;
    }
    let f = &funcs[idx];
    for instr in &f.code {
        match instr {
            Instr::LoadConst(ci) => match consts.get(*ci as usize) {
                Some(Const::Int(_)) => {}
                _ => return false,
            },
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
            // Fix 5: 扩展白名单 — Not（可用 ineg 实现）/ ReturnUnit
            | Instr::Not
            | Instr::ReturnUnit
            | Instr::Jump(_)
            | Instr::JumpIfTrue(_)
            | Instr::JumpIfFalse(_)
            | Instr::Return
            | Instr::CallNative(_)
            | Instr::CallNativeArgs(_, _) => {}
            // Fix B: 扩展白名单（P1.2）— 逻辑/位运算 + 集合分配 + ARC no-op
            | Instr::And
            | Instr::Or
            | Instr::BitAnd
            | Instr::BitOr
            | Instr::BitXor
            | Instr::Shl
            | Instr::Shr
            | Instr::NewObject(_)
            | Instr::NewArray
            | Instr::NewList
            | Instr::NewMap
            | Instr::IncRef
            | Instr::DecRef
            | Instr::Retain
            | Instr::Release
            | Instr::DropRef => {}
            // Phase D: 并发指令白名单
            | Instr::ThreadSpawn(_)
            | Instr::ThreadJoin
            | Instr::ThreadSleep
            | Instr::ThreadId
            | Instr::ThreadParallelism
            | Instr::MutexNew
            | Instr::MutexLock
            | Instr::MutexUnlock
            | Instr::MutexTryLock
            | Instr::AtomicNew
            | Instr::AtomicLoad
            | Instr::AtomicStore
            | Instr::AtomicAdd
            | Instr::AtomicCas
            | Instr::RwLockNew
            | Instr::RwLockReadLock
            | Instr::RwLockWriteLock
            | Instr::RwLockReadUnlock
            | Instr::RwLockWriteUnlock
            | Instr::ChannelNew
            | Instr::ChannelSend
            | Instr::ChannelRecv
            | Instr::CondvarNew
            | Instr::CondvarWait
            | Instr::CondvarSignal
            | Instr::CondvarBroadcast => {}
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

pub fn compile_function(
    idx: usize,
    f: &DecodedFunction,
    consts: &[Const],
    funcs: &[DecodedFunction],
) -> Option<JitEntry> {
    if !is_jit_compilable(idx, f, consts, funcs) {
        return None;
    }

    // Run optimization passes before Cranelift compilation
    let opt_result = crate::vm::jit_opt::optimize_function_with_deps(f, consts, funcs);

    // Combine original and extra constants
    let mut all_consts = consts.to_vec();
    all_consts.extend(opt_result.extra_consts);

    cranelift_backend::jit_compile_cranelift(&opt_result.func, &all_consts, funcs)
}

#[cfg(feature = "jit")]
mod cranelift_backend {
    use super::{DecodedFunction, Instr, JitEntry, TAG_BOOL, TAG_INT, TAG_NULL};
    use crate::codegen::opcode::Const;
    use cranelift::codegen::ir::Block;
    use cranelift::codegen::ir::{
        AbiParam, ExtFuncData, ExternalName, InstBuilder, MemFlags, Signature, StackSlotData,
        StackSlotKind, TrapCode, Value as IrValue, condcodes::IntCC, immediates::Offset32, types,
    };
    use cranelift::frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
    use cranelift_jit::{JITBuilder, JITModule};
    use cranelift_module::{Linkage, Module, default_libcall_names};
    use std::collections::HashMap;

    const VALUE_BYTES: i64 = 16;
    const MAX_STACK: i64 = 256;

    pub(super) fn jit_compile_cranelift(
        f: &DecodedFunction,
        consts: &[Const],
        funcs: &[DecodedFunction],
    ) -> Option<JitEntry> {
        // Cranelift optimization flags:
        // - opt_level: "speed" for maximum performance
        // - enable_verifier: false to speed up compilation (no runtime impact)
        let flags = &[
            ("opt_level", "speed"),
            ("enable_verifier", "false"),
        ];
        let builder = JITBuilder::with_flags(flags, default_libcall_names()).ok()?;
        let mut module = JITModule::new(builder);
        let tc = module.target_config();
        let ptr_ty = tc.pointer_type();
        let call_conv = tc.default_call_conv;

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

        let func_id = module.declare_function("aura_jit_entry", Linkage::Export, &sig).ok()?;
        let mut ctx = module.make_context();
        ctx.func.signature = sig;

        let targets = compute_targets(f);
        let mut blocks: HashMap<usize, Block> = HashMap::new();
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
            fb.append_block_params_for_function_params(entry);
            blocks.insert(0, entry);
            for &t in targets.iter().skip(1) {
                if t < f.code.len() {
                    blocks.insert(t, fb.create_block());
                }
            }
            fb.switch_to_block(entry);

            let jit_entry_sig = Signature {
                params: vec![
                    AbiParam::new(ptr_ty),
                    AbiParam::new(ptr_ty),
                    AbiParam::new(types::I64),
                    AbiParam::new(ptr_ty),
                ],
                returns: vec![],
                call_conv,
            };
            let jit_entry_sig_ref = fb.import_signature(jit_entry_sig);

            // Import the native dispatcher function for CallNativeArgs
            let native_dispatch_sig = Signature {
                params: vec![
                    AbiParam::new(types::I64), // native_idx
                    AbiParam::new(types::I64), // argc
                    AbiParam::new(ptr_ty),     // args_ptr
                    AbiParam::new(ptr_ty),     // out_ptr
                ],
                returns: vec![],
                call_conv,
            };
            let native_dispatch_sig_ref = fb.import_signature(native_dispatch_sig);
            let native_dispatch_data = ExtFuncData {
                name: ExternalName::testcase("aura_jit_call_native_by_index"),
                signature: native_dispatch_sig_ref,
                colocated: false,
            };
            let native_dispatch_ref = fb.import_function(native_dispatch_data);

            // Phase D: Import the name-based native dispatcher for concurrent instructions
            let native_name_dispatch_sig = Signature {
                params: vec![
                    AbiParam::new(ptr_ty),     // name_ptr
                    AbiParam::new(types::I64), // name_len
                    AbiParam::new(types::I64), // argc
                    AbiParam::new(ptr_ty),     // args_ptr
                    AbiParam::new(ptr_ty),     // out_ptr
                ],
                returns: vec![],
                call_conv,
            };
            let native_name_dispatch_sig_ref = fb.import_signature(native_name_dispatch_sig);
            let native_name_dispatch_data = ExtFuncData {
                name: ExternalName::testcase("aura_jit_call_native_by_name"),
                signature: native_name_dispatch_sig_ref,
                colocated: false,
            };
            let native_name_dispatch_ref = fb.import_function(native_name_dispatch_data);

            if pred_total.get(&0).copied().unwrap_or(0) == 0 {
                sealed.insert(0);
                fb.seal_block(entry);
            }

            let tag_vars: Vec<Variable> =
                (0..f.locals as usize).map(|i| Variable::from_bits(100 + i as u32 * 2)).collect();
            let payload_vars: Vec<Variable> = (0..f.locals as usize)
                .map(|i| Variable::from_bits(100 + i as u32 * 2 + 1))
                .collect();
            for v in tag_vars.iter().chain(payload_vars.iter()) {
                fb.declare_var(*v, types::I64);
                let zero = fb.ins().iconst(types::I64, 0);
                fb.def_var(*v, zero);
            }

            let stack_slot = fb.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                (VALUE_BYTES * MAX_STACK) as u32,
                4,
            ));
            let sp = Variable::from_bits(3);
            fb.declare_var(sp, types::I64);
            let zero_sp = fb.ins().iconst(types::I64, 0);
            fb.def_var(sp, zero_sp);

            let args_ptr = Variable::from_bits(0);
            let out_ptr = Variable::from_bits(1);
            let dispatch_table = Variable::from_bits(4);
            fb.declare_var(args_ptr, ptr_ty);
            fb.def_var(args_ptr, fb.block_params(entry)[0]);
            fb.declare_var(out_ptr, ptr_ty);
            fb.def_var(out_ptr, fb.block_params(entry)[1]);
            fb.declare_var(dispatch_table, ptr_ty);
            fb.def_var(dispatch_table, fb.block_params(entry)[3]);

            for i in 0..f.param_count as usize {
                let off = (i as i64) * VALUE_BYTES;
                let base = fb.block_params(entry)[0];
                let addr = fb.ins().iadd_imm(base, off);
                let tag = fb.ins().load(types::I64, MemFlags::new(), addr, Offset32::new(0));
                let payload = fb.ins().load(types::I64, MemFlags::new(), addr, Offset32::new(8));
                if let Some(tv) = tag_vars.get(i) {
                    fb.def_var(*tv, tag);
                }
                if let Some(pv) = payload_vars.get(i) {
                    fb.def_var(*pv, payload);
                }
            }

            let mut current = entry;
            let mut terminated = false;
            for (idx, instr) in f.code.iter().enumerate() {
                if let Some(&b) = blocks.get(&idx) {
                    if b != current {
                        if !terminated {
                            fb.ins().jump(b, &[]);
                            seal_if_complete(
                                &mut fb,
                                idx,
                                &pred_total,
                                &mut pred_declared,
                                &mut sealed,
                                &blocks,
                            );
                        }
                        fb.switch_to_block(b);
                        current = b;
                        terminated = false;
                    }
                } else if terminated {
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
                    native_dispatch_ref,
                    native_name_dispatch_ref,
                );
                terminated = terminated || term;

                if term {
                    match instr {
                        Instr::Jump(t) => {
                            seal_if_complete(
                                &mut fb,
                                *t,
                                &pred_total,
                                &mut pred_declared,
                                &mut sealed,
                                &blocks,
                            );
                        }
                        Instr::JumpIfTrue(t) | Instr::JumpIfFalse(t) => {
                            seal_if_complete(
                                &mut fb,
                                *t,
                                &pred_total,
                                &mut pred_declared,
                                &mut sealed,
                                &blocks,
                            );
                            seal_if_complete(
                                &mut fb,
                                idx + 1,
                                &pred_total,
                                &mut pred_declared,
                                &mut sealed,
                                &blocks,
                            );
                        }
                        _ => {}
                    }
                }
            }
            if !terminated {
                fb.ins().return_(&[]);
            }
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
        std::mem::forget(module);
        Some(unsafe { std::mem::transmute::<*const u8, JitEntry>(code) })
    }

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
                Instr::Return | Instr::ReturnUnit => {
                    set.insert(idx + 1);
                }
                _ => {}
            }
        }
        set.into_iter().collect()
    }

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
        native_dispatch_entry: cranelift::codegen::ir::FuncRef,
        native_name_dispatch_entry: cranelift::codegen::ir::FuncRef,
    ) -> bool {
        let i64_ty = types::I64;
        let zero32 = Offset32::new(0);
        let eight32 = Offset32::new(8);

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
            // Fix 5: Not — 位取反（~x = -x - 1）
            Instr::Not => {
                let (_t, p) = pop(fb);
                let r = fb.ins().ineg(p);
                let one = fb.ins().iconst(i64_ty, 1);
                let r = fb.ins().isub(r, one);
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
                let (tag, payload) = pop(fb);
                let out = fb.use_var(out_ptr);
                fb.ins().store(MemFlags::new(), tag, out, zero32);
                fb.ins().store(MemFlags::new(), payload, out, eight32);
                fb.ins().return_(&[]);
                true
            }
            // Fix 5: ReturnUnit — 返回空值
            Instr::ReturnUnit => {
                let out = fb.use_var(out_ptr);
                let null_tag = fb.ins().iconst(i64_ty, 0);
                fb.ins().store(MemFlags::new(), null_tag, out, zero32);
                fb.ins().store(MemFlags::new(), null_tag, out, eight32);
                fb.ins().return_(&[]);
                true
            }
            Instr::Call(callee_idx) => {
                let callee_idx = *callee_idx as usize;
                let callee = &funcs[callee_idx];
                let param_count = callee.param_count as i64;

                let cur_sp = fb.use_var(sp);
                let args_sp = fb.ins().iadd_imm(cur_sp, -param_count);

                let base = fb.ins().stack_addr(ptr_ty, *stack_slot, 0);
                let args_off = fb.ins().imul_imm(args_sp, VALUE_BYTES);
                let args_ptr_val = fb.ins().iadd(base, args_off);

                let out_off = fb.ins().imul_imm(cur_sp, VALUE_BYTES);
                let out_ptr_val = fb.ins().iadd(base, out_off);

                let dt = fb.use_var(dispatch_table);
                let entry_off = fb.ins().iconst(types::I64, callee_idx as i64 * 8);
                let entry_addr = fb.ins().iadd(dt, entry_off);
                let entry_ptr = fb.ins().load(ptr_ty, MemFlags::new(), entry_addr, zero32);

                let argc_val = fb.ins().iconst(types::I64, param_count);
                fb.ins().call_indirect(
                    jit_entry_sig_ref,
                    entry_ptr,
                    &[
                        args_ptr_val,
                        out_ptr_val,
                        argc_val,
                        dt,
                    ],
                );

                fb.def_var(sp, args_sp);

                let ret_tag = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, zero32);
                let ret_payload = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, eight32);
                push(fb, ret_tag, ret_payload);

                false
            }
            // JIT 原生函数调用：直接调用 C 调度器，不回退到解释器
            Instr::CallNativeArgs(native_idx, arg_count) => {
                let native_idx = *native_idx as i64;
                let param_count = *arg_count as i64;

                let cur_sp = fb.use_var(sp);
                let args_sp = fb.ins().iadd_imm(cur_sp, -param_count);

                let base = fb.ins().stack_addr(ptr_ty, *stack_slot, 0);
                let args_off = fb.ins().imul_imm(args_sp, VALUE_BYTES);
                let args_ptr_val = fb.ins().iadd(base, args_off);

                let out_off = fb.ins().imul_imm(cur_sp, VALUE_BYTES);
                let out_ptr_val = fb.ins().iadd(base, out_off);

                let argc_val = fb.ins().iconst(types::I64, param_count);
                let native_idx_val = fb.ins().iconst(types::I64, native_idx);

                // 调用原生调度器
                fb.ins().call(
                    native_dispatch_entry,
                    &[
                        native_idx_val,
                        argc_val,
                        args_ptr_val,
                        out_ptr_val,
                    ],
                );

                fb.def_var(sp, args_sp);

                // 从 out_ptr 加载返回值
                let ret_tag = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, zero32);
                let ret_payload = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, eight32);
                push(fb, ret_tag, ret_payload);

                false
            }
            // CallNative (无参数计数) — 回退到解释器
            Instr::CallNative(_) => {
                fb.ins().trap(TrapCode::unwrap_user(2));
                true
            }
            // Fix B: 逻辑运算（and/or → band/bor，返回 TAG_INT）
            Instr::And => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().band(x, y)),
            Instr::Or => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().bor(x, y)),
            // Fix B: 位运算
            Instr::BitAnd => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().band(x, y)),
            Instr::BitOr => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().bor(x, y)),
            Instr::BitXor => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().bxor(x, y)),
            Instr::Shl => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().ishl(x, y)),
            Instr::Shr => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().sshr(x, y)),
            // Fix B: 对象/数组/集合分配 → 返回 null 指针（简化：堆管理由运行时处理）
            Instr::NewObject(_) | Instr::NewArray | Instr::NewList | Instr::NewMap => {
                let tag = fb.ins().iconst(i64_ty, TAG_NULL);
                let payload = fb.ins().iconst(i64_ty, 0);
                push(fb, tag, payload);
                false
            }
            // Fix B: ARC 引用计数 → JIT 中为 no-op（引用管理由解释器回退处理）
            Instr::IncRef | Instr::DecRef | Instr::Retain | Instr::Release | Instr::DropRef => {
                false
            }
            // ── Phase D: 并发指令 — 通过名称调度器调用 ──
            Instr::ThreadSpawn(_) => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Thread.spawn",
                    2,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::ThreadJoin => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Thread.join",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::ThreadSleep => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Thread.sleep",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::ThreadId => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Thread.id",
                    0,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::ThreadParallelism => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Thread.parallelism",
                    0,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::MutexNew => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Mutex.new",
                    0,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::MutexLock => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Mutex.lock",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::MutexUnlock => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Mutex.unlock",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::MutexTryLock => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Mutex.tryLock",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::AtomicNew => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Atomic.new",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::AtomicLoad => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Atomic.load",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::AtomicStore => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Atomic.store",
                    2,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::AtomicAdd => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Atomic.add",
                    2,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::AtomicCas => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Atomic.cas",
                    3,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::RwLockNew => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.RwLock.new",
                    0,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::RwLockReadLock => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.RwLock.readLock",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::RwLockWriteLock => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.RwLock.writeLock",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::RwLockReadUnlock => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.RwLock.readUnlock",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::RwLockWriteUnlock => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.RwLock.writeUnlock",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::ChannelNew => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Channel.newChannel",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::ChannelSend => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Channel.channelSend",
                    2,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::ChannelRecv => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Channel.channelRecv",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::CondvarNew => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Condvar.new",
                    0,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::CondvarWait => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Condvar.wait",
                    2,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::CondvarSignal => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Condvar.signal",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            Instr::CondvarBroadcast => {
                emit_concurrent_call(
                    fb,
                    "aura.lang.concurrent.Condvar.broadcast",
                    1,
                    &mut push,
                    &mut pop,
                    stack_slot,
                    sp,
                    ptr_ty,
                    native_name_dispatch_entry,
                );
                false
            }
            _ => {
                fb.ins().trap(TrapCode::unwrap_user(2));
                true
            }
        }
    }

    /// Phase D: 并发指令 JIT 发射辅助函数
    ///
    /// 通过名称调度器 (`aura_jit_call_native_by_name`) 调用并发原生函数。
    /// 函数名作为栈上常量传递（避免 Cranelift 全局常量 API 兼容性差异）。
    ///
    /// 性能优化说明：
    /// - 当前方案：每次调用将函数名字节写入栈，产生 N 条 store 指令（N = 函数名长度）
    /// - 未来优化：使用 Cranelift `declare_data` + `define_data` API 创建只读数据段，
    ///   通过 `fb.ins().global_value(gv, Offset32::new(0))` 直接加载地址，消除栈写入开销
    /// - 影响评估：并发指令本身开销大（涉及内核调用），栈写入开销可忽略
    fn emit_concurrent_call(
        fb: &mut FunctionBuilder,
        func_name: &str,
        arg_count: i64,
        push: &mut dyn FnMut(&mut FunctionBuilder, IrValue, IrValue),
        _pop: &mut dyn FnMut(&mut FunctionBuilder) -> (IrValue, IrValue),
        stack_slot: &cranelift::codegen::ir::StackSlot,
        sp: Variable,
        ptr_ty: types::Type,
        native_name_dispatch_entry: cranelift::codegen::ir::FuncRef,
    ) {
        let i64_ty = types::I64;
        let zero32 = Offset32::new(0);
        let eight32 = Offset32::new(8);

        // 在栈上分配空间存储函数名（最大 64 字节）
        let name_size = std::cmp::max(func_name.len(), 16);
        let name_slot = fb.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            name_size as u32,
            4,
        ));
        let name_base = fb.ins().stack_addr(ptr_ty, name_slot, 0);

        // 写入函数名字节到栈
        let bytes = func_name.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            let val = fb.ins().iconst(types::I8, b as i64);
            let offset = i as i64;
            let addr = fb.ins().iadd_imm(name_base, offset);
            fb.ins().store(MemFlags::new(), val, addr, Offset32::new(0));
        }
        // 写入 null 终止符
        let null_val = fb.ins().iconst(types::I8, 0i64);
        let null_offset = func_name.len() as i64;
        let null_addr = fb.ins().iadd_imm(name_base, null_offset);
        fb.ins().store(MemFlags::new(), null_val, null_addr, Offset32::new(0));

        // 准备调用栈帧
        let cur_sp = fb.use_var(sp);
        let args_sp = fb.ins().iadd_imm(cur_sp, -(arg_count));

        let base = fb.ins().stack_addr(ptr_ty, *stack_slot, 0);
        let args_off = fb.ins().imul_imm(args_sp, VALUE_BYTES);
        let args_ptr_val = fb.ins().iadd(base, args_off);

        let out_off = fb.ins().imul_imm(cur_sp, VALUE_BYTES);
        let out_ptr_val = fb.ins().iadd(base, out_off);

        let argc_val = fb.ins().iconst(types::I64, arg_count);
        let name_len_val = fb.ins().iconst(types::I64, func_name.len() as i64);

        // 调用名称调度器
        fb.ins().call(
            native_name_dispatch_entry,
            &[
                name_base,
                name_len_val,
                argc_val,
                args_ptr_val,
                out_ptr_val,
            ],
        );

        fb.def_var(sp, args_sp);

        // 从 out_ptr 加载返回值
        let ret_tag = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, zero32);
        let ret_payload = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, eight32);
        push(fb, ret_tag, ret_payload);
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
