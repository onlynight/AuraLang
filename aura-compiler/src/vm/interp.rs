//! Aura VM 解释器执行循环（直接线程码分派）
//!
//! 主循环 `step()` 对栈顶帧逐条执行指令。`Call`/`Return` 切换调用帧；
//! `CallNative`/`CallC` 经原生注册表分发；`NewObject`/`GetField`/`SetField`
//! 经堆管理器操作对象；`IncRef`/`DecRef` 维护 ARC。

use crate::vm::value::Value;
use crate::vm::{Instr, Vm, VmError};

impl Vm {
    /// 执行一条指令（栈顶帧）
    pub(crate) fn step(&mut self) -> Result<(), VmError> {
        let top = self.frames.len() - 1;
        let func = self.frames[top].func;
        let ip = self.frames[top].ip;

        // 跑到函数末尾：入口帧视为 Halt，否则隐式 ReturnUnit
        if ip >= self.module.funcs[func].code.len() {
            if self.frames.len() == 1 {
                self.halt = true;
                return Ok(());
            }
            self.pop_frame(Value::Null);
            return Ok(());
        }

        let instr = self.module.funcs[func].code[ip].clone();
        self.frames[top].ip += 1;
        self.exec_instr(top, func, instr)
    }

    fn exec_instr(&mut self, top: usize, _func: usize, instr: Instr) -> Result<(), VmError> {
        match instr {
            Instr::LoadConst(ci) => {
                let v = const_to_value(&self.module.consts[ci as usize]);
                self.frames[top].stack.push(v);
            }
            Instr::LoadVar(slot) => {
                let v = self.frames[top].locals[slot as usize].clone();
                self.frames[top].stack.push(v);
            }
            Instr::StoreVar(slot) => {
                let v = self.pop(top)?;
                self.frames[top].locals[slot as usize] = v;
            }

            // ── 算术 ──
            Instr::Add => bin_op(self, top, bin_add)?,
            Instr::Sub => bin_op(self, top, bin_sub)?,
            Instr::Mul => bin_op(self, top, bin_mul)?,
            Instr::Div => bin_op(self, top, bin_div)?,
            Instr::Rem => bin_op(self, top, bin_rem)?,
            Instr::Neg => {
                let a = self.pop(top)?;
                self.frames[top].stack.push(unary_neg(a));
            }
            Instr::Not => {
                let a = self.pop(top)?;
                self.frames[top].stack.push(unary_not(a));
            }

            // ── 逻辑 / 位运算 ──
            Instr::And => bin_op(self, top, bin_and)?,
            Instr::Or => bin_op(self, top, bin_or)?,
            Instr::BitAnd => bin_op(self, top, bin_bitand)?,
            Instr::BitOr => bin_op(self, top, bin_bitor)?,
            Instr::BitXor => bin_op(self, top, bin_bitxor)?,
            Instr::Shl => bin_op(self, top, bin_shl)?,
            Instr::Shr => bin_op(self, top, bin_shr)?,

            // ── 比较 ──
            Instr::Eq => bin_op(self, top, |a, b| Value::Bool(a == b))?,
            Instr::Ne => bin_op(self, top, |a, b| Value::Bool(a != b))?,
            Instr::Lt => bin_op(self, top, bin_lt)?,
            Instr::Gt => bin_op(self, top, bin_gt)?,
            Instr::Le => bin_op(self, top, bin_le)?,
            Instr::Ge => bin_op(self, top, bin_ge)?,

            // ── 控制流 ──
            Instr::Jump(target) => {
                self.frames[top].ip = target;
            }
            Instr::JumpIfTrue(target) => {
                let v = self.pop(top)?;
                if v.is_truthy() {
                    self.frames[top].ip = target;
                }
            }
            Instr::JumpIfFalse(target) => {
                let v = self.pop(top)?;
                if !v.is_truthy() {
                    self.frames[top].ip = target;
                }
            }

            // ── 函数调用 ──
            Instr::Call(idx) => self.do_call(top, idx as usize, false)?,
            Instr::CallNative(idx) => self.do_call_native(top, idx as usize)?,
            Instr::Return => {
                let v = self.pop(top)?;
                self.pop_frame(v);
            }
            Instr::ReturnUnit => {
                self.pop_frame(Value::Null);
            }

            // ── 对象 / 数组 ──
            Instr::NewObject(type_tag) => {
                let h = self.heap.alloc_object(type_tag);
                self.frames[top].stack.push(Value::Ref(h));
            }
            Instr::NewArray => {
                let len = self.pop(top)?.as_int().max(0) as usize;
                let h = self.heap.alloc_array(len);
                self.frames[top].stack.push(Value::Ref(h));
            }
            Instr::GetField(field) => {
                let obj = self.pop(top)?;
                let v = match obj {
                    Value::Ref(h) => self.heap.get_field(h, field),
                    _ => Value::Null,
                };
                self.frames[top].stack.push(v);
            }
            Instr::SetField(field) => {
                // 字节码发射顺序：先压值、再压对象（见 emit.rs）
                let obj = self.pop(top)?;
                let val = self.pop(top)?;
                if let Value::Ref(h) = obj {
                    self.heap.set_field(h, field, val);
                }
            }
            Instr::GetIndex => {
                // 字节码发射顺序：先压数组引用、再压索引（见 emit.rs）
                let idx_v = self.pop(top)?;
                let obj = self.pop(top)?;
                let v = match obj {
                    Value::Ref(h) => self.heap.get_index(h, idx_v.as_int().max(0) as usize),
                    _ => Value::Null,
                };
                self.frames[top].stack.push(v);
            }
            Instr::SetIndex => {
                // 字节码发射顺序：先压值、再压数组引用、最后压索引（见 emit.rs）
                let idx_v = self.pop(top)?;
                let obj = self.pop(top)?;
                let val = self.pop(top)?;
                if let Value::Ref(h) = obj {
                    self.heap.set_index(h, idx_v.as_int().max(0) as usize, val);
                }
            }

            // ── 引用计数 ──
            Instr::IncRef => {
                if let Some(Value::Ref(h)) = self.frames[top].stack.last().cloned() {
                    self.heap.inc_ref(h);
                }
            }
            Instr::DecRef => {
                if let Some(Value::Ref(h)) = self.frames[top].stack.last().cloned() {
                    self.heap.dec_ref(h);
                }
            }

            // ── FFI（C ABI）──
            Instr::CallC(idx) => {
                // 当前字节码未单独携带 C 函数表，按原生索引查注册表处理
                self.do_call_native(top, idx as usize)?;
            }

            Instr::Halt => {
                self.halt = true;
            }
        }
        Ok(())
    }

    /// 用户函数调用（含 JIT 原生派发）
    fn do_call(&mut self, top: usize, idx: usize, _from_jit: bool) -> Result<(), VmError> {
        if idx >= self.module.funcs.len() {
            return Err(VmError::Runtime(format!("call to undefined function #{}", idx)));
        }
        let param_count = self.module.funcs[idx].param_count as usize;
        let args = self.pop_n(top, param_count)?;

        // 热点检测（5.11）：累计计数超阈值触发 Cranelift 编译（5.12）；
        // 已编译的函数直接派发到原生入口（§7.2「方法级 JIT」Patch 替换字节码解释），
        // 编译失败/未达阈值的函数回退解释器（5.13）。
        #[cfg(feature = "jit")]
        if self.opts.jit {
            self.maybe_jit_compile(idx);
            let compiled = self.jit.as_ref().map(|j| j.is_compiled(idx)).unwrap_or(false);
            if compiled {
                let jargs: Vec<crate::vm::jit::JitValue> =
                    args.iter().map(crate::vm::jit::JitValue::from_value).collect();
                if let Some(ret) = self.jit.as_ref().and_then(|j| j.call(idx, &jargs)) {
                    self.frames[top].stack.push(ret.to_value());
                    return Ok(());
                }
            }
        }

        self.push_frame(idx, args)?;
        Ok(())
    }

    /// 原生 / FFI 函数调用
    fn do_call_native(&mut self, top: usize, idx: usize) -> Result<(), VmError> {
        if idx >= self.module.natives.len() {
            return Err(VmError::Runtime(format!(
                "call to undefined native #{}",
                idx
            )));
        }
        let native = self.module.natives[idx].clone();
        let param_count = native.param_count as usize;
        let args = self.pop_n(top, param_count)?;

        let result = if let Some(f) = self.natives.get(&native.name) {
            f(&args)
        } else {
            // 未链接的外部函数：占位实现（打印参数并返回 0），保证字节码可继续执行
            eprintln!(
                "[vm] 未链接的外部函数 `{}`，已忽略调用（参数: {:?}）",
                native.name,
                args.iter().map(|v| v.to_string()).collect::<Vec<_>>()
            );
            Value::Int(0)
        };
        self.frames[top].stack.push(result);
        Ok(())
    }

    // ── 栈辅助 ──

    fn pop(&mut self, top: usize) -> Result<Value, VmError> {
        let func_name = self.module.funcs[self.frames[top].func].name.clone();
        let ip = self.frames[top].ip;
        match self.frames[top].stack.pop() {
            Some(v) => Ok(v),
            None => Err(VmError::Runtime(format!(
                "operand stack underflow in `{}` at ip={}",
                func_name, ip
            ))),
        }
    }

    fn pop_n(&mut self, top: usize, n: usize) -> Result<Vec<Value>, VmError> {
        let stack = &mut self.frames[top].stack;
        if stack.len() < n {
            let func_name = self.module.funcs[self.frames[top].func].name.clone();
            let ip = self.frames[top].ip;
            return Err(VmError::Runtime(format!(
                "operand stack underflow (need {} args) in `{}` at ip={}",
                n, func_name, ip
            )));
        }
        let start = stack.len() - n;
        Ok(stack.split_off(start))
    }
}

/// 二元运算：弹出 b、a，计算后压回结果
fn bin_op<F>(vm: &mut Vm, top: usize, f: F) -> Result<(), VmError>
where
    F: FnOnce(Value, Value) -> Value,
{
    let b = vm.pop(top)?;
    let a = vm.pop(top)?;
    vm.frames[top].stack.push(f(a, b));
    Ok(())
}

fn const_to_value(c: &crate::codegen::opcode::Const) -> Value {
    match c {
        crate::codegen::opcode::Const::Int(i) => Value::Int(*i),
        crate::codegen::opcode::Const::Float(f) => Value::Float(*f),
        crate::codegen::opcode::Const::Str(s) => Value::str_(s.clone()),
        crate::codegen::opcode::Const::Bool(b) => Value::Bool(*b),
        crate::codegen::opcode::Const::Null => Value::Null,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 数值 / 逻辑运算（类型感知）
// ─────────────────────────────────────────────────────────────────────────────

fn both_int(a: &Value, b: &Value) -> bool {
    matches!(a, Value::Int(_)) && matches!(b, Value::Int(_))
}

fn bin_add(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        Value::Int(a.as_int().wrapping_add(b.as_int()))
    } else {
        Value::Float(a.as_float() + b.as_float())
    }
}

fn bin_sub(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        Value::Int(a.as_int().wrapping_sub(b.as_int()))
    } else {
        Value::Float(a.as_float() - b.as_float())
    }
}

fn bin_mul(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        Value::Int(a.as_int().wrapping_mul(b.as_int()))
    } else {
        Value::Float(a.as_float() * b.as_float())
    }
}

fn bin_div(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        let (x, y) = (a.as_int(), b.as_int());
        if y == 0 {
            Value::Int(0)
        } else {
            Value::Int(x.wrapping_div(y))
        }
    } else {
        Value::Float(a.as_float() / b.as_float())
    }
}

fn bin_rem(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        let (x, y) = (a.as_int(), b.as_int());
        if y == 0 {
            Value::Int(0)
        } else {
            Value::Int(x.wrapping_rem(y))
        }
    } else {
        Value::Float(a.as_float() % b.as_float())
    }
}

fn unary_neg(a: Value) -> Value {
    match a {
        Value::Int(i) => Value::Int(i.wrapping_neg()),
        Value::Float(f) => Value::Float(-f),
        _ => Value::Int(-a.as_int()),
    }
}

fn unary_not(a: Value) -> Value {
    match a {
        Value::Bool(b) => Value::Bool(!b),
        Value::Int(i) => Value::Int(!i),
        _ => Value::Bool(!a.is_truthy()),
    }
}

fn bin_and(a: Value, b: Value) -> Value {
    if let (Value::Bool(x), Value::Bool(y)) = (&a, &b) {
        return Value::Bool(*x && *y);
    }
    Value::Int(a.as_int() & b.as_int())
}

fn bin_or(a: Value, b: Value) -> Value {
    if let (Value::Bool(x), Value::Bool(y)) = (&a, &b) {
        return Value::Bool(*x || *y);
    }
    Value::Int(a.as_int() | b.as_int())
}

fn bin_bitand(a: Value, b: Value) -> Value {
    Value::Int(a.as_int() & b.as_int())
}
fn bin_bitor(a: Value, b: Value) -> Value {
    Value::Int(a.as_int() | b.as_int())
}
fn bin_bitxor(a: Value, b: Value) -> Value {
    Value::Int(a.as_int() ^ b.as_int())
}
fn bin_shl(a: Value, b: Value) -> Value {
    Value::Int(a.as_int().wrapping_shl(b.as_int() as u32))
}
fn bin_shr(a: Value, b: Value) -> Value {
    Value::Int(a.as_int().wrapping_shr(b.as_int() as u32))
}

fn bin_lt(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        Value::Bool(a.as_int() < b.as_int())
    } else {
        Value::Bool(a.as_float() < b.as_float())
    }
}
fn bin_gt(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        Value::Bool(a.as_int() > b.as_int())
    } else {
        Value::Bool(a.as_float() > b.as_float())
    }
}
fn bin_le(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        Value::Bool(a.as_int() <= b.as_int())
    } else {
        Value::Bool(a.as_float() <= b.as_float())
    }
}
fn bin_ge(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        Value::Bool(a.as_int() >= b.as_int())
    } else {
        Value::Bool(a.as_float() >= b.as_float())
    }
}
