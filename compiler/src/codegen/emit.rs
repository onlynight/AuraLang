//! MIR → 字节码发射（对应 技术方案 §5.2 / §7.1）
//!
//! 将寄存器式 MIR 线性化为栈式字节码：
//! - 每个 `BasicBlock` 顺序发射，记录各块起始字节偏移
//! - 寄存器即为局部变量槽，`LoadLocal`/`StoreLocal` 映射为 `LoadVar`/`StoreVar`
//! - 控制流终结指令映射为 `Jump`/`JumpIfTrue`/`JumpIfFalse`，目标为绝对字节偏移
//! - 普通块顺序：条件块 → then 块（顺序落入）→ else 块 → merge 块

use crate::codegen::hir::HirProgram;
use crate::codegen::mir::{BasicBlock, LowerCtx, MirClosure, MirFunction, MirInstr, Terminator};
use crate::codegen::opcode::{
    BytecodeClosure, BytecodeFunction, BytecodeModule, BytecodeNative, ClassDef, Const, OpCode,
    TYPE_ID_BOOL, TYPE_ID_CSTRING, TYPE_ID_F64, TYPE_ID_I32, TYPE_ID_I64, TYPE_ID_PTR,
    TYPE_ID_VOID, VirtualTable,
};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// 虚方法名 → 槽位（emit_module 构建，单次发射内有效）
    static METHOD_SLOTS: RefCell<HashMap<String, u16>> = RefCell::new(HashMap::new());
}

/// 虚方法名 → 槽位编号
fn method_slot(name: &str) -> u16 {
    METHOD_SLOTS.with(|m| m.borrow().get(name).copied().unwrap_or(0))
}

/// 将 MIR 函数列表发射为字节码模块
pub fn emit_module(hir: &HirProgram, mir_funcs: &[MirFunction], ctx: &LowerCtx) -> BytecodeModule {
    // 原生函数表
    let natives: Vec<BytecodeNative> = hir
        .natives
        .iter()
        .map(|n| {
            // 映射 HIR 参数类型到 CType ID
            let param_types: Vec<u8> =
                n.params.iter().map(|p| hir_type_to_c_type_id(p.ty.as_ref())).collect();
            // 映射 HIR 返回类型到 CType ID
            let ret_type =
                n.ret.as_ref().map(|t| hir_type_to_c_type_id(Some(t))).unwrap_or(TYPE_ID_VOID);
            BytecodeNative {
                name: n.name.clone(),
                param_count: n.params.len() as u16,
                ffi_abi: n.ffi_abi,
                ffi_lib: n.ffi_lib.clone(),
                param_types,
                ret_type,
            }
        })
        .collect();

    // 用户函数名 -> 索引（预计算最终索引：闭包在前，用户函数在后）
    let mut fn_index: HashMap<&str, u16> = HashMap::new();
    // 统计所有闭包数量
    let total_closures: u16 = mir_funcs.iter().map(|f| f.closures.len() as u16).sum();
    // 用户函数索引 = 闭包总数 + 函数序号
    for (i, f) in mir_funcs.iter().enumerate() {
        fn_index.insert(f.name.as_str(), total_closures + i as u16);
    }
    // 原生函数名 -> 索引
    let mut native_index: HashMap<&str, u16> = HashMap::new();
    for (i, n) in natives.iter().enumerate() {
        native_index.insert(n.name.as_str(), i as u16);
    }

    // P-K2：虚方法表 —— 全局 open 方法槽位 + 每个类的分派表
    // 必须在函数发射之前构建，因为 emit_function 中 method_slot() 依赖 METHOD_SLOTS

    // Phase 1: 类定义表 —— 编译时分配递增类 ID（替代 FNV 哈希）
    // 0: Any（内置顶级类）
    let mut classes: Vec<ClassDef> = Vec::new();
    classes.push(ClassDef::builtin("Any"));
    // 类名 -> 类 ID 映射
    let mut class_id_map: HashMap<&str, u16> = HashMap::new();
    class_id_map.insert("Any", 0);
    // 为所有类分配递增 ID
    for s in &hir.structs {
        if s.is_class {
            let id = classes.len() as u16;
            class_id_map.insert(s.name.as_str(), id);
            classes.push(ClassDef::builtin(&s.name)); // 占位，稍后填充
        }
    }

    // 全局 open 方法槽位
    let mut slot_names: Vec<String> = Vec::new();
    for s in &hir.structs {
        for vm in &s.virtual_methods {
            if !slot_names.contains(vm) {
                slot_names.push(vm.clone());
            }
        }
    }
    let mut vtables: Vec<VirtualTable> = Vec::new();
    const NO_METHOD: u16 = u16::MAX;
    for s in hir.structs.iter().filter(|st| st.is_class) {
        // Phase 1: 使用类 ID（顺序分配）替代 FNV 哈希
        let tag = class_id_map.get(s.name.as_str()).copied().unwrap_or(0);
        let mut slots = vec![NO_METHOD; slot_names.len()];
        for (i, mname) in slot_names.iter().enumerate() {
            // 沿继承链从本类向上查找实现 `Class.method`
            let mut cur = Some(s.name.clone());
            while let Some(cn) = cur {
                if let Some(&fidx) = fn_index.get(format!("{}.{}", cn, mname).as_str()) {
                    slots[i] = fidx;
                    break;
                }
                cur = hir
                    .structs
                    .iter()
                    .find(|st| st.name == cn)
                    .and_then(|st| st.superclass.clone());
            }
        }
        vtables.push(VirtualTable {
            type_tag: tag,
            slots,
        });
    }
    // Phase 1: 填充类定义（继承链 + vtable 索引）
    for s in &hir.structs {
        if !s.is_class {
            continue;
        }
        let cid = class_id_map.get(s.name.as_str()).copied().unwrap_or(0);
        let parent_id =
            s.superclass.as_deref().and_then(|sc| class_id_map.get(sc).copied()).unwrap_or(0); // 无显式父类时默认继承 Any（class ID 0）
        let vtable_idx =
            vtables.iter().position(|vt| vt.type_tag == cid).map(|p| p as u16).unwrap_or(u16::MAX);
        classes[cid as usize] = ClassDef {
            name: s.name.clone(),
            parent_id,
            field_count: s.fields.len() as u16,
            vtable_idx,
            interfaces: Vec::new(),
            is_builtin: false,
            is_singleton: s.is_singleton,
            field_names: s.fields.iter().map(|f| f.0.clone()).collect(),
        };
    }
    METHOD_SLOTS.with(|m| {
        *m.borrow_mut() =
            slot_names.iter().enumerate().map(|(i, n)| (n.clone(), i as u16)).collect()
    });

    let mut functions = Vec::new();
    let mut closures: Vec<BytecodeClosure> = Vec::new();
    // 闭包名 -> 函数表索引（用于 MakeClosure 查找）
    let mut closure_fn_index: HashMap<&str, u16> = HashMap::new();

    // 第一遍：注册所有闭包到函数表
    for f in mir_funcs {
        for closure in &f.closures {
            let closure_code = emit_closure(
                closure,
                &fn_index,
                &native_index,
                &closure_fn_index,
                &class_id_map,
            );
            let fn_idx = functions.len() as u16;
            closure_fn_index.insert(closure.name.as_str(), fn_idx);
            functions.push(BytecodeFunction {
                name: closure.name.clone(),
                param_count: (closure.params.len() + closure.capture_names.len()) as u16,
                locals: closure.reg_count as u16,
                is_native: false,
                code: closure_code,
                line_table: None,
                aot_mode: 0,
                aot_desc_idx: 0,
            });
        }
    }

    // 第二遍：注册所有用户函数到函数表
    for f in mir_funcs {
        let code = emit_function(
            f,
            &fn_index,
            &native_index,
            &closure_fn_index,
            &class_id_map,
        );
        let fn_idx = functions.len() as u16;
        functions.push(BytecodeFunction {
            name: f.name.clone(),
            param_count: f.param_slots.len() as u16,
            locals: f.reg_count as u16,
            is_native: false,
            code,
            line_table: None,
            aot_mode: 0,
            aot_desc_idx: 0,
        });
        // 发射闭包记录（用于 MakeClosure 查找参数数量）
        for closure in &f.closures {
            let fn_idx = closure_fn_index.get(closure.name.as_str()).copied().unwrap_or(0);
            closures.push(BytecodeClosure {
                name: closure.name.clone(),
                param_count: closure.params.len() as u16,
                locals: closure.reg_count as u16,
                capture_count: closure.capture_names.len() as u16,
                func_idx: fn_idx,
            });
        }
    }

    // Entry 逻辑（脚本模式支持）：
    // 1. 有 main 函数 → 使用其索引（兼容模式）
    // 2. 无 main 但有函数 → 使用第一个函数索引（兼容行为）
    // 3. 无任何函数 → 报错
    let entry = if let Some(idx) = fn_index.get("main") {
        *idx // 兼容模式：有 main 函数
    } else if !mir_funcs.is_empty() {
        0 // 兼容行为：无 main 但有函数，执行第一个
    } else {
        // 无任何函数（无 main 且无顶层语句），由 VM 报错
        0
    };

    // P8.1: 合并 FFI 常量到常量池
    let mut consts = ctx.consts.clone();
    for (_name, c) in &hir.constants {
        consts.push(c.clone());
    }

    let module = BytecodeModule {
        consts,
        natives,
        functions,
        closures,
        entry,
        enabled_modules: Vec::new(),
        module_identity: crate::codegen::opcode::ModuleIdentity::default(),
        header_flags: if !classes.is_empty() {
            crate::codegen::opcode::HEADER_HAS_CLASS_DEFS
        } else {
            0
        },
        exports: Vec::new(),
        imports: Vec::new(),
        dependencies: Vec::new(),
        sig_ids: Vec::new(),
        entry_kind: "app".to_string(),
        aot_segments: Vec::new(),
        aot_blob_data: Vec::new(),
        vtables,
        classes,
        source_index: None,
    };
    // 清理 thread_local 槽表
    METHOD_SLOTS.with(|m| m.borrow_mut().clear());
    module
}

fn emit_function(
    f: &MirFunction,
    fn_index: &HashMap<&str, u16>,
    native_index: &HashMap<&str, u16>,
    closure_index: &HashMap<&str, u16>,
    class_id_map: &HashMap<&str, u16>,
) -> Vec<u8> {
    // 基本块布局：保证每个 `If` 的 `then` 块紧跟其条件块之后，
    // 这样 `JumpIfFalse(else)` 后自然 fallthrough 到 `then`，与块的物理创建顺序无关。
    let order = layout_blocks(f);

    // 第一遍：计算每个块的起始字节偏移
    let mut block_offsets = vec![0usize; f.blocks.len()];
    let mut off = 0usize;
    for &bid in &order {
        let b = &f.blocks[bid];
        block_offsets[bid] = off;
        for instr in &b.instrs {
            off += instr_size(instr);
        }
        off += term_size(&b.term);
    }

    // 第二遍：发射
    let mut code = Vec::with_capacity(off);
    for &bid in &order {
        let b = &f.blocks[bid];
        for instr in &b.instrs {
            emit_instr(
                &mut code,
                instr,
                fn_index,
                native_index,
                closure_index,
                &class_id_map,
                &block_offsets,
            );
        }
        match &b.term {
            Terminator::Goto(target) => {
                OpCode::Jump(block_offsets[*target] as i32).write(&mut code);
            }
            Terminator::If {
                cond,
                then_b,
                else_b,
            } => {
                // 顺序落入 then 块：先 JumpIfFalse(else)，then 自然顺序执行
                OpCode::LoadVar(*cond as u16).write(&mut code);
                OpCode::JumpIfFalse(block_offsets[*else_b] as i32).write(&mut code);
                // then 块顺序落入；其末尾 Goto(merge)
                let _ = then_b;
            }
            Terminator::Return(reg) => {
                OpCode::LoadVar(*reg as u16).write(&mut code);
                OpCode::Return.write(&mut code);
            }
            Terminator::ReturnVoid => {
                OpCode::ReturnUnit.write(&mut code);
            }
        }
    }
    code
}

/// 发射闭包代码（Phase 2）
fn emit_closure(
    closure: &MirClosure,
    fn_index: &HashMap<&str, u16>,
    native_index: &HashMap<&str, u16>,
    closure_index: &HashMap<&str, u16>,
    class_id_map: &HashMap<&str, u16>,
) -> Vec<u8> {
    let mut code = Vec::new();
    // 发射闭包指令
    for instr in &closure.body {
        emit_instr(
            &mut code,
            instr,
            fn_index,
            native_index,
            closure_index,
            class_id_map,
            &[],
        );
    }
    // 发射终结指令
    match &closure.term {
        Terminator::Return(reg) => {
            OpCode::LoadVar(*reg as u16).write(&mut code);
            OpCode::Return.write(&mut code);
        }
        Terminator::ReturnVoid => {
            OpCode::ReturnUnit.write(&mut code);
        }
        _ => {
            OpCode::ReturnUnit.write(&mut code);
        }
    }
    code
}

/// 计算基本块的发射顺序，使得每个 `If` 的 `then` 块紧邻其条件块之后。
///
/// 理由：字节码发射时 `Terminator::If` 依赖「顺序落入 then」的 fallthrough 语义，
/// 而 MIR 降级（尤其是嵌套控制流 / LICM 插入前置块）并不保证块在数组中的物理顺序
/// 与 fallthrough 一致。`emit` 仅使用绝对字节偏移作为跳转目标，因此只要
/// then 块在 `order` 中紧随其条件块，`JumpIfFalse(else)` 的 fallthrough 即正确。
fn layout_blocks(f: &MirFunction) -> Vec<usize> {
    let n = f.blocks.len();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut pending: Vec<usize> = vec![0]; // 待处理的 trace 起点（如 if 的 else 分支）
    while let Some(start) = pending.pop() {
        if visited[start] {
            continue;
        }
        // 沿 trace 向下：If 优先走 then（fallthrough），else 压入 pending 稍后处理；
        // Goto 顺次前进；Return/已访问块停止当前 trace。
        let mut cur = start;
        while !visited[cur] {
            visited[cur] = true;
            order.push(cur);
            match &f.blocks[cur].term {
                Terminator::If {
                    then_b,
                    else_b,
                    ..
                } => {
                    pending.push(*else_b);
                    cur = *then_b;
                }
                Terminator::Goto(t) => cur = *t,
                Terminator::Return(_) | Terminator::ReturnVoid => break,
            }
        }
    }
    // 兜底：理论上所有块均从入口可达；若有遗漏按 id 顺序补齐
    for i in 0..n {
        if !visited[i] {
            order.push(i);
        }
    }
    order
}

/// 单条 MIR 指令发射为字节码后的精确字节数（必须与 `emit_instr` 完全一致）
fn instr_size(instr: &crate::codegen::mir::MirInstr) -> usize {
    use crate::codegen::mir::MirInstr::*;
    match instr {
        // LoadConst/LoadVar/StoreVar 各 3 字节（操作码 + u16 操作数）
        LoadConst { .. } => 6,
        LoadLocal { .. } => 6,
        StoreLocal { .. } => 6,
        // BinOp：LoadVar(a) + LoadVar(b) + 算术(1) + StoreVar(dst) = 10；`To`/`Is`/`As` 仅 LoadVar(b)+StoreVar = 6
        BinOp { op, .. } => {
            if matches!(
                *op,
                crate::codegen::hir::HirBinOp::To
                    | crate::codegen::hir::HirBinOp::Is
                    | crate::codegen::hir::HirBinOp::As
            ) {
                6
            } else {
                10
            }
        }
        // UnOp：LoadVar(a) + op(1) + StoreVar(dst) = 7
        UnOp { .. } => 7,
        // Call：每个参数 LoadVar(3) + Call(3) + 可选 StoreVar(3)
        Call {
            args, dst, ..
        } => 3 * args.len() + 3 + if dst.is_some() { 3 } else { 0 },
        // CallNative：每个参数 LoadVar(3) + CallNativeArgs(5) + 可选 StoreVar(3)
        // 注意：emit_instr 发射的是 OpCode::CallNativeArgs（5字节），而非 CallNative（3字节）
        CallNative {
            args, dst, ..
        } => 3 * args.len() + 5 + if dst.is_some() { 3 } else { 0 },
        // CallClosure：每个参数 LoadVar(3) + LoadVar(closure)(3) + CallClosure(1) + 可选 StoreVar(3)
        CallClosure {
            args, dst, ..
        } => 3 * args.len() + 3 + 1 + if dst.is_some() { 3 } else { 0 },
        // Alloc：NewObject(3) + StoreVar(3) = 6
        Alloc { .. } => 6,
        // GetField：LoadVar(obj) + GetField(3) + StoreVar(3) = 9
        GetField { .. } => 9,
        // SetField：LoadVar(src) + LoadVar(obj) + SetField(3) = 9
        SetField { .. } => 9,
        // GetIndex：LoadVar(obj) + LoadVar(idx) + GetIndex(1) + StoreVar(3) = 10
        GetIndex { .. } => 10,
        // SetIndex：LoadVar(src) + LoadVar(obj) + LoadVar(idx) + SetIndex(1) = 10
        SetIndex { .. } => 10,
        // Retain：LoadVar(src) + Retain(1) = 4
        Retain { .. } => 4,
        // Release：LoadVar(src) + Release(1) = 4
        Release { .. } => 4,
        // WeakRef：LoadVar(src) + WeakRef(1) + StoreVar(dst) = 7
        WeakRef { .. } => 7,
        // WeakGet：LoadVar(src) + WeakGet(1) + StoreVar(dst) = 7
        WeakGet { .. } => 7,
        // Box：LoadVar(src) + BoxAlloc(1) + StoreVar(dst) = 7
        Box { .. } => 7,
        // MakeCallback：MakeCallback(3) + StoreVar(3) = 6
        MakeCallback { .. } => 6,
        // DeferBegin：DeferBegin(1) = 1
        DeferBegin => 1,
        // DeferEnd：DeferEnd(1) = 1
        DeferEnd => 1,
        // Yield：Yield(1) = 1
        Yield => 1,
        // MakeClosure：捕获参数 LoadVar(3) * n + MakeClosure(3) + StoreVar(3)
        MakeClosure {
            captures, ..
        } => 3 * captures.len() + 3 + 3,
        // EnumConstruct：枚举构造（Phase 3）
        EnumConstruct { .. } => 6,
        // EnumTag：枚举变体索引（Phase 3）
        EnumTag { .. } => 7,
        // MakeFnRef：函数引用（Phase 3）
        MakeFnRef { .. } => 6,
        // CallMethod（P-K2）：每个参数 LoadVar(3) + 接收者再 LoadVar(3) + CallMethod(3) + 可选 StoreVar(3)
        CallMethod {
            args, dst, ..
        } => 3 * args.len() + 3 + 3 + if dst.is_some() { 3 } else { 0 },
        // InstanceOf（Phase 2）：LoadVar(src)(3) + InstanceOf(3) + StoreVar(dst)(3) = 9
        InstanceOf { .. } => 9,
        // CheckCast（Phase 2）：LoadVar(src)(3) + CheckCast(3) + StoreVar(dst)(3) = 9
        CheckCast { .. } => 9,
        // PushHandler：PUSH_HANDLER(1 + i32) + u16 槽位 = 7
        PushHandler { .. } => 7,
        // PopHandler：POP_HANDLER(1) = 1
        PopHandler => 1,
    }
}

/// 终结指令发射为字节码后的精确字节数（必须与 `emit_function` 一致）
fn term_size(t: &Terminator) -> usize {
    match t {
        // Goto → JUMP (1 + i32 = 5)
        Terminator::Goto(_) => 5,
        // If → LoadVar(cond)(3) + JUMP_IF_FALSE(5) = 8
        Terminator::If { .. } => 8,
        // Return → LoadVar(3) + RETURN(1) = 4
        Terminator::Return(_) => 4,
        // ReturnVoid → RETURN_UNIT(1)
        Terminator::ReturnVoid => 1,
    }
}

fn emit_instr(
    code: &mut Vec<u8>,
    instr: &crate::codegen::mir::MirInstr,
    fn_index: &HashMap<&str, u16>,
    native_index: &HashMap<&str, u16>,
    closure_index: &HashMap<&str, u16>,
    class_id_map: &HashMap<&str, u16>,
    block_offsets: &[usize],
) {
    use crate::codegen::hir::HirBinOp::*;
    use crate::codegen::mir::MirInstr::*;
    match instr {
        LoadConst { dst, ci } => {
            OpCode::LoadConst(*ci as u16).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        LoadLocal { dst, slot } => {
            OpCode::LoadVar(*slot as u16).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        StoreLocal { slot, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::StoreVar(*slot as u16).write(code);
        }
        BinOp {
            dst,
            op,
            a,
            b,
        } => {
            if matches!(*op, To | Is | As) {
                // 近似：不发射运算，仅把 b 当作结果
                OpCode::LoadVar(*b as u16).write(code);
                OpCode::StoreVar(*dst as u16).write(code);
                return;
            }
            OpCode::LoadVar(*a as u16).write(code);
            OpCode::LoadVar(*b as u16).write(code);
            let oc = match op {
                Add => OpCode::Add,
                Sub => OpCode::Sub,
                Mul => OpCode::Mul,
                Div => OpCode::Div,
                Rem => OpCode::Rem,
                Eq => OpCode::Eq,
                Ne => OpCode::Ne,
                Lt => OpCode::Lt,
                Gt => OpCode::Gt,
                Le => OpCode::Le,
                Ge => OpCode::Ge,
                And => OpCode::And,
                Or => OpCode::Or,
                BitAnd => OpCode::BitAnd,
                BitOr => OpCode::BitOr,
                BitXor => OpCode::BitXor,
                Shl => OpCode::Shl,
                Shr => OpCode::Shr,
                To => OpCode::Add, // 不会到达
                // Phase 2: is/as 在 HIR 层已降级，不会到达此处
                Is => OpCode::Eq,  // 回退
                As => OpCode::Add, // 回退
            };
            oc.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        UnOp { dst, op, a } => {
            OpCode::LoadVar(*a as u16).write(code);
            match op {
                crate::codegen::hir::HirUnOp::Minus => OpCode::Neg,
                crate::codegen::hir::HirUnOp::Not => OpCode::Not,
            }
            .write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        Call {
            dst,
            func,
            args,
        } => {
            for a in args {
                OpCode::LoadVar(*a as u16).write(code);
            }
            let idx = fn_index.get(func.as_str()).copied().unwrap_or(0);
            OpCode::Call(idx).write(code);
            if let Some(d) = dst {
                OpCode::StoreVar(*d as u16).write(code);
            }
        }
        CallNative {
            dst,
            func,
            args,
        } => {
            for a in args {
                OpCode::LoadVar(*a as u16).write(code);
            }
            let idx = native_index.get(func.as_str()).copied().unwrap_or(0);
            OpCode::CallNativeArgs(idx, args.len() as u16).write(code);
            if let Some(d) = dst {
                OpCode::StoreVar(*d as u16).write(code);
            }
        }
        // Phase 2: 闭包调用
        CallClosure {
            dst,
            closure,
            args,
        } => {
            // 先加载参数，再加载闭包引用（栈顶为闭包）
            for a in args {
                OpCode::LoadVar(*a as u16).write(code);
            }
            OpCode::LoadVar(*closure as u16).write(code);
            OpCode::CallClosure.write(code);
            if let Some(d) = dst {
                OpCode::StoreVar(*d as u16).write(code);
            }
        }
        CallMethod {
            dst,
            method,
            args,
        } => {
            // 栈序：args（含 self）按序压栈，最后再压一次接收者（do_call_method 先弹对象）
            for a in args {
                OpCode::LoadVar(*a as u16).write(code);
            }
            if let Some(recv) = args.first() {
                OpCode::LoadVar(*recv as u16).write(code);
            }
            let slot = method_slot(method);
            OpCode::CallMethod(slot).write(code);
            if let Some(d) = dst {
                OpCode::StoreVar(*d as u16).write(code);
            }
        }
        InstanceOf {
            dst,
            src,
            type_id,
        } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::InstanceOf(*type_id).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        CheckCast {
            dst,
            src,
            type_id,
        } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::CheckCast(*type_id).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        PushHandler {
            handler,
            slot,
        } => {
            // 处理器块 id → 绝对字节偏移（与 Jump 同一套跳转目标约定）
            let off = block_offsets.get(*handler).copied().unwrap_or(0) as i32;
            OpCode::PushHandler(off, *slot).write(code);
        }
        PopHandler => {
            OpCode::PopHandler.write(code);
        }
        Alloc {
            dst,
            type_name,
        } => {
            // Phase 2: 使用类 ID（顺序分配）替代 FNV 哈希
            let idx = class_id_map
                .get(type_name.as_str())
                .copied()
                .unwrap_or_else(|| type_index(type_name));
            OpCode::NewObject(idx).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        GetField {
            dst,
            obj,
            field,
        } => {
            OpCode::LoadVar(*obj as u16).write(code);
            let idx = field_index(field);
            OpCode::GetField(idx).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        SetField {
            obj,
            field,
            src,
        } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::LoadVar(*obj as u16).write(code);
            let idx = field_index(field);
            OpCode::SetField(idx).write(code);
        }
        GetIndex {
            dst,
            obj,
            idx,
        } => {
            OpCode::LoadVar(*obj as u16).write(code);
            OpCode::LoadVar(*idx as u16).write(code);
            OpCode::GetIndex.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        SetIndex {
            obj,
            idx,
            src,
        } => {
            // 栈布局：值在下、索引在顶（与 SetField 的「值、对象」顺序一致扩展）
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::LoadVar(*obj as u16).write(code);
            OpCode::LoadVar(*idx as u16).write(code);
            OpCode::SetIndex.write(code);
        }
        Retain { src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::Retain.write(code);
        }
        Release { src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::Release.write(code);
        }
        WeakRef { dst, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::WeakRef.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        WeakGet { dst, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::WeakGet.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        Box { dst, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::BoxAlloc.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        MakeCallback { dst, func } => {
            let idx = fn_index.get(func.as_str()).copied().unwrap_or(0);
            OpCode::MakeCallback(idx).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        DeferBegin => {
            OpCode::DeferBegin.write(code);
        }
        DeferEnd => {
            OpCode::DeferEnd.write(code);
        }
        Yield => {
            OpCode::Yield.write(code);
        }
        // MakeClosure：闭包创建（Phase 2）
        MakeClosure {
            dst,
            func,
            captures,
        } => {
            // 1. 发射捕获值（按序压栈）
            for (_name, src) in captures {
                OpCode::LoadVar(*src as u16).write(code);
            }
            // 2. 发射 MakeClosure 指令（闭包在 closures 表中的索引）
            // 需要查找闭包名对应的 closures 表索引
            // 简化：使用 fn_index 查找（因为闭包函数也在函数表中）
            let idx = closure_index.get(func.as_str()).copied().unwrap_or(0);
            OpCode::MakeClosure(idx).write(code);
            // 3. 将结果存入目标寄存器
            OpCode::StoreVar(*dst as u16).write(code);
        }
        // EnumConstruct：枚举构造（Phase 3）
        EnumConstruct {
            dst,
            enum_name,
            variant_idx,
        } => {
            // 使用 EnumConstruct opcode（操作数为变体索引）
            let _ = enum_name;
            OpCode::EnumConstruct(*variant_idx).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        // EnumTag：枚举变体索引（Phase 3）
        EnumTag { dst, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::EnumTag.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        // MakeFnRef：函数引用（Phase 3）
        MakeFnRef { dst, func } => {
            let idx = fn_index.get(func.as_str()).copied().unwrap_or(0);
            OpCode::MakeFnRef(idx).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
    }
}

/// 类型名 -> 类型表索引（简化：字符串哈希）
fn type_index(name: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 65535) as u16
}

/// 字段名 -> 字段表索引（简化：字符串哈希）
/// 字段名 → 索引（FNV-1a 哈希，`GetField`/`SetField` 指令携带）。
///
/// 公开给 VM：集合/字符串的内建成员访问（`size`/`first`/`last`/`isEmpty`）
/// 通过同一哈希在运行期识别字段名。
pub fn field_index(name: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 65535) as u16
}

/// 工具：常量池查找（供优化器/反汇编器复用）
pub fn find_const<'a>(module: &'a BytecodeModule, idx: usize) -> &'a Const {
    &module.consts[idx]
}

/// 将 HIR 类型映射到 CType ID（用于 BytecodeNative 的 param_types/ret_type）
fn hir_type_to_c_type_id(ty: Option<&crate::codegen::hir::HirType>) -> u8 {
    use crate::codegen::hir::HirType;
    match ty {
        Some(HirType::Named(name)) => match name.as_str() {
            "Int" | "Long" | "Short" | "Byte" | "U8" | "Char" => TYPE_ID_I32,
            "Float" | "Double" => TYPE_ID_F64,
            "Boolean" | "Bool" => TYPE_ID_BOOL,
            "String" | "Str" | "CString" | "CStr" => TYPE_ID_CSTRING,
            _ => TYPE_ID_I32, // 默认 i32
        },
        Some(HirType::Pointer(_)) => TYPE_ID_PTR,
        _ => TYPE_ID_VOID,
    }
}
