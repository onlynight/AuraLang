//! Bootstrap 最小虚拟机核心。
//!
//! 包含：最小 `Value` 表示、字节码指令集、栈帧管理、指令分发、
//! 异常处理（`Trap` 传播）与 **FFI AOT 直连** 支持（`FfiCache`）。
//!
//! # FFI AOT 直连（VM 模式）
//!
//! 启动阶段通过 [`FfiCache::preload_std`] 一次性加载所有 C 函数地址
//! 并绑定类型化调用器；执行阶段 `CallFfi <slot>` 按槽位**直接调用**，
//! 无按名查找开销（传统方式每次调用都要 `GetModuleHandle/GetProcAddress`
//! 或 `dlsym` 式查找）。
//!
//! JIT 模式复用同一槽位表（[`crate::bootstrap::jit_core`] 编译期把调用点
//! 解析为槽位 + 内联缓存）；AOT 模式据此生成 `call @symbol` 直接调用指令。

use std::collections::HashMap;
use std::rc::Rc;

use super::Trap;
use super::jit_core::JitCompiler;

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

/// Bootstrap 层最小值表示。
///
/// 与编译器主 VM 的 `vm::Value` 解耦：bootstrap 是独立引导层，
/// 未来主 VM 可以把它作为 Layer 0 的最小内核引用。
#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    /// 裸指针（malloc/mmap 等的宿主表示）
    Ptr(usize),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            // Int 与 Float 数值相等
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => {
                (*a as f64) == *b
            }
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Ptr(a), Value::Ptr(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    /// 类型名称（供 type_core / any_core 使用）。
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "Null",
            Value::Bool(_) => "Bool",
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Str(_) => "Str",
            Value::Ptr(_) => "Pointer",
        }
    }

    /// 条件判定真值（Null/Int(0)/Float(0.0)/"" 视为假）。
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Ptr(p) => *p != 0,
        }
    }

    pub fn as_int(&self) -> Result<i64, Trap> {
        match self {
            Value::Int(n) => Ok(*n),
            other => Err(Trap::new(format!(
                "类型错误：期望 Int，实际 {}",
                other.type_name()
            ))),
        }
    }

    pub fn as_float(&self) -> Result<f64, Trap> {
        match self {
            Value::Float(f) => Ok(*f),
            Value::Int(n) => Ok(*n as f64),
            other => Err(Trap::new(format!(
                "类型错误：期望 Float，实际 {}",
                other.type_name()
            ))),
        }
    }

    pub fn as_str(&self) -> Result<&str, Trap> {
        match self {
            Value::Str(s) => Ok(s),
            other => Err(Trap::new(format!(
                "类型错误：期望 Str，实际 {}",
                other.type_name()
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// 字节码
// ---------------------------------------------------------------------------

/// 最小字节码指令集。
///
/// 分支使用**相对偏移**（`Jmp`/`JmpIfFalse`），偏移相对「下一条指令」；
/// JIT 编译时被重解析为绝对目标（见 `jit_core`）。
#[derive(Clone, Debug)]
pub enum Insn {
    /// 压入常量
    Const(Value),
    /// 读取局部变量
    LoadLocal(u16),
    /// 写入局部变量
    StoreLocal(u16),
    /// 二元算术（Int/Float 多态；Str+Str 为拼接）
    Add,
    Sub,
    Mul,
    /// 除法（整数除零产生 Trap）
    Div,
    /// 相等比较（栈顶为右操作数）
    Eq,
    /// 小于比较
    Lt,
    /// 无条件跳转（相对偏移）
    Jmp(i32),
    /// 弹出条件，为假时跳转
    JmpIfFalse(i32),
    /// 调用模块内函数（函数索引）
    Call(u16),
    /// **FFI AOT 直连调用**（FfiCache 槽位，启动时预加载绑定）
    CallFfi(u16),
    /// 返回（栈顶为返回值，空栈返回 Null）
    Ret,
    /// 协程让出（栈顶为让出值）；仅允许在协程入口函数中直接执行
    Yield,
}

/// 函数定义。
#[derive(Clone, Debug)]
pub struct FuncDef {
    pub name: String,
    pub params: usize,
    pub locals: usize,
    pub code: Vec<Insn>,
    /// 可选的显式类型签名（AOT 生成 LLVM IR 时使用；VM/JIT 忽略）。
    /// 参数依次对应 `params`，为 `None` 时 AOT 侧按推断/默认 i64 处理。
    pub sig: Option<FuncSig>,
}

impl FuncDef {
    pub fn new(name: impl Into<String>, params: usize, locals: usize, code: Vec<Insn>) -> Self {
        Self {
            name: name.into(),
            params,
            locals,
            code,
            sig: None,
        }
    }
}

/// 显式函数签名（仅 AOT 需要）。
#[derive(Clone, Debug)]
pub struct FuncSig {
    pub params: Vec<Option<super::aot_core::ValType>>,
    pub ret: super::aot_core::ValType,
}

/// 字节码模块。
#[derive(Clone, Debug, Default)]
pub struct BytecodeModule {
    pub name: String,
    pub funcs: Vec<FuncDef>,
}

impl BytecodeModule {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            funcs: Vec::new(),
        }
    }

    /// 追加函数，返回其索引。
    pub fn add_func(&mut self, def: FuncDef) -> usize {
        self.funcs.push(def);
        self.funcs.len() - 1
    }

    /// 按名查找函数索引。
    pub fn function_index(&self, name: &str) -> Option<usize> {
        self.funcs.iter().position(|f| f.name == name)
    }
}

// ---------------------------------------------------------------------------
// FFI AOT 直连缓存
// ---------------------------------------------------------------------------

/// 类型化 FFI 调用器：预加载阶段绑定的**直接调用**入口。
pub type FfiCallable = Box<dyn Fn(&[Value]) -> Result<Value, Trap>>;

/// C 侧真实类型（生成 LLVM IR `declare` / 调用转换时使用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CType {
    I32,
    I64,
    U64,
    F64,
    Ptr,
}

impl CType {
    /// LLVM IR 类型文本。
    pub fn llvm(self) -> &'static str {
        match self {
            CType::I32 => "i32",
            CType::I64 | CType::U64 => "i64",
            CType::F64 => "double",
            CType::Ptr => "ptr",
        }
    }
}

/// 预加载后的 FFI 函数条目。
pub struct FfiEntry {
    pub name: String,
    /// 预加载的 C 函数地址（VM 直连依据；AOT 侧使用符号名直连）。
    pub addr: usize,
    /// C 侧真实签名
    pub c_params: Vec<CType>,
    pub c_ret: CType,
    /// 参数个数
    pub arity: usize,
    /// 累计调用次数（内联缓存/热点统计）
    pub calls: u64,
    /// 类型化直接调用器（避免每次调用做符号查找）
    pub(crate) call: FfiCallable,
}

/// FFI 函数地址缓存：**启动时预加载，执行期直连**。
///
/// - `register` / `preload_std`：启动阶段一次性完成符号地址解析；
/// - `call` / `call_by_slot`：执行阶段直接派发，无函数指针查找；
/// - 槽位（slot）是 FFI 表的稳定序号，字节码 `CallFfi <slot>` 与
///   JIT 内联缓存均以槽位为键。
pub struct FfiCache {
    entries: Vec<FfiEntry>,
    index: HashMap<String, usize>,
}

impl FfiCache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// 注册（预加载）一个 C 函数：解析一次，此后直连。
    pub fn register(
        &mut self,
        name: &str,
        addr: usize,
        c_params: Vec<CType>,
        c_ret: CType,
        call: FfiCallable,
    ) -> usize {
        let arity = c_params.len();
        assert_eq!(self.index.get(name), None, "FFI 函数重复注册: {name}");
        let slot = self.entries.len();
        self.index.insert(name.to_string(), slot);
        self.entries.push(FfiEntry {
            name: name.to_string(),
            addr,
            c_params,
            c_ret,
            arity,
            calls: 0,
            call,
        });
        slot
    }

    /// 预加载 bootstrap 标准直连集：
    /// - `abs`（libc）：i32 → i32
    /// - `strlen`（libc）：ptr → u64
    /// - `aura_bootstrap_add_i32`（自定义 C ABI 示例）：i32 × i32 → i32
    pub fn preload_std(&mut self) {
        // libc: abs
        self.register(
            "abs",
            abs as *const () as usize,
            vec![CType::I32],
            CType::I32,
            Box::new(|args| {
                let x = args.first().ok_or_else(|| Trap::new("abs: 缺少参数"))?;
                let x =
                    i32::try_from(x.as_int()?).map_err(|_| Trap::new("abs: 参数超出 i32 范围"))?;
                // SAFETY: abs 是 libc 标准函数，无副作用
                Ok(Value::Int(unsafe { abs(x) } as i64))
            }),
        );
        // libc: strlen
        self.register(
            "strlen",
            strlen as *const () as usize,
            vec![CType::Ptr],
            CType::U64,
            Box::new(|args| {
                let s = args.first().ok_or_else(|| Trap::new("strlen: 缺少参数"))?;
                let s = s.as_str()?;
                // SAFETY: Rc<str> 内容连续存储，strlen 只读至终止符；
                // 我们传入的串不含 NUL，因此读取范围恰为字符串内容。
                Ok(Value::Int(unsafe { strlen(s.as_ptr() as *const i8) } as i64))
            }),
        );
        // 自定义 C ABI 库函数
        self.register(
            "aura_bootstrap_add_i32",
            aura_bootstrap_add_i32 as *const () as usize,
            vec![
                CType::I32,
                CType::I32,
            ],
            CType::I32,
            Box::new(|args| {
                if args.len() != 2 {
                    return Err(Trap::new("aura_bootstrap_add_i32: 需要 2 个参数"));
                }
                let a = i32::try_from(args[0].as_int()?)
                    .map_err(|_| Trap::new("aura_bootstrap_add_i32: 参数溢出"))?;
                let b = i32::try_from(args[1].as_int()?)
                    .map_err(|_| Trap::new("aura_bootstrap_add_i32: 参数溢出"))?;
                Ok(Value::Int(aura_bootstrap_add_i32(a, b) as i64))
            }),
        );
    }

    /// 按名查槽位（仅前端绑定阶段使用；执行期不再按名查找）。
    pub fn slot(&self, name: &str) -> Option<usize> {
        self.index.get(name).copied()
    }

    /// 按槽位取条目。
    pub fn entry(&self, slot: usize) -> Option<&FfiEntry> {
        self.entries.get(slot)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 执行期直连调用（按名；等价于先查槽位再 `call_by_slot`，
    /// 槽位表预加载后是 O(1) 索引，无符号解析）。
    pub fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, Trap> {
        let slot = self.slot(name).ok_or_else(|| Trap::new(format!("FFI 函数未预加载: {name}")))?;
        self.call_by_slot(slot, args)
    }

    /// **直连路径**：按预加载槽位直接调用（VM `CallFfi` / JIT 内联缓存共用）。
    pub fn call_by_slot(&mut self, slot: usize, args: &[Value]) -> Result<Value, Trap> {
        let entry =
            self.entries.get_mut(slot).ok_or_else(|| Trap::new(format!("FFI 槽位无效: {slot}")))?;
        entry.calls += 1;
        (entry.call)(args)
    }
}

impl Default for FfiCache {
    fn default() -> Self {
        Self::new()
    }
}

// 预加载目标（Windows: msvcrt / Linux: libc）
unsafe extern "C" {
    fn abs(x: i32) -> i32;
    fn strlen(s: *const i8) -> usize;
}

/// 自定义 C ABI 函数（模拟第三方库，验证自定义库直连）。
extern "C" fn aura_bootstrap_add_i32(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

// ---------------------------------------------------------------------------
// VM：栈帧管理与指令分发
// ---------------------------------------------------------------------------

/// 调用栈帧。
struct Frame {
    func: usize,
    ip: usize,
    locals: Vec<Value>,
    stack: Vec<Value>,
}

impl Frame {
    fn new(func: usize, def: &FuncDef, args: &[Value]) -> Self {
        let mut locals = vec![Value::Null; def.locals];
        for (i, v) in args.iter().enumerate() {
            locals[i] = v.clone();
        }
        Self {
            func,
            ip: 0,
            locals,
            stack: Vec::new(),
        }
    }
}

/// 执行结果：完成或协程让出。
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// 执行完成，携带返回值
    Done(Value),
    /// 协程让出（`coroutine_yield`），携带让出值
    Yielded(Value),
}

/// 最小虚拟机。
///
/// 栈帧管理（每帧独立局部变量槽与操作数栈）、指令分发、异常处理
/// （`Trap` 沿调用链传播）、FFI 直连（`CallFfi` 走预加载槽位），
/// 以及可选 JIT（热点函数经 `jit_core` 编译，去优化回退解释器）。
pub struct Vm<'m> {
    module: &'m BytecodeModule,
    ffi: &'m mut FfiCache,
    frames: Vec<Frame>,
    call_counts: Vec<u64>,
    jit: Option<JitCompiler>,
}

impl<'m> Vm<'m> {
    pub fn new(module: &'m BytecodeModule, ffi: &'m mut FfiCache) -> Self {
        Self {
            module,
            ffi,
            frames: Vec::new(),
            call_counts: vec![0; module.funcs.len()],
            jit: None,
        }
    }

    /// 启用 JIT：调用次数达到 `hot_threshold` 的叶子函数被编译为
    /// JIT 单元并以直接派发执行；失败/不支持时自动去优化回解释器。
    pub fn enable_jit(&mut self, hot_threshold: u64) {
        self.jit = Some(JitCompiler::new(JitCompiler::config_with_threshold(
            hot_threshold,
        )));
    }

    pub fn jit(&self) -> Option<&JitCompiler> {
        self.jit.as_ref()
    }

    pub fn ffi(&self) -> &FfiCache {
        self.ffi
    }

    /// 压入入口帧（不执行）——协程 API 使用。
    pub fn enter(&mut self, func_name: &str, args: &[Value]) -> Result<(), Trap> {
        let fidx = self
            .module
            .function_index(func_name)
            .ok_or_else(|| Trap::new(format!("函数不存在: {func_name}")))?;
        self.enter_index(fidx, args)
    }

    fn enter_index(&mut self, fidx: usize, args: &[Value]) -> Result<(), Trap> {
        let def = &self.module.funcs[fidx];
        if args.len() != def.params {
            return Err(Trap::new(format!(
                "函数 {} 期望 {} 个参数，实际 {} 个",
                def.name,
                def.params,
                args.len()
            )));
        }
        if def.locals < def.params {
            return Err(Trap::new(format!(
                "函数 {} 局部变量槽 ({}) 少于参数个数 ({})",
                def.name, def.locals, def.params
            )));
        }
        self.frames.push(Frame::new(fidx, def, args));
        Ok(())
    }

    /// 调用函数并执行到完成（不支持入口函数 `Yield`；
    /// 协程请使用 [`Vm::start`] / [`Vm::resume`]）。
    pub fn call(&mut self, func_name: &str, args: &[Value]) -> Result<Value, Trap> {
        let fidx = self
            .module
            .function_index(func_name)
            .ok_or_else(|| Trap::new(format!("函数不存在: {func_name}")))?;
        self.call_counts[fidx] += 1;
        // 入口函数同样参与热点 JIT 直接派发
        if let Some(Ok(v)) = self.jit_try_direct(fidx, args) {
            return Ok(v);
        }
        self.enter_index(fidx, args)?;
        match self.run()? {
            Step::Done(v) => Ok(v),
            Step::Yielded(_) => {
                self.frames.pop();
                Err(Trap::new("coroutine_yield 仅允许在协程入口函数中使用"))
            }
        }
    }

    /// 启动协程入口：执行到完成或第一次 `Yield`。
    pub fn start(&mut self, func_name: &str, args: &[Value]) -> Result<Step, Trap> {
        self.enter(func_name, args)?;
        self.run()
    }

    /// 恢复协程：`v` 作为 `Yield` 表达式的值交付给协程。
    pub fn resume(&mut self, v: Value) -> Result<Step, Trap> {
        if self.frames.is_empty() {
            return Err(Trap::new("协程已结束，无法恢复"));
        }
        self.frames.last_mut().unwrap().stack.push(v);
        self.run()
    }

    /// 驱动**已压入入口帧**的执行循环（协程 API 使用；
    /// 常规调用请使用 [`Vm::call`] / [`Vm::start`]）。
    pub fn run_started(&mut self) -> Result<Step, Trap> {
        self.run()
    }

    /// 热点 JIT 直接派发。
    ///
    /// 返回值语义：
    /// - `Some(Ok(v))`：JIT 编译单元执行成功，返回结果；
    /// - `Some(Err(()))`：去优化（deoptimization），需回退解释执行；
    /// - `None`：未达热点阈值或函数不可编译（非叶子），直接解释执行。
    fn jit_try_direct(&mut self, fidx: usize, args: &[Value]) -> Option<Result<Value, ()>> {
        let count = self.call_counts[fidx];
        let hot = self.jit.as_ref().is_some_and(|j| count >= j.config.hot_threshold);
        if !hot {
            return None;
        }
        let compiled = {
            let jit = self.jit.as_mut().unwrap();
            if !jit.is_compiled(fidx) {
                jit.try_compile(&self.module.funcs[fidx], fidx);
            }
            jit.is_compiled(fidx)
        };
        if !compiled {
            return None;
        }
        let unit = self.jit.as_mut().unwrap().unit(fidx).unwrap().clone();
        match super::jit_core::execute(&unit, args, self.ffi) {
            Ok(v) => Some(Ok(v)),
            Err(super::jit_core::Deopt) => {
                self.jit.as_mut().unwrap().deopts += 1;
                Some(Err(()))
            }
        }
    }

    /// 主解释循环（单层派发，无递归）。
    ///
    /// - `base` 是进入本循环时的帧深基准：基准帧 `Ret` 即结束（Done）；
    /// - `Call` 直接压入被调帧并在同一循环内继续执行（返回值由
    ///   `Ret` 处理器交还父帧），或对热点叶子函数走 JIT 直接派发；
    /// - `Yield` 仅允许在基准帧（协程入口）中让出。
    fn run(&mut self) -> Result<Step, Trap> {
        let base = match self.frames.len() {
            0 => return Ok(Step::Done(Value::Null)),
            n => n - 1,
        };
        loop {
            let (fidx, ip) = match self.frames.last() {
                Some(f) => (f.func, f.ip),
                None => return Ok(Step::Done(Value::Null)),
            };
            let def = &self.module.funcs[fidx];
            if ip >= def.code.len() {
                return Err(Trap::new(format!(
                    "程序计数器越界: {} ip={ip} len={}",
                    def.name,
                    def.code.len()
                )));
            }
            let insn = def.code[ip].clone();
            let next_ip = ip + 1;
            self.frames.last_mut().unwrap().ip = next_ip;

            match insn {
                Insn::Const(v) => self.push(v)?,
                Insn::LoadLocal(i) => {
                    let v = self
                        .frames
                        .last()
                        .ok_or_else(|| Trap::new("LoadLocal: 无活动帧"))?
                        .locals
                        .get(i as usize)
                        .cloned()
                        .ok_or_else(|| Trap::new(format!("局部变量槽越界: {i}")))?;
                    self.push(v)?;
                }
                Insn::StoreLocal(i) => {
                    let v = self.pop()?;
                    let frame = self.frames.last_mut().unwrap();
                    let slot = frame
                        .locals
                        .get_mut(i as usize)
                        .ok_or_else(|| Trap::new(format!("局部变量槽越界: {i}")))?;
                    *slot = v;
                }
                Insn::Add => self.binop(BinOp::Add)?,
                Insn::Sub => self.binop(BinOp::Sub)?,
                Insn::Mul => self.binop(BinOp::Mul)?,
                Insn::Div => self.binop(BinOp::Div)?,
                Insn::Eq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(Value::Bool(a == b))?;
                }
                Insn::Lt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(Value::Bool(compare_lt(&a, &b)?))?;
                }
                Insn::Jmp(off) => {
                    self.jump(next_ip, off)?;
                }
                Insn::JmpIfFalse(off) => {
                    let cond = self.pop()?;
                    if !cond.truthy() {
                        self.jump(next_ip, off)?;
                    }
                }
                Insn::Call(callee) => {
                    let callee = callee as usize;
                    let nargs = self.module.funcs[callee].params;
                    let args = self.pop_n(nargs)?;

                    // 热点检测：达到阈值 → JIT 编译 + 直接派发；去优化回解释
                    self.call_counts[callee] += 1;
                    match self.jit_try_direct(callee, &args) {
                        Some(Ok(v)) => {
                            self.push(v)?;
                        }
                        _ => {
                            // 解释执行：压入被调帧，同一循环内继续
                            self.enter_index(callee, &args)?;
                        }
                    }
                }
                Insn::CallFfi(slot) => {
                    let arity = self
                        .ffi
                        .entry(slot as usize)
                        .ok_or_else(|| Trap::new(format!("FFI 槽位无效: {slot}")))?
                        .arity;
                    let args = self.pop_n(arity)?;
                    let v = self.ffi.call_by_slot(slot as usize, &args)?;
                    self.push(v)?;
                }
                Insn::Ret => {
                    let frame = self.frames.pop().unwrap();
                    let v = frame.stack.into_iter().last().unwrap_or(Value::Null);
                    if self.frames.len() == base {
                        return Ok(Step::Done(v));
                    }
                    self.frames.last_mut().unwrap().stack.push(v);
                }
                Insn::Yield => {
                    // 仅基准帧（协程入口）可以让出
                    if self.frames.len() - 1 == base {
                        let v = self.frames.last_mut().unwrap().stack.pop().unwrap_or(Value::Null);
                        return Ok(Step::Yielded(v));
                    }
                    return Err(Trap::new("coroutine_yield 仅允许在协程入口函数中使用"));
                }
            }
        }
    }

    // ---- 栈/跳转辅助 ----

    fn push(&mut self, v: Value) -> Result<(), Trap> {
        self.frames.last_mut().ok_or_else(|| Trap::new("操作数栈空：无活动帧"))?.stack.push(v);
        Ok(())
    }

    fn pop(&mut self) -> Result<Value, Trap> {
        self.frames
            .last_mut()
            .ok_or_else(|| Trap::new("操作数栈空：无活动帧"))?
            .stack
            .pop()
            .ok_or_else(|| Trap::new("操作数栈下溢"))
    }

    fn pop_n(&mut self, n: usize) -> Result<Vec<Value>, Trap> {
        let frame = self.frames.last_mut().ok_or_else(|| Trap::new("操作数栈空：无活动帧"))?;
        if frame.stack.len() < n {
            return Err(Trap::new("操作数栈下溢"));
        }
        Ok(frame.stack.split_off(frame.stack.len() - n))
    }

    fn jump(&mut self, next_ip: usize, off: i32) -> Result<(), Trap> {
        let target = next_ip as i64 + off as i64;
        if target < 0 {
            return Err(Trap::new(format!("非法跳转目标: {target}")));
        }
        self.frames.last_mut().unwrap().ip = target as usize;
        Ok(())
    }

    fn binop(&mut self, op: BinOp) -> Result<(), Trap> {
        let b = self.pop()?;
        let a = self.pop()?;
        let v = match (&a, &b) {
            (Value::Int(x), Value::Int(y)) => {
                let (x, y) = (*x, *y);
                Value::Int(match op {
                    BinOp::Add => x.wrapping_add(y),
                    BinOp::Sub => x.wrapping_sub(y),
                    BinOp::Mul => x.wrapping_mul(y),
                    BinOp::Div => {
                        if y == 0 {
                            return Err(Trap::new("整数除零"));
                        }
                        x.wrapping_div(y)
                    }
                })
            }
            (Value::Str(x), Value::Str(y)) if matches!(op, BinOp::Add) => {
                let mut s = String::with_capacity(x.len() + y.len());
                s.push_str(x);
                s.push_str(y);
                Value::Str(Rc::from(s.as_str()))
            }
            _ => {
                let x = a.as_float()?;
                let y = b.as_float()?;
                Value::Float(match op {
                    BinOp::Add => x + y,
                    BinOp::Sub => x - y,
                    BinOp::Mul => x * y,
                    BinOp::Div => x / y,
                })
            }
        };
        self.push(v)
    }
}

enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

fn compare_lt(a: &Value, b: &Value) -> Result<bool, Trap> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x < y),
        _ => {
            let x = a.as_float()?;
            let y = b.as_float()?;
            Ok(x < y)
        }
    }
}
