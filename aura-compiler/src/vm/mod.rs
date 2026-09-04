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

pub mod native;
pub mod heap;
pub mod value;
pub mod interp;
pub mod coroutine;
pub mod dynamic_ffi;
#[cfg(feature = "jit")]
pub mod jit;

pub use heap::Heap;
pub use native::NativeRegistry;
pub use value::Value;
pub use coroutine::{CoroutineScheduler, CoroutineState};
pub use dynamic_ffi::DynamicLoader;

use std::collections::HashMap;

use crate::codegen::opcode::{BytecodeFunction, BytecodeModule, BytecodeNative, Const};

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
    NoEntry,
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::Load(m) => write!(f, "load error: {}", m),
            VmError::Runtime(m) => write!(f, "runtime error: {}", m),
            VmError::NoEntry => write!(f, "no entry function (main)"),
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

    Halt,
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
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::LoadConst(v));
            }
            crate::codegen::opcode::OpCode::LoadVar(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::LoadVar(v));
            }
            crate::codegen::opcode::OpCode::StoreVar(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
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
                let off = i32::from_le_bytes([code[ip], code[ip + 1], code[ip + 2], code[ip + 3]]);
                ip += 4;
                // 暂存字节偏移，稍后解析为指令索引
                instrs.push(Instr::Jump(off as usize));
            }
            crate::codegen::opcode::OpCode::JumpIfTrue(_) => {
                let off = i32::from_le_bytes([code[ip], code[ip + 1], code[ip + 2], code[ip + 3]]);
                ip += 4;
                instrs.push(Instr::JumpIfTrue(off as usize));
            }
            crate::codegen::opcode::OpCode::JumpIfFalse(_) => {
                let off = i32::from_le_bytes([code[ip], code[ip + 1], code[ip + 2], code[ip + 3]]);
                ip += 4;
                instrs.push(Instr::JumpIfFalse(off as usize));
            }
            crate::codegen::opcode::OpCode::Call(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::Call(v));
            }
            crate::codegen::opcode::OpCode::CallNative(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::CallNative(v));
            }
            crate::codegen::opcode::OpCode::Return => instrs.push(Instr::Return),
            crate::codegen::opcode::OpCode::ReturnUnit => instrs.push(Instr::ReturnUnit),
            crate::codegen::opcode::OpCode::NewObject(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::NewObject(v));
            }
            crate::codegen::opcode::OpCode::NewArray => instrs.push(Instr::NewArray),
            crate::codegen::opcode::OpCode::GetField(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::GetField(v));
            }
            crate::codegen::opcode::OpCode::SetField(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::SetField(v));
            }
            crate::codegen::opcode::OpCode::GetIndex => instrs.push(Instr::GetIndex),
            crate::codegen::opcode::OpCode::SetIndex => instrs.push(Instr::SetIndex),
            crate::codegen::opcode::OpCode::IncRef => instrs.push(Instr::IncRef),
            crate::codegen::opcode::OpCode::DecRef => instrs.push(Instr::DecRef),
            crate::codegen::opcode::OpCode::CallC(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::CallC(v));
            }
            crate::codegen::opcode::OpCode::CallMethod(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::CallMethod(v));
            }
            crate::codegen::opcode::OpCode::CallCtor(_) => {
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::CallCtor(v));
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
                let v = u16::from_le_bytes([code[ip], code[ip + 1]]);
                ip += 2;
                instrs.push(Instr::NewCoroutine(v));
            }
            crate::codegen::opcode::OpCode::ResumeCoroutine => instrs.push(Instr::ResumeCoroutine),
            crate::codegen::opcode::OpCode::DropRef => instrs.push(Instr::DropRef),
            crate::codegen::opcode::OpCode::Halt => instrs.push(Instr::Halt),
        }
    }

    // 将跳转的「字节偏移」解析为「指令索引」
    let offset_to_idx: HashMap<usize, usize> = starts.iter().enumerate().map(|(i, s)| (*s, i)).collect();
    for instr in instrs.iter_mut() {
        match instr {
            Instr::Jump(off) | Instr::JumpIfTrue(off) | Instr::JumpIfFalse(off) => {
                let idx = *offset_to_idx
                    .get(off)
                    .ok_or_else(|| VmError::Load(format!("jump target {} not at instruction boundary", off)))?;
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
    halt: bool,
    opts: VmOptions,
    #[cfg(feature = "jit")]
    jit: Option<crate::vm::jit::JitState>,
    /// 协程调度器（5.8）：协程 ID → 协程状态
    pub coroutines: CoroutineScheduler,
}

impl Vm {
    /// 从字节码模块创建 VM
    pub fn new(module: &BytecodeModule, opts: VmOptions) -> Result<Self, VmError> {
        let loaded = LoadedModule::from_module(module)?;
        Ok(Vm {
            module: loaded,
            natives: NativeRegistry::new(),
            heap: Heap::new(),
            frames: Vec::new(),
            call_counts: vec![0; module.functions.len()],
            result: None,
            halt: false,
            opts,
            #[cfg(feature = "jit")]
            jit: if cfg!(feature = "jit") {
                Some(crate::vm::jit::JitState::new())
            } else {
                None
            },
            coroutines: CoroutineScheduler::new(),
        })
    }

    /// 注册额外原生函数（供 FFI / 标准库扩展）
    pub fn register_native(&mut self, name: &str, f: native::NativeFn) {
        self.natives.register(name, f);
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
            return Err(VmError::NoEntry);
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
        Ok(self.result.take().unwrap_or(Value::Null))
    }

    /// 重置 VM 状态以重复运行（基准测试用）
    pub fn reset_for_reuse(&mut self) {
        self.frames.clear();
        self.call_counts.iter_mut().for_each(|c| *c = 0);
        self.result = None;
        self.halt = false;
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

    // ── 内部辅助 ──

    fn push_frame(&mut self, func_idx: usize, args: Vec<Value>) -> Result<(), VmError> {
        if self.frames.len() >= self.opts.max_call_depth {
            return Err(VmError::Runtime(format!(
                "call stack overflow (max depth {})",
                self.opts.max_call_depth
            )));
        }
        let current_co = if self.frames.is_empty() {
            0
        } else {
            self.frames.last().unwrap().coroutine_id
        };
        let mut frame = Frame::new(&self.module.funcs[func_idx], args, current_co);
        frame.func = func_idx;
        self.frames.push(frame);
        // 热点计数
        self.call_counts[func_idx] += 1;
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
                let compiled = self
                    .jit
                    .as_ref()
                    .map(|j| j.is_compiled(i))
                    .unwrap_or(false);
                let skipped = self
                    .jit
                    .as_ref()
                    .map(|j| j.is_skipped(i))
                    .unwrap_or(false);
                (compiled, skipped)
            })
            .collect()
    }

    /// 热点编译接缝（5.12）：当 `jit` feature 且 `opts.jit` 开启时，
    /// 对累计调用超过 [`VmOptions::hotspot_threshold`] 的函数尝试 Cranelift
    /// 编译并缓存；之后对该函数的 `Call` 直接派发到原生入口（§7.2 方法级 JIT）。
    /// 编译失败（非叶子整数函数等）则记入 skip 集合，永久回退解释器（5.13）。
    #[cfg(feature = "jit")]
    fn maybe_jit_compile(&mut self, idx: usize) {
        let already = self
            .jit
            .as_ref()
            .map(|j| j.is_compiled(idx) || j.is_skipped(idx))
            .unwrap_or(true);
        if already {
            return;
        }
        // 热点阈值：`push_frame` 中的计数在本次调用后才更新，
        // 故以 `已调用次数 + 1`（本次调用）与阈值比较
        let count = self.call_counts.get(idx).copied().unwrap_or(0);
        if count + 1 < self.opts.hotspot_threshold {
            return;
        }
        self.try_jit_compile(idx);
    }

    /// 强制 JIT 编译（忽略调用阈值）：入口函数使用此路径，绕过热点计数限制。
    /// 编译失败则记入 skip 集合，后续回退解释器。
    #[cfg(feature = "jit")]
    fn force_jit_compile(&mut self, idx: usize) {
        let already = self
            .jit
            .as_ref()
            .map(|j| j.is_compiled(idx) || j.is_skipped(idx))
            .unwrap_or(true);
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
                None => jit.skip(idx),
            }
        }
    }
}
