//! Aura 虚拟机（P5）
//!
//! 对应 技术方案 §7.1 / §7.2：栈式字节码解释器 + 直接线程码分发 + 热点 JIT 接缝。
//!
//! ## 架构
//! - **字节码加载**：`.auc` 反序列化为 [`BytecodeModule`] 后，预解码为
//!   [`DecodedFunction`]（指令流 + 跳转目标已解析为指令索引），避免解释期重复解析操作数。
//! - **执行引擎**：显式调用帧栈（`Vec<Frame>`），每帧持有独立操作数栈与局部变量槽；
//!   主循环对栈顶帧逐条 `exec_instr`，`Call`/`Return` 切换帧。
//! - **直接线程码**：Rust 无 `computed goto`，这里用「预解码指令 + 紧凑 `match` 分派」
//!   达到同等效果——每条指令的操作数在加载期解析完毕，解释期零解析开销。
//! - **原生调度**：`CallNative`/`CallC` 经 [`NativeRegistry`] 分发到 Rust 实现的内置函数。
//! - **堆与 ARC**：对象 / 数组存于 [`Heap`]，`IncRef`/`DecRef` 维护引用计数。
//! - **热点 JIT**：[`VmOptions::jit`] 开启且 `jit` feature 编译时，累计调用超过
//!   阈值的（叶子整数）函数由 Cranelift 编译为原生代码并缓存，后续调用直接派发到
//!   原生入口（`5.12`）；编译失败则永久回退解释器（`5.13`）。

pub mod abi;
pub mod actor;
pub mod actor_process;
pub mod aot_runtime;
pub mod channel;
pub mod channel_tcp;
pub mod coroutine;
pub mod debugger;
pub mod dynamic_ffi;
pub mod ffi;
pub mod heap;
pub mod interp;
pub mod ipc;
#[cfg(feature = "jit")]
pub mod jit;
#[cfg(feature = "jit")]
pub mod jit_native;
#[cfg(feature = "jit")]
pub mod jit_opt;
pub mod mmap_util;
pub mod native;
pub mod serialize;
pub mod thread_pool;
pub mod value;

pub use aot_runtime::AotRuntime;
pub use coroutine::{CoroutineScheduler, CoroutineState};
pub use dynamic_ffi::DynamicLoader;
pub use ffi::{
    CallbackRegistry, clear_dispatcher, resolve_static_symbol, set_dispatcher, trampoline_ptr,
};
pub use heap::Heap;
pub use native::NativeRegistry;
pub use value::Value;

use std::collections::HashMap;

use crate::codegen::opcode::{BytecodeFunction, BytecodeModule, BytecodeNative, Const};

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: 模块注册表
// ─────────────────────────────────────────────────────────────────────────────

/// 已加载模块的记录（Phase 2 ModuleRegistry 用）
#[derive(Debug, Clone)]
pub struct RegisteredModule {
    /// 模块唯一标识（UUID）
    pub uuid: [u8; 16],
    /// 模块名称
    pub name: String,
    /// 模块版本
    pub version: String,
    /// 导出符号索引：name -> (export_idx, func_idx)
    pub export_index: HashMap<String, (u16, u16)>,
    /// 字节码模块
    pub module: BytecodeModule,
}

impl RegisteredModule {
    /// 从字节码模块创建已注册模块记录
    pub fn from_module(module: &BytecodeModule) -> Self {
        let mut export_index = HashMap::new();
        for (i, exp) in module.exports.iter().enumerate() {
            if let Some(func_idx) = exp.func_idx {
                export_index.insert(exp.name.clone(), (i as u16, func_idx));
            }
        }
        RegisteredModule {
            uuid: module.module_identity.uuid,
            name: module.module_identity.name.clone(),
            version: module.module_identity.version.clone(),
            export_index,
            module: module.clone(),
        }
    }
}

/// 模块注册表 — 跟踪所有已加载模块
#[derive(Debug, Default)]
pub struct ModuleRegistry {
    /// UUID -> 已加载模块
    pub modules: HashMap<[u8; 16], RegisteredModule>,
    /// 名称 -> UUID
    pub name_index: HashMap<String, [u8; 16]>,
}

impl ModuleRegistry {
    /// 创建空的模块注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册模块
    pub fn register(&mut self, module: &BytecodeModule) -> Result<[u8; 16], String> {
        let uuid = module.module_identity.uuid;
        let name = module.module_identity.name.clone();
        if self.name_index.contains_key(&name) {
            return Err(format!("模块名称冲突: {}", name));
        }
        let loaded = RegisteredModule::from_module(module);
        self.name_index.insert(name, uuid);
        self.modules.insert(uuid, loaded);
        Ok(uuid)
    }

    /// 按 UUID 查找模块
    pub fn find_by_uuid(&self, uuid: &[u8; 16]) -> Option<&RegisteredModule> {
        self.modules.get(uuid)
    }

    /// 按名称查找模块
    pub fn find_by_name(&self, name: &str) -> Option<&RegisteredModule> {
        self.name_index.get(name).and_then(|uuid| self.modules.get(uuid))
    }

    /// 按名称查找导出符号
    pub fn find_export(&self, module_name: &str, symbol: &str) -> Option<&RegisteredModule> {
        self.find_by_name(module_name).and_then(|m| {
            if m.export_index.contains_key(symbol) { Some(m) } else { None }
        })
    }

    /// 列出所有已加载模块
    pub fn list_modules(&self) -> Vec<&RegisteredModule> {
        self.modules.values().collect()
    }
}

/// VM 配置
#[derive(Debug, Clone)]
pub struct VmOptions {
    /// 最大调用帧深度（防止失控递归导致栈/堆爆炸）
    pub max_call_depth: usize,
    /// 是否启用 JIT 编译热点函数（需 `jit` feature）
    pub jit: bool,
    /// 热点阈值：函数累计调用次数超过该值即标记为热点
    pub hotspot_threshold: u64,
}

impl Default for VmOptions {
    fn default() -> Self {
        VmOptions {
            max_call_depth: 4096,
            jit: false,
            hotspot_threshold: 10_000,
        }
    }
}

/// VM 错误
#[derive(Debug, Clone)]
pub enum VmError {
    /// 字节码格式 / 加载错误
    Load(String),
    /// 运行时错误（如调用不存在的函数、栈下溢）
    Runtime(String),
    /// 入口函数缺失
    NoEntry(String),
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::Load(m) => write!(f, "load error: {}", m),
            VmError::Runtime(m) => write!(f, "runtime error: {}", m),
            VmError::NoEntry(m) => write!(f, "no entry function: {}", m),
        }
    }
}

impl std::error::Error for VmError {}

/// 解码后的指令（操作数已在加载期解析为索引）
#[derive(Debug, Clone)]
pub enum Instr {
    LoadConst(u16),
    LoadVar(u16),
    StoreVar(u16),

    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Neg,
    Not,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,

    /// 操作数为「指令索引」（加载期由字节偏移解析得到）
    Jump(usize),
    JumpIfTrue(usize),
    JumpIfFalse(usize),

    Call(u16),
    CallNative(u16),
    CallNativeArgs(u16, u16),
    Return,
    ReturnUnit,

    NewObject(u16),
    NewArray,
    GetField(u16),
    SetField(u16),
    GetIndex,
    SetIndex,

    IncRef,
    DecRef,

    CallC(u16),

    // ── 方法 / 接口调用（5.6） ──
    CallMethod(u16),
    CallCtor(u16),

    // ── 类型检查（Phase 2） ──
    /// 实例类型检查：栈顶为值，class_id 为目标类 ID，Boolean 结果压栈
    InstanceOf(u16),
    /// 类型转换：栈顶为值，class_id 为目标类 ID，匹配则压栈原引用
    CheckCast(u16),

    // ── 集合类型（5.7） ──
    NewList,
    NewMap,
    ListPush,
    ListPop,
    ListLen,
    MapSet,
    MapGet,
    MapLen,

    // ── 协程（5.8） ──
    Yield,
    NewCoroutine(u16),
    ResumeCoroutine,

    // ── ARC 生命周期（5.10） ──
    DropRef,

    // ── P7 内存管理 ──
    /// 保留引用计数 +1（P7.2）
    Retain,
    /// 释放引用计数 -1（P7.2）
    Release,
    /// 创建弱引用（P7.3）
    WeakRef,
    /// 从弱引用升级（P7.3）
    WeakGet,
    /// 显式堆分配（P7.5）
    BoxAlloc,
    /// defer 清理块开始（P7.4）
    DeferBegin,
    /// defer 清理块结束（P7.4）
    DeferEnd,

    Halt,

    // ── FFI（P8）──
    /// 将栈顶字符串转换为 C 字符串指针
    CString,
    /// 从栈顶的 C 字符串指针读取字符串
    ReadCStr,
    /// 栈顶指针是否为 nullptr
    PtrIsNull,
    /// 将栈顶指针转换为整数地址
    PtrToInt,
    /// 将栈顶整数地址转换为指针
    IntToPtr,
    /// 创建 C 回调蹦床
    MakeCallback(u16),
    /// 创建闭包（Phase 2）
    MakeClosure(u16),
    /// 调用闭包（Phase 2）
    CallClosure,
    /// 构造枚举变体（Phase 3）
    EnumConstruct(u16),
    /// 获取枚举变体索引（Phase 3）
    EnumTag,
    /// 创建函数引用（Phase 3）
    MakeFnRef(u16),

    // ── Phase 2: 跨模块调用 ──
    /// 调用同模块内导出符号
    CallExport(u16),
    /// 调用外部模块符号
    CallExternal(u16, u16),

    // ── Phase 1: AOT 嵌入调用 ──
    /// 调用 AOT 预编译函数（`func_idx` 为函数表索引），经 [`crate::vm::aot_runtime::AotRuntime`]
    /// dispatch_table 查找到 mmap 的机器码入口，使用共享 JitValue ABI 直接 `call`。
    CallAot(u16),
}

/// 解码后的函数
#[derive(Debug, Clone)]
pub struct DecodedFunction {
    pub name: String,
    pub param_count: u16,
    pub locals: u16,
    pub is_native: bool,
    /// 预解码指令流
    pub code: Vec<Instr>,
}

/// 加载后的模块（供解释器直接执行）
pub struct LoadedModule {
    pub consts: Vec<Const>,
    pub natives: Vec<BytecodeNative>,
    pub funcs: Vec<DecodedFunction>,
    pub entry: u16,
    native_index: HashMap<String, u16>,
    /// Phase 2: 原始字节码模块引用（用于访问 exports/imports 表）
    pub module: BytecodeModule,
}

impl LoadedModule {
    /// 从字节码模块加载：解码所有函数、建立原生索引
    pub fn from_module(m: &BytecodeModule) -> Result<Self, VmError> {
        let mut native_index = HashMap::new();
        for (i, n) in m.natives.iter().enumerate() {
            native_index.insert(n.name.clone(), i as u16);
        }

        let mut funcs = Vec::with_capacity(m.functions.len());
        for f in &m.functions {
            funcs.push(decode_function(f)?);
        }

        Ok(LoadedModule {
            consts: m.consts.clone(),
            natives: m.natives.clone(),
            funcs,
            entry: m.entry,
            native_index,
            module: m.clone(),
        })
    }

    pub fn native_index(&self, name: &str) -> Option<u16> {
        self.native_index.get(name).copied()
    }
}

/// 将单个函数的字节码解码为指令流
fn decode_function(f: &BytecodeFunction) -> Result<DecodedFunction, VmError> {
    let code = &f.code;
    let mut instrs: Vec<Instr> = Vec::new();
    // 每条指令起始字节偏移，用于跳转目标解析
    let mut starts: Vec<usize> = Vec::new();

    let mut ip = 0usize;
    while ip < code.len() {
        starts.push(ip);
        let byte = code[ip];
        let op = crate::codegen::opcode::OpCode::from_byte(byte)
            .ok_or_else(|| VmError::Load(format!("unknown opcode {}", byte)))?;
        ip += 1;
        match op {
            crate::codegen::opcode::OpCode::LoadConst(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::LoadConst(v));
            }
            crate::codegen::opcode::OpCode::LoadVar(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::LoadVar(v));
            }
            crate::codegen::opcode::OpCode::StoreVar(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::StoreVar(v));
            }
            crate::codegen::opcode::OpCode::Add => instrs.push(Instr::Add),
            crate::codegen::opcode::OpCode::Sub => instrs.push(Instr::Sub),
            crate::codegen::opcode::OpCode::Mul => instrs.push(Instr::Mul),
            crate::codegen::opcode::OpCode::Div => instrs.push(Instr::Div),
            crate::codegen::opcode::OpCode::Rem => instrs.push(Instr::Rem),
            crate::codegen::opcode::OpCode::Neg => instrs.push(Instr::Neg),
            crate::codegen::opcode::OpCode::Not => instrs.push(Instr::Not),
            crate::codegen::opcode::OpCode::And => instrs.push(Instr::And),
            crate::codegen::opcode::OpCode::Or => instrs.push(Instr::Or),
            crate::codegen::opcode::OpCode::BitAnd => instrs.push(Instr::BitAnd),
            crate::codegen::opcode::OpCode::BitOr => instrs.push(Instr::BitOr),
            crate::codegen::opcode::OpCode::BitXor => instrs.push(Instr::BitXor),
            crate::codegen::opcode::OpCode::Shl => instrs.push(Instr::Shl),
            crate::codegen::opcode::OpCode::Shr => instrs.push(Instr::Shr),
            crate::codegen::opcode::OpCode::Eq => instrs.push(Instr::Eq),
            crate::codegen::opcode::OpCode::Ne => instrs.push(Instr::Ne),
            crate::codegen::opcode::OpCode::Lt => instrs.push(Instr::Lt),
            crate::codegen::opcode::OpCode::Gt => instrs.push(Instr::Gt),
            crate::codegen::opcode::OpCode::Le => instrs.push(Instr::Le),
            crate::codegen::opcode::OpCode::Ge => instrs.push(Instr::Ge),
            crate::codegen::opcode::OpCode::Jump(_) => {
                let off = i32::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                    code[ip + 2],
                    code[ip + 3],
                ]);
                ip += 4;
                // 暂存字节偏移，稍后解析为指令索引
                instrs.push(Instr::Jump(off as usize));
            }
            crate::codegen::opcode::OpCode::JumpIfTrue(_) => {
                let off = i32::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                    code[ip + 2],
                    code[ip + 3],
                ]);
                ip += 4;
                instrs.push(Instr::JumpIfTrue(off as usize));
            }
            crate::codegen::opcode::OpCode::JumpIfFalse(_) => {
                let off = i32::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                    code[ip + 2],
                    code[ip + 3],
                ]);
                ip += 4;
                instrs.push(Instr::JumpIfFalse(off as usize));
            }
            crate::codegen::opcode::OpCode::Call(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::Call(v));
            }
            crate::codegen::opcode::OpCode::CallNative(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::CallNative(v));
            }
            crate::codegen::opcode::OpCode::CallNativeArgs(_, _) => {
                let idx = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                let argc = u16::from_le_bytes([
                    code[ip + 2],
                    code[ip + 3],
                ]);
                ip += 4;
                instrs.push(Instr::CallNativeArgs(idx, argc));
            }
            crate::codegen::opcode::OpCode::Return => instrs.push(Instr::Return),
            crate::codegen::opcode::OpCode::ReturnUnit => instrs.push(Instr::ReturnUnit),
            crate::codegen::opcode::OpCode::NewObject(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::NewObject(v));
            }
            crate::codegen::opcode::OpCode::NewArray => instrs.push(Instr::NewArray),
            crate::codegen::opcode::OpCode::GetField(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::GetField(v));
            }
            crate::codegen::opcode::OpCode::SetField(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::SetField(v));
            }
            crate::codegen::opcode::OpCode::GetIndex => instrs.push(Instr::GetIndex),
            crate::codegen::opcode::OpCode::SetIndex => instrs.push(Instr::SetIndex),
            crate::codegen::opcode::OpCode::IncRef => instrs.push(Instr::IncRef),
            crate::codegen::opcode::OpCode::DecRef => instrs.push(Instr::DecRef),
            crate::codegen::opcode::OpCode::CallC(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::CallC(v));
            }
            crate::codegen::opcode::OpCode::CallMethod(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::CallMethod(v));
            }
            crate::codegen::opcode::OpCode::CallCtor(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::CallCtor(v));
            }
            crate::codegen::opcode::OpCode::InstanceOf(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::InstanceOf(v));
            }
            crate::codegen::opcode::OpCode::CheckCast(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::CheckCast(v));
            }
            crate::codegen::opcode::OpCode::NewList => instrs.push(Instr::NewList),
            crate::codegen::opcode::OpCode::NewMap => instrs.push(Instr::NewMap),
            crate::codegen::opcode::OpCode::ListPush => instrs.push(Instr::ListPush),
            crate::codegen::opcode::OpCode::ListPop => instrs.push(Instr::ListPop),
            crate::codegen::opcode::OpCode::ListLen => instrs.push(Instr::ListLen),
            crate::codegen::opcode::OpCode::MapSet => instrs.push(Instr::MapSet),
            crate::codegen::opcode::OpCode::MapGet => instrs.push(Instr::MapGet),
            crate::codegen::opcode::OpCode::MapLen => instrs.push(Instr::MapLen),
            crate::codegen::opcode::OpCode::Yield => instrs.push(Instr::Yield),
            crate::codegen::opcode::OpCode::NewCoroutine(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::NewCoroutine(v));
            }
            crate::codegen::opcode::OpCode::ResumeCoroutine => instrs.push(Instr::ResumeCoroutine),
            crate::codegen::opcode::OpCode::DropRef => instrs.push(Instr::DropRef),
            crate::codegen::opcode::OpCode::Retain => instrs.push(Instr::Retain),
            crate::codegen::opcode::OpCode::Release => instrs.push(Instr::Release),
            crate::codegen::opcode::OpCode::WeakRef => instrs.push(Instr::WeakRef),
            crate::codegen::opcode::OpCode::WeakGet => instrs.push(Instr::WeakGet),
            crate::codegen::opcode::OpCode::BoxAlloc => instrs.push(Instr::BoxAlloc),
            crate::codegen::opcode::OpCode::DeferBegin => instrs.push(Instr::DeferBegin),
            crate::codegen::opcode::OpCode::DeferEnd => instrs.push(Instr::DeferEnd),
            crate::codegen::opcode::OpCode::Halt => instrs.push(Instr::Halt),
            crate::codegen::opcode::OpCode::CString => instrs.push(Instr::CString),
            crate::codegen::opcode::OpCode::ReadCStr => instrs.push(Instr::ReadCStr),
            crate::codegen::opcode::OpCode::PtrIsNull => instrs.push(Instr::PtrIsNull),
            crate::codegen::opcode::OpCode::PtrToInt => instrs.push(Instr::PtrToInt),
            crate::codegen::opcode::OpCode::IntToPtr => instrs.push(Instr::IntToPtr),
            crate::codegen::opcode::OpCode::MakeCallback(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::MakeCallback(v));
            }
            crate::codegen::opcode::OpCode::MakeClosure(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::MakeClosure(v));
            }
            crate::codegen::opcode::OpCode::CallClosure => {
                instrs.push(Instr::CallClosure);
            }
            crate::codegen::opcode::OpCode::EnumConstruct(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::EnumConstruct(v));
            }
            crate::codegen::opcode::OpCode::EnumTag => {
                instrs.push(Instr::EnumTag);
            }
            crate::codegen::opcode::OpCode::MakeFnRef(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::MakeFnRef(v));
            }
            crate::codegen::opcode::OpCode::CallExport(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::CallExport(v));
            }
            crate::codegen::opcode::OpCode::CallExternal(_, _) => {
                let mod_idx = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                let sym_idx = u16::from_le_bytes([
                    code[ip + 2],
                    code[ip + 3],
                ]);
                ip += 4;
                instrs.push(Instr::CallExternal(mod_idx, sym_idx));
            }
            crate::codegen::opcode::OpCode::CallAot(_) => {
                let v = u16::from_le_bytes([
                    code[ip],
                    code[ip + 1],
                ]);
                ip += 2;
                instrs.push(Instr::CallAot(v));
            }
        }
    }

    // 将跳转的「字节偏移」解析为「指令索引」
    let offset_to_idx: HashMap<usize, usize> =
        starts.iter().enumerate().map(|(i, s)| (*s, i)).collect();
    for instr in instrs.iter_mut() {
        match instr {
            Instr::Jump(off) | Instr::JumpIfTrue(off) | Instr::JumpIfFalse(off) => {
                let idx = *offset_to_idx.get(off).ok_or_else(|| {
                    VmError::Load(format!("jump target {} not at instruction boundary", off))
                })?;
                *instr = match instr {
                    Instr::Jump(_) => Instr::Jump(idx),
                    Instr::JumpIfTrue(_) => Instr::JumpIfTrue(idx),
                    _ => Instr::JumpIfFalse(idx),
                };
            }
            _ => {}
        }
    }

    Ok(DecodedFunction {
        name: f.name.clone(),
        param_count: f.param_count,
        locals: f.locals,
        is_native: f.is_native,
        code: instrs,
    })
}

/// 调用帧
#[derive(Debug, Clone)]
pub struct Frame {
    /// 所属函数索引
    pub func: usize,
    /// 当前指令索引
    pub ip: usize,
    /// 局部变量槽（含参数槽 0..param_count）
    pub locals: Vec<Value>,
    /// 操作数栈
    pub stack: Vec<Value>,
    /// 协程 ID（0 = 主线程）
    pub coroutine_id: usize,
}

impl Frame {
    fn new(func: &DecodedFunction, args: Vec<Value>, coroutine_id: usize) -> Self {
        let mut locals = vec![Value::Null; func.locals as usize];
        let n = func.param_count as usize;
        for (i, a) in args.into_iter().take(n).enumerate() {
            locals[i] = a;
        }
        Frame {
            func: 0,
            ip: 0,
            locals,
            stack: Vec::new(),
            coroutine_id,
        }
    }
}

/// Aura 虚拟机实例
pub struct Vm {
    module: LoadedModule,
    natives: NativeRegistry,
    heap: Heap,
    frames: Vec<Frame>,
    /// 各函数累计调用次数（热点检测，5.11）
    call_counts: Vec<u64>,
    /// 入口函数返回值
    result: Option<Value>,
    /// object 单例实例（类名 → 堆引用）
    singletons: std::collections::HashMap<String, Value>,
    halt: bool,
    opts: VmOptions,
    #[cfg(feature = "jit")]
    jit: Option<crate::vm::jit::JitState>,
    /// 协程调度器（5.8）：协程 ID → 协程状态
    pub coroutines: CoroutineScheduler,
    /// 回调注册表（P8.7）：Aura 函数 → C 回调蹦床
    pub callbacks: CallbackRegistry,
    /// Actor 运行时（P10.4）：Actor 实例 → 消息队列
    pub actors: crate::vm::actor::ActorRuntime,
    /// Channel 运行时（P10.8）：Channel 实例 → 缓冲区
    pub channels: crate::vm::channel::ChannelRuntime,
    /// Phase 2: 模块注册表
    pub registry: ModuleRegistry,
    /// Phase 1: AOT 运行时（机器码嵌入模块管理）
    pub aot_runtime: crate::vm::aot_runtime::AotRuntime,
    /// extern interface: 已加载的 AOT 模块映射（库名 → module_id）
    aot_module_map: std::collections::HashMap<String, u32>,
    /// P9: 已加载的动态库（库名 → 库句柄）
    #[cfg(windows)]
    loaded_libs: std::collections::HashMap<String, usize>,
    #[cfg(unix)]
    loaded_libs: std::collections::HashMap<String, *mut std::os::raw::c_void>,
}

impl Vm {
    /// 从字节码模块创建 VM
    pub fn new(module: &BytecodeModule, opts: VmOptions) -> Result<Self, VmError> {
        let loaded = LoadedModule::from_module(module)?;
        // Phase 1c: 按需注册 std 模块
        // 如果 enabled_modules 为空（无 import 声明），使用全量注册（向后兼容）
        let natives = if module.enabled_modules.is_empty() {
            NativeRegistry::new()
        } else {
            let modules_refs: Vec<&str> =
                module.enabled_modules.iter().map(|s| s.as_str()).collect();
            NativeRegistry::with_modules(&modules_refs)
        };
        // Phase 1: AOT 机器码嵌入 —— 若 `.auc` 含机器码段则加载到 AotRuntime。
        // 加载失败不影响 VM 创建：未命中 dispatch_table 的函数回退字节码解释。
        let mut aot_runtime = crate::vm::aot_runtime::AotRuntime::new();
        if module.has_aot() {
            let desc_idx: Vec<u32> = module.functions.iter().map(|f| f.aot_desc_idx).collect();
            let name = if module.module_identity.name.is_empty() {
                "aot_module".to_string()
            } else {
                module.module_identity.name.clone()
            };
            if let Err(e) = aot_runtime.load_module_from(
                &module.aot_blob_data,
                &module.aot_segments,
                &desc_idx,
                name,
            ) {
                eprintln!("[vm] AOT 模块加载失败，回退字节码解释: {}", e);
            }
        }
        let mut vm = Vm {
            module: loaded,
            natives,
            heap: Heap::new(),
            frames: Vec::new(),
            call_counts: vec![0; module.functions.len()],
            result: None,
            singletons: std::collections::HashMap::new(),
            halt: false,
            opts,
            #[cfg(feature = "jit")]
            jit: if cfg!(feature = "jit") { Some(crate::vm::jit::JitState::new()) } else { None },
            coroutines: CoroutineScheduler::new(),
            callbacks: CallbackRegistry::new(),
            actors: crate::vm::actor::ActorRuntime::new(),
            channels: crate::vm::channel::ChannelRuntime::new(),
            registry: ModuleRegistry::new(),
            aot_runtime,
            aot_module_map: std::collections::HashMap::new(),
            #[cfg(windows)]
            loaded_libs: std::collections::HashMap::new(),
            #[cfg(unix)]
            loaded_libs: std::collections::HashMap::new(),
        };
        #[cfg(feature = "jit")]
        vm.set_global_native_registry();
        // Phase 2: 创建 object 单例实例
        vm.create_singletons();
        Ok(vm)
    }

    /// 为所有 `is_singleton` 类创建单例实例
    fn create_singletons(&mut self) {
        for (type_id, class_def) in self.module.module.classes.iter().enumerate() {
            if class_def.is_singleton {
                // 分配堆对象
                let handle = self.heap.alloc_object(type_id as u16);
                // 初始化字段默认值
                for i in 0..class_def.field_count as u16 {
                    self.heap.set_field(handle, i, Value::Null);
                }
                self.singletons.insert(class_def.name.clone(), Value::Ref(handle));
            }
        }
    }

    /// 设置全局 NativeRegistry 指针（JIT 原生调度器使用）
    #[cfg(feature = "jit")]
    pub fn set_global_native_registry(&mut self) {
        crate::vm::jit_native::set_native_registry(&mut self.natives);
        crate::vm::jit_native::set_natives_ptr(self.module.natives.as_ptr());
    }

    /// 注册额外原生函数（供 FFI / 标准库扩展）
    pub fn register_native(&mut self, name: &str, f: native::NativeFn) {
        self.natives.register(name, f);
    }

    /// 检查是否注册了指定的原生函数
    pub fn contains_native(&self, name: &str) -> bool {
        self.natives.contains(name)
    }

    /// 执行入口函数，返回其返回值
    ///
    /// **JIT 入口派发（Fix A）**：当 `jit` 开启时，入口函数在首次运行时强制
    /// JIT 编译（忽略调用阈值）。编译成功后直接派发到原生入口，绕过解释器
    /// 主循环——这解决了「入口函数无调用计数」导致循环热点永远无法触发 JIT 的问题
    /// （见 docs/JIT性能分析.md §3.2）。
    pub fn run(&mut self) -> Result<Value, VmError> {
        let entry = self.module.entry as usize;
        if entry >= self.module.funcs.len() {
            return Err(VmError::NoEntry(format!(
                "no entry function (module has {} functions, entry={}, max={})",
                self.module.funcs.len(),
                entry,
                self.module.funcs.len().saturating_sub(1)
            )));
        }

        // P10: 设置并发运行时 VM 引用（供原生函数访问 Actor/Channel 状态）
        crate::vm::native::set_vm_ref(self as *mut Self as *mut ());

        // P8.7: 设置回调派发闭包（C 蹦床通过 thread-local 派发回 VM）
        {
            use crate::vm::ffi::set_dispatcher;
            use std::sync::atomic::AtomicPtr;
            let vm_ptr = std::sync::Arc::new(AtomicPtr::new(self as *mut Vm));
            let vm_ptr_clone = vm_ptr.clone();
            let dispatcher = std::sync::Arc::new(move |callback_id: i64, args: &[i64]| {
                use std::sync::atomic::Ordering;
                let ptr = vm_ptr_clone.load(Ordering::SeqCst);
                if ptr.is_null() {
                    return 0;
                }
                unsafe { (*ptr).call_callback(callback_id, args) }
            });
            set_dispatcher(dispatcher);
        }

        #[cfg(feature = "jit")]
        {
            if self.opts.jit {
                // Fix A：入口函数强制 JIT 编译（忽略调用阈值）
                self.force_jit_compile(entry);
                if let Some(jit) = self.jit.as_ref() {
                    if jit.is_compiled(entry) {
                        // 直接派发入口函数到 JIT 原生码（C ABI：args, out, argc）
                        let mut out = crate::vm::jit::JitValue::null();
                        let jargs: Vec<crate::vm::jit::JitValue> = Vec::new();
                        // Safety: out 指向合法 JitValue 缓冲；entry 已在上面确认编译成功
                        unsafe {
                            jit.invoke(entry, jargs.as_ptr(), &mut out, 0);
                        }
                        return Ok(out.to_value());
                    }
                }
            }
        }

        self.push_frame(entry, Vec::new())?;
        while !self.frames.is_empty() && !self.halt {
            self.step()?;
        }
        // P8.7: 清除回调派发闭包
        crate::vm::ffi::clear_dispatcher();
        // P10: 清除并发运行时 VM 引用
        crate::vm::native::clear_vm_ref();
        Ok(self.result.take().unwrap_or(Value::Null))
    }

    /// 回调派发入口（P8.7）：被 C 蹦床通过 thread-local 派发闭包调用
    ///
    /// 从回调 ID 查注册表，获取 Aura 函数索引，推送新帧并执行。
    /// 执行完毕后返回结果（i64）。
    ///
    /// # Safety
    ///
    /// 此方法由 C ABI 蹦床通过 `set_dispatcher` 设置的闭包调用。
    /// 调用者必须保证 VM 处于活跃状态且未在另一线程执行。
    pub unsafe fn call_callback(&mut self, callback_id: i64, args: &[i64]) -> i64 {
        let func_idx = match self.callbacks.lookup(callback_id) {
            Some(idx) => idx,
            None => {
                eprintln!("[vm] 回调 #{} 未注册，返回 0", callback_id);
                return 0;
            }
        };

        // 检查参数数量
        let param_count = if func_idx < self.module.funcs.len() {
            self.module.funcs[func_idx].param_count as usize
        } else {
            eprintln!("[vm] 回调 #{} 指向无效函数 #{}", callback_id, func_idx);
            return 0;
        };

        let args = args.iter().take(param_count).cloned().collect::<Vec<_>>();
        let values: Vec<Value> = args.iter().map(|a| Value::Int(*a)).collect();

        // 保存当前帧栈，执行回调，恢复
        let old_frames = std::mem::take(&mut self.frames);
        let old_result = self.result.take();
        let old_halt = self.halt;

        self.push_frame(func_idx, values).ok();
        while !self.frames.is_empty() && !self.halt {
            let _ = self.step();
        }

        let result = self.result.take().unwrap_or(Value::Null);

        // 恢复帧栈
        self.frames = old_frames;
        self.result = old_result;
        self.halt = old_halt;

        result.as_int()
    }

    /// 重置 VM 状态以重复运行（基准测试用）
    pub fn reset_for_reuse(&mut self) {
        self.frames.clear();
        self.call_counts.iter_mut().for_each(|c| *c = 0);
        self.result = None;
        self.halt = false;
        // P10: 重置并发运行时状态
        self.actors = crate::vm::actor::ActorRuntime::new();
        self.channels = crate::vm::channel::ChannelRuntime::new();
    }

    /// 当前调用帧深度
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// 当前存活堆对象数（诊断）
    pub fn live_objects(&self) -> usize {
        self.heap.live_count()
    }

    /// 获取堆的可变引用（供测试 / 调试使用）
    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }

    /// 获取堆的引用（供测试 / 调试使用）
    pub fn heap_ref(&self) -> &Heap {
        &self.heap
    }

    /// 获取当前帧栈的可变引用（供测试 / 调试使用）
    pub fn frames_mut(&mut self) -> &mut Vec<Frame> {
        &mut self.frames
    }

    pub fn frames(&self) -> &Vec<Frame> {
        &self.frames
    }

    // ── 调试器公开接口 ──

    /// 执行是否已停止（`Halt` 或帧栈空）
    pub fn is_halt(&self) -> bool {
        self.halt
    }

    /// 获取入口函数返回值（仅在执行完成后有效）
    pub fn result(&self) -> Option<Value> {
        self.result.clone()
    }

    /// 获取已加载模块的引用（供调试器读取函数表）
    pub fn module_ref(&self) -> &LoadedModule {
        &self.module
    }

    /// 单步执行一条指令（调试器专用，调用方负责检查断点）
    pub fn debug_step(&mut self) -> Result<(), VmError> {
        self.step()
    }

    /// 压入入口调用帧（调试器初始化专用）
    pub fn debug_push_entry(&mut self) -> Result<(), VmError> {
        let entry = self.module.entry as usize;
        if entry >= self.module.funcs.len() {
            return Err(VmError::NoEntry(format!(
                "no entry function (module has {} functions, entry={}, max={})",
                self.module.funcs.len(),
                entry,
                self.module.funcs.len().saturating_sub(1)
            )));
        }
        self.push_frame(entry, Vec::new())
    }

    /// 初始化 VM 分发器（原生函数 + 回调），与 `run()` 中的初始化逻辑一致
    pub fn debug_setup(&mut self) {
        crate::vm::native::set_vm_ref(self as *mut Self as *mut ());
        use crate::vm::ffi::set_dispatcher;
        use std::sync::atomic::AtomicPtr;
        let vm_ptr = std::sync::Arc::new(AtomicPtr::new(self as *mut Vm));
        let vm_ptr_clone = vm_ptr.clone();
        let dispatcher = std::sync::Arc::new(move |callback_id: i64, args: &[i64]| {
            use std::sync::atomic::Ordering;
            let ptr = vm_ptr_clone.load(Ordering::SeqCst);
            if ptr.is_null() {
                return 0;
            }
            unsafe { (*ptr).call_callback(callback_id, args) }
        });
        set_dispatcher(dispatcher);
    }

    /// 清理 VM 分发器（原生函数 + 回调）
    pub fn debug_cleanup(&mut self) {
        crate::vm::ffi::clear_dispatcher();
        crate::vm::native::clear_vm_ref();
    }

    // ── 内部辅助 ──

    fn push_frame(&mut self, func_idx: usize, args: Vec<Value>) -> Result<(), VmError> {
        if self.frames.len() >= self.opts.max_call_depth {
            return Err(VmError::Runtime(format!(
                "call stack overflow (max depth {})",
                self.opts.max_call_depth
            )));
        }
        let current_co =
            if self.frames.is_empty() { 0 } else { self.frames.last().unwrap().coroutine_id };
        let mut frame = Frame::new(&self.module.funcs[func_idx], args, current_co);
        frame.func = func_idx;
        self.frames.push(frame);
        // 热点计数
        self.call_counts[func_idx] += 1;
        // Fix B: push_frame 级热点检测 — 覆盖入口函数和循环调用
        #[cfg(feature = "jit")]
        if self.opts.jit {
            self.maybe_jit_compile(func_idx);
        }
        Ok(())
    }

    fn pop_frame(&mut self, ret: Value) {
        self.frames.pop();
        if self.frames.is_empty() {
            self.result = Some(ret);
            self.halt = true;
        } else {
            let top = self.frames.len() - 1;
            self.frames[top].stack.push(ret);
        }
    }

    /// 热点检测（5.11）：返回某函数的累计调用次数
    pub fn call_count(&self, func_idx: usize) -> u64 {
        self.call_counts.get(func_idx).copied().unwrap_or(0)
    }

    /// 各函数调用计数快照（诊断）
    pub fn call_counts(&self) -> &[u64] {
        &self.call_counts
    }

    /// JIT 状态快照（诊断，仅 jit feature）：返回每个函数是否已编译 / 已跳过
    #[cfg(feature = "jit")]
    pub fn jit_state(&self) -> Vec<(bool, bool)> {
        let n = self.module.funcs.len();
        (0..n)
            .map(|i| {
                let compiled = self.jit.as_ref().map(|j| j.is_compiled(i)).unwrap_or(false);
                let skipped = self.jit.as_ref().map(|j| j.is_skipped(i)).unwrap_or(false);
                (compiled, skipped)
            })
            .collect()
    }

    /// JIT 跳过原因（仅 jit feature）
    #[cfg(feature = "jit")]
    pub fn jit_skip_reason(&self, idx: usize) -> Option<&str> {
        self.jit.as_ref().and_then(|j| j.skip_reason(idx))
    }

    #[cfg(not(feature = "jit"))]
    pub fn jit_skip_reason(&self, _idx: usize) -> Option<&str> {
        None
    }

    /// 热点编译接缝（5.12）：当 `jit` feature 且 `opts.jit` 开启时，
    /// 对累计调用超过 [`VmOptions::hotspot_threshold`] 的函数尝试 Cranelift
    /// 编译并缓存；之后对该函数的 `Call` 直接派发到原生入口（§7.2 方法级 JIT）。
    /// 编译失败（非叶子整数函数等）则记入 skip 集合，永久回退解释器（5.13）。
    ///
    /// **Fix B**：在 `push_frame` 中也调用此方法（计数已更新），
    /// 覆盖入口函数和循环调用等不经过 `do_call` 的路径。
    #[cfg(feature = "jit")]
    fn maybe_jit_compile(&mut self, idx: usize) {
        let already =
            self.jit.as_ref().map(|j| j.is_compiled(idx) || j.is_skipped(idx)).unwrap_or(true);
        if already {
            return;
        }
        let count = self.call_counts.get(idx).copied().unwrap_or(0);
        if count < self.opts.hotspot_threshold {
            return;
        }
        self.try_jit_compile(idx);
    }

    /// 强制 JIT 编译（忽略调用阈值）：入口函数使用此路径，绕过热点计数限制。
    /// 编译失败则记入 skip 集合，后续回退解释器。
    #[cfg(feature = "jit")]
    fn force_jit_compile(&mut self, idx: usize) {
        let already =
            self.jit.as_ref().map(|j| j.is_compiled(idx) || j.is_skipped(idx)).unwrap_or(true);
        if already {
            return;
        }
        self.try_jit_compile(idx);
    }

    /// 尝试 JIT 编译函数 `idx`（共享逻辑）：成功则缓存原生入口，失败则记入 skip。
    #[cfg(feature = "jit")]
    fn try_jit_compile(&mut self, idx: usize) {
        // Ensure dispatch table capacity to keep pointer stable
        if let Some(jit) = self.jit.as_mut() {
            jit.ensure_capacity(self.module.funcs.len());
        }
        // Recursively compile any callee functions so their dispatch entries exist
        let f_clone = self.module.funcs[idx].clone();
        for instr in &f_clone.code {
            if let Instr::Call(ci) = instr {
                let callee_idx = *ci as usize;
                // Skip self-recursive calls to avoid infinite recursion
                if callee_idx != idx {
                    if let Some(jit) = self.jit.as_ref() {
                        if !(jit.is_compiled(callee_idx) || jit.is_skipped(callee_idx)) {
                            self.try_jit_compile(callee_idx);
                        }
                    }
                }
            }
        }
        // Compile the current function
        let consts = self.module.consts.clone();
        let funcs = self.module.funcs.clone();
        let entry = crate::vm::jit::compile_function(idx, &f_clone, &consts, &funcs);
        if let Some(jit) = self.jit.as_mut() {
            match entry {
                Some(e) => jit.insert(idx, e),
                None => jit.skip(idx, "JIT 白名单不匹配（函数包含非可编译指令）"),
            }
        }
    }
}
