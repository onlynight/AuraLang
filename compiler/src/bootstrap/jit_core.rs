//! Bootstrap JIT 编译器核心（最小实现）。
//!
//! 职责（对应方案 Phase 1）：
//! - **热点检测**：按函数调用计数（`hot_threshold`）触发编译；
//! - **基线编译**：把字节码函数预解码为 `JitUnit`——分支重解析为
//!   绝对目标、FFI 调用点解析为预加载槽位（**内联缓存**），
//!   执行时免指令解码、免符号查找，直接派发；
//! - **去优化（deoptimization）**：编译单元遇到不支持结构或运行期
//!   异常前置条件（如除零）时返回 [`Deopt`]，VM 回退到解释器执行，
//!   保证语义一致。
//!
//! 机器码后端（Cranelift）位于编译器主 VM（`vm::jit`），bootstrap 层
//! 保持零外部依赖；`JitUnit` 的预解码形态即未来传给机器码后端的输入。

use std::collections::HashMap;

use super::vm_core::{FfiCache, FuncDef, Insn, Value};

/// JIT 配置。
#[derive(Clone, Copy, Debug)]
pub struct JitConfig {
    /// 热点阈值：函数调用次数达到该值即触发编译。
    pub hot_threshold: u64,
}

impl Default for JitConfig {
    fn default() -> Self {
        Self {
            hot_threshold: 1000,
        }
    }
}

/// 去优化信号：JIT 单元无法继续执行，请求回退解释器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deopt;

/// 编译后的指令（分支已解析为绝对目标；FFI 调用点已绑定内联缓存槽位）。
#[derive(Clone, Debug)]
pub enum CInsn {
    Const(Value),
    LoadLocal(u16),
    StoreLocal(u16),
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Lt,
    Br(usize),
    BrIfFalse(usize),
    /// FFI 直连调用：槽位即内联缓存条目（预加载时已绑定地址与调用器）
    CallFfi(usize),
    Ret,
}

/// JIT 编译单元（单个函数的预解码形态）。
#[derive(Clone, Debug)]
pub struct JitUnit {
    pub func: usize,
    pub name: String,
    pub params: usize,
    pub locals: usize,
    pub code: Vec<CInsn>,
    /// 内联缓存：本单元内 FFI 调用点绑定的槽位集合（去重）。
    pub ffi_slots: Vec<usize>,
}

/// JIT 编译器：热点检测 + 基线编译 + 去优化统计。
pub struct JitCompiler {
    pub config: JitConfig,
    units: HashMap<usize, JitUnit>,
    /// 编译次数（统计）
    pub compiles: u64,
    /// 去优化次数（统计）
    pub deopts: u64,
}

impl JitCompiler {
    pub fn new(config: JitConfig) -> Self {
        Self {
            config,
            units: HashMap::new(),
            compiles: 0,
            deopts: 0,
        }
    }

    pub fn config_with_threshold(hot_threshold: u64) -> JitConfig {
        JitConfig {
            hot_threshold,
        }
    }

    pub fn is_compiled(&self, func: usize) -> bool {
        self.units.contains_key(&func)
    }

    pub fn unit(&self, func: usize) -> Option<&JitUnit> {
        self.units.get(&func)
    }

    /// 尝试编译函数：仅叶子函数（不含 `Call`/`Yield`）可编译；
    /// 非叶子函数返回 `false`，调用时走解释器（语义回退）。
    pub fn try_compile(&mut self, def: &FuncDef, func: usize) -> bool {
        if self.units.contains_key(&func) {
            return true;
        }
        let unit = match Self::compile(def, func) {
            Some(u) => u,
            None => return false,
        };
        self.units.insert(func, unit);
        self.compiles += 1;
        true
    }

    /// 使编译单元失效（语义变化时使用）。
    pub fn invalidate(&mut self, func: usize) {
        self.units.remove(&func);
    }

    /// 基线编译：解码 + 分支重解析 + FFI 内联缓存绑定。
    /// 遇到不支持的结构（`Call`/`Yield`）返回 `None`。
    fn compile(def: &FuncDef, func: usize) -> Option<JitUnit> {
        let mut code = Vec::with_capacity(def.code.len());
        let mut ffi_slots = Vec::new();

        for (ip, insn) in def.code.iter().enumerate() {
            let c = match insn {
                Insn::Const(v) => CInsn::Const(v.clone()),
                Insn::LoadLocal(i) => CInsn::LoadLocal(*i),
                Insn::StoreLocal(i) => CInsn::StoreLocal(*i),
                Insn::Add => CInsn::Add,
                Insn::Sub => CInsn::Sub,
                Insn::Mul => CInsn::Mul,
                Insn::Div => CInsn::Div,
                Insn::Eq => CInsn::Eq,
                Insn::Lt => CInsn::Lt,
                Insn::Jmp(off) => CInsn::Br(resolve_branch(ip, *off)?),
                Insn::JmpIfFalse(off) => CInsn::BrIfFalse(resolve_branch(ip, *off)?),
                Insn::Call(_) | Insn::Yield => return None, // 不可编译：触发去优化回退
                Insn::CallFfi(slot) => {
                    let slot = *slot as usize;
                    if !ffi_slots.contains(&slot) {
                        ffi_slots.push(slot);
                    }
                    CInsn::CallFfi(slot)
                }
                Insn::Ret => CInsn::Ret,
            };
            code.push(c);
        }

        Some(JitUnit {
            func,
            name: def.name.clone(),
            params: def.params,
            locals: def.locals,
            code,
            ffi_slots,
        })
    }
}

/// 相对偏移 → 绝对目标（相对「下一条指令」）。
fn resolve_branch(ip: usize, off: i32) -> Option<usize> {
    let next = ip as i64 + 1;
    let target = next + off as i64;
    usize::try_from(target).ok()
}

/// 执行 JIT 单元（直接派发，无指令解码、无符号查找）。
///
/// 任何运行期异常前置条件（栈下溢、除零、FFI 槽位失效）返回
/// [`Deopt`]，由 VM 回退解释器并产生精确的 [`Trap`] 错误信息。
pub fn execute(unit: &JitUnit, args: &[Value], ffi: &mut FfiCache) -> Result<Value, Deopt> {
    if args.len() != unit.params {
        return Err(Deopt);
    }
    let mut locals: Vec<Value> = vec![Value::Null; unit.locals];
    for (i, v) in args.iter().enumerate() {
        locals[i] = v.clone();
    }
    let mut stack: Vec<Value> = Vec::with_capacity(8);
    let mut ip = 0usize;

    macro_rules! pop {
        () => {
            stack.pop().ok_or(Deopt)?
        };
    }

    loop {
        let insn = unit.code.get(ip).ok_or(Deopt)?;
        ip += 1;
        match insn {
            CInsn::Const(v) => stack.push(v.clone()),
            CInsn::LoadLocal(i) => {
                let v = locals.get(*i as usize).ok_or(Deopt)?.clone();
                stack.push(v);
            }
            CInsn::StoreLocal(i) => {
                let v = pop!();
                let slot = locals.get_mut(*i as usize).ok_or(Deopt)?;
                *slot = v;
            }
            CInsn::Add | CInsn::Sub | CInsn::Mul | CInsn::Div => {
                let b = pop!();
                let a = pop!();
                let v = arith(&a, &b, insn)?;
                stack.push(v);
            }
            CInsn::Eq => {
                let b = pop!();
                let a = pop!();
                stack.push(Value::Bool(a == b));
            }
            CInsn::Lt => {
                let b = pop!();
                let a = pop!();
                let x = a.as_float().map_err(|_| Deopt)?;
                let y = b.as_float().map_err(|_| Deopt)?;
                stack.push(Value::Bool(x < y));
            }
            CInsn::Br(t) => ip = *t,
            CInsn::BrIfFalse(t) => {
                let c = pop!();
                if !c.truthy() {
                    ip = *t;
                }
            }
            CInsn::CallFfi(slot) => {
                // 内联缓存直连：槽位 → 预加载条目 → 直接调用
                let arity = ffi.entry(*slot).ok_or(Deopt)?.arity;
                if stack.len() < arity {
                    return Err(Deopt);
                }
                let args = stack.split_off(stack.len() - arity);
                let v = ffi.call_by_slot(*slot, &args).map_err(|_| Deopt)?;
                stack.push(v);
            }
            CInsn::Ret => {
                return Ok(stack.pop().unwrap_or(Value::Null));
            }
        }
    }
}

fn arith(a: &Value, b: &Value, op: &CInsn) -> Result<Value, Deopt> {
    let tag = match op {
        CInsn::Add => "add",
        CInsn::Sub => "sub",
        CInsn::Mul => "mul",
        CInsn::Div => "div",
        _ => unreachable!(),
    };
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(match op {
            CInsn::Add => x.wrapping_add(*y),
            CInsn::Sub => x.wrapping_sub(*y),
            CInsn::Mul => x.wrapping_mul(*y),
            CInsn::Div => {
                if *y == 0 {
                    return Err(Deopt); // 除零 → 去优化，由解释器产生精确 Trap
                }
                x.wrapping_div(*y)
            }
            _ => unreachable!(),
        })),
        (Value::Str(x), Value::Str(y)) if matches!(op, CInsn::Add) => {
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(x);
            s.push_str(y);
            Ok(Value::Str(std::rc::Rc::from(s.as_str())))
        }
        _ => {
            let x = a.as_float().map_err(|_| Deopt)?;
            let y = b.as_float().map_err(|_| Deopt)?;
            let _ = tag;
            Ok(Value::Float(match op {
                CInsn::Add => x + y,
                CInsn::Sub => x - y,
                CInsn::Mul => x * y,
                CInsn::Div => x / y,
                _ => unreachable!(),
            }))
        }
    }
}
