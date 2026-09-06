//! Aura VM 解释器执行循环（直接线程码分派）
//!
//! 主循环 `step()` 对栈顶帧逐条执行指令。`Call`/`Return` 切换调用帧；
//! `CallNative`/`CallC` 经原生注册表分发；`NewObject`/`GetField`/`SetField`
//! 经堆管理器操作对象；`IncRef`/`DecRef` 维护 ARC。

use crate::vm::value::Value;
use crate::vm::{Instr, Vm, VmError};

// P9: FFI 动态库加载
#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
unsafe extern "system" {
    fn LoadLibraryW(libname: *const u16) -> usize;
    fn GetProcAddress(hmodule: usize, procname: *const std::os::raw::c_char) -> usize;
}

#[cfg(unix)]
unsafe extern "C" {
    fn dlopen(filename: *const std::os::raw::c_char, flags: i32) -> *mut std::os::raw::c_void;
    fn dlsym(
        handle: *mut std::os::raw::c_void,
        symbol: *const std::os::raw::c_char,
    ) -> *mut std::os::raw::c_void;
}

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

            // ── 方法 / 接口调用（5.6） ──
            Instr::CallMethod(method_idx) => self.do_call_method(top, method_idx as usize)?,
            Instr::CallCtor(idx) => {
                // 构造器调用语义与普通 Call 一致：从栈顶弹出参数、调用指定函数、结果入栈
                self.do_call(top, idx as usize, false)?;
            }

            // ── 集合类型（5.7） ──
            Instr::NewList => {
                let cap = self.pop(top)?.as_int().max(0) as usize;
                let h = self.heap.alloc_list(cap);
                self.frames[top].stack.push(Value::Ref(h));
            }
            Instr::NewMap => {
                let h = self.heap.alloc_map();
                self.frames[top].stack.push(Value::Ref(h));
            }
            Instr::ListPush => {
                let obj = self.pop(top)?;
                let val = self.pop(top)?;
                if let Value::Ref(h) = obj {
                    self.heap.list_push(h, val);
                }
                self.frames[top].stack.push(Value::Null);
            }
            Instr::ListPop => {
                let obj = self.pop(top)?;
                let v = match obj {
                    Value::Ref(h) => self.heap.list_pop(h),
                    _ => Value::Null,
                };
                self.frames[top].stack.push(v);
            }
            Instr::ListLen => {
                let obj = self.pop(top)?;
                let len = match obj {
                    Value::Ref(h) => self.heap.list_len(h),
                    _ => 0,
                };
                self.frames[top].stack.push(Value::Int(len));
            }
            Instr::MapSet => {
                // 栈：值、键、Map 引用
                let obj = self.pop(top)?;
                let key = self.pop(top)?;
                let val = self.pop(top)?;
                if let Value::Ref(h) = obj {
                    self.heap.map_set(h, key, val);
                }
            }
            Instr::MapGet => {
                // 栈：键、Map 引用
                let obj = self.pop(top)?;
                let key = self.pop(top)?;
                let v = match obj {
                    Value::Ref(h) => self.heap.map_get(h, &key),
                    _ => Value::Null,
                };
                self.frames[top].stack.push(v);
            }
            Instr::MapLen => {
                let obj = self.pop(top)?;
                let len = match obj {
                    Value::Ref(h) => self.heap.map_len(h),
                    _ => 0,
                };
                self.frames[top].stack.push(Value::Int(len));
            }

            // ── 协程（5.8） ──
            Instr::Yield => self.do_yield(top)?,
            Instr::NewCoroutine(entry_idx) => {
                let co_id = self.coroutines.spawn(entry_idx as usize);
                self.frames[top].stack.push(Value::Int(co_id as i64));
            }
            Instr::ResumeCoroutine => self.do_resume(top)?,

            // ── ARC 生命周期（5.10） ──
            Instr::DropRef => {
                if let Some(Value::Ref(h)) = self.frames[top].stack.last().cloned() {
                    self.heap.drop_ref(h);
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

            // ── P7 内存管理 ──
            Instr::Retain => {
                if let Some(Value::Ref(h)) = self.frames[top].stack.last().cloned() {
                    self.heap.inc_ref(h);
                }
            }
            Instr::Release => {
                if let Some(Value::Ref(h)) = self.frames[top].stack.last().cloned() {
                    self.heap.dec_ref(h);
                }
            }
            Instr::WeakRef => {
                let v = self.pop(top)?;
                match v {
                    Value::Ref(h) => {
                        // 弱引用：创建一个 Weak 值，不增加引用计数
                        self.frames[top].stack.push(Value::Weak(h));
                    }
                    _ => {
                        self.frames[top].stack.push(Value::Null);
                    }
                }
            }
            Instr::WeakGet => {
                let v = self.pop(top)?;
                match v {
                    Value::Weak(h) => {
                        // 尝试升级：若对象仍存活则转为强引用
                        let alive = self.heap.is_alive(h);
                        if alive {
                            self.heap.inc_ref(h);
                            self.frames[top].stack.push(Value::Ref(h));
                        } else {
                            self.frames[top].stack.push(Value::Null);
                        }
                    }
                    _ => {
                        self.frames[top].stack.push(Value::Null);
                    }
                }
            }
            Instr::BoxAlloc => {
                let v = self.pop(top)?;
                // 将值分配到堆上：创建堆对象并存储值
                let h = self.heap.alloc_box_value(v);
                self.frames[top].stack.push(Value::Ref(h));
            }
            Instr::DeferBegin => {
                // 标记 defer 区域开始：VM 端无需特殊处理，编译期已通过 DeferEnd 注入清理代码
            }
            Instr::DeferEnd => {
                // 标记 defer 区域结束
            }

            // ── FFI（C ABI）──
            Instr::CallC(idx) => {
                // 当前字节码未单独携带 C 函数表，按原生索引查注册表处理
                self.do_call_native(top, idx as usize)?;
            }

            Instr::Halt => {
                self.halt = true;
            }

            // ── FFI（P8）──
            Instr::CString => {
                let s = self.pop(top)?;
                let cs = s.as_string();
                // 分配 C 字符串到堆
                let h = self.heap.alloc_c_string(cs);
                self.frames[top].stack.push(Value::Ptr(h as i64));
            }
            Instr::ReadCStr => {
                let v = self.pop(top)?;
                let ptr = v.as_ptr();
                if ptr == 0 {
                    self.frames[top].stack.push(Value::str_(""));
                } else {
                    let s = self.heap.read_c_string(ptr as usize);
                    self.frames[top].stack.push(Value::str_(s));
                }
            }
            Instr::PtrIsNull => {
                let v = self.pop(top)?;
                self.frames[top].stack.push(Value::Bool(v.is_null_ptr()));
            }
            Instr::PtrToInt => {
                let v = self.pop(top)?;
                self.frames[top].stack.push(Value::Int(v.as_ptr()));
            }
            Instr::IntToPtr => {
                let v = self.pop(top)?;
                self.frames[top].stack.push(Value::Ptr(v.as_int()));
            }
            Instr::MakeCallback(func_idx) => {
                let cb_id = self.callbacks.register(func_idx as usize);
                self.frames[top].stack.push(Value::Ptr(cb_id as i64));
            }
            // Phase 2: 闭包
            Instr::MakeClosure(closure_idx) => {
                // 先克隆闭包信息，避免借用冲突
                let closure_info =
                    self.module.module.closures.get(closure_idx as usize).ok_or_else(|| {
                        VmError::Runtime(format!("MakeClosure: 闭包 {} 不存在", closure_idx))
                    })?;
                let closure_name = closure_info.name.clone();
                let param_count = closure_info.param_count;
                let locals = closure_info.locals;
                let capture_count = closure_info.capture_count as usize;
                let func_idx = closure_info.func_idx as usize;
                // 弹出捕获值（按序）
                let mut captures = Vec::with_capacity(capture_count);
                for _ in 0..capture_count {
                    captures.push(self.pop(top)?);
                }
                // 创建闭包对象
                let heap_data = crate::vm::heap::HeapData::Closure {
                    func_name: closure_name,
                    param_count,
                    locals,
                    captures,
                    func_idx,
                };
                let ref_id = self.heap.alloc(heap_data);
                self.frames[top].stack.push(Value::Ref(ref_id));
            }
            Instr::CallClosure => {
                // 栈：参数...、闭包引用（栈顶）
                // 弹出闭包引用
                let closure_val = self.frames[top]
                    .stack
                    .last()
                    .cloned()
                    .ok_or_else(|| VmError::Runtime("CallClosure: 空栈".to_string()))?;
                if let Value::Ref(ref_id) = closure_val {
                    let heap_data = self.heap.get_data_mut(ref_id);
                    if let Some(crate::vm::heap::HeapData::Closure {
                        captures,
                        func_idx,
                        ..
                    }) = heap_data
                    {
                        let captures = captures.clone();
                        let func_idx = *func_idx;
                        // 弹出闭包引用
                        self.frames[top].stack.pop();
                        // 获取闭包函数的实际参数数量（捕获 + 用户参数）
                        let total_param_count = self.module.funcs[func_idx].param_count as usize;
                        // 弹出用户参数（总数 - 捕获数）
                        let user_param_count = total_param_count - captures.len();
                        let mut args = Vec::with_capacity(user_param_count);
                        for _ in 0..user_param_count {
                            args.push(self.pop(top)?);
                        }
                        args.reverse();
                        // 将捕获值作为参数前置
                        let mut all_args = captures;
                        all_args.extend(args);
                        // 将参数压回栈（反转，pop_n 会反转回来）
                        for arg in all_args.iter().rev() {
                            self.frames[top].stack.push(arg.clone());
                        }
                        // 调用函数
                        self.do_call(top, func_idx, false)?;
                    }
                }
            }
            // Phase 3: 枚举
            Instr::EnumConstruct(variant_idx) => {
                // 将枚举值作为 Int 压栈（变体索引）
                self.frames[top].stack.push(Value::Int(variant_idx as i64));
            }
            Instr::EnumTag => {
                // 弹出栈顶值，提取变体索引（如果是 Enum 类型）
                let val = self.pop(top)?;
                let tag = match val {
                    Value::Int(i) => i as u16,
                    _ => 0,
                };
                self.frames[top].stack.push(Value::Int(tag as i64));
            }
            // Phase 3: 函数引用
            Instr::MakeFnRef(func_idx) => {
                // 创建函数引用对象
                let heap_data = crate::vm::heap::HeapData::FnRef(func_idx as usize);
                let ref_id = self.heap.alloc(heap_data);
                self.frames[top].stack.push(Value::Ref(ref_id));
            }
            // Phase 2: 跨模块调用
            Instr::CallExport(sym_idx) => {
                // 从导出符号表查找函数索引
                let export = self.module.module.exports.get(sym_idx as usize).ok_or_else(|| {
                    VmError::Runtime(format!("CallExport: 导出符号 {} 不存在", sym_idx))
                })?;
                let func_idx = export.func_idx.ok_or_else(|| {
                    VmError::Runtime(format!("CallExport: 导出符号 {} 没有函数索引", export.name))
                })?;
                self.do_call(top, func_idx as usize, false)?;
            }
            Instr::CallExternal(mod_idx, sym_idx) => {
                // 从导入表查找外部模块
                let import = self.module.module.imports.get(mod_idx as usize).ok_or_else(|| {
                    VmError::Runtime(format!("CallExternal: 导入模块 {} 不存在", mod_idx))
                })?;
                // 在注册表中查找目标模块
                let target =
                    self.registry.find_export(&import.module, &import.symbol).ok_or_else(|| {
                        VmError::Runtime(format!(
                            "CallExternal: 未加载模块 {} 或符号 {} 不存在",
                            import.module, import.symbol
                        ))
                    })?;
                // 从目标模块的导出索引获取函数索引
                let (_, func_idx) = target.export_index.get(&import.symbol).ok_or_else(|| {
                    VmError::Runtime(format!("CallExternal: 符号 {} 没有函数索引", import.symbol))
                })?;
                self.do_call(top, *func_idx as usize, false)?;
            }
            // Phase 1: AOT 嵌入调用 —— 查 AotRuntime dispatch_table 后直接 call 机器码
            Instr::CallAot(idx) => self.do_call_aot(top, idx as usize)?,
        }
        Ok(())
    }

    /// 虚方法调用（5.6）：从对象 vtable 查找方法并调用
    fn do_call_method(&mut self, top: usize, method_idx: usize) -> Result<(), VmError> {
        let obj = self.pop(top)?;
        match obj {
            Value::Ref(h) => {
                let func_idx = self.heap.get_vtable_method(h, method_idx as u16);
                match func_idx {
                    Some(fidx) => {
                        // 查找方法对应的函数索引：vtable 中存的是函数索引
                        // 参数在栈上（已在对象之前压入）
                        let param_count = if fidx < self.module.funcs.len() {
                            self.module.funcs[fidx].param_count as usize
                        } else {
                            return Err(VmError::Runtime(format!(
                                "vtable method #{} resolves to invalid function #{}",
                                method_idx, fidx
                            )));
                        };
                        let args = self.pop_n(top, param_count)?;
                        self.push_frame(fidx, args)?;
                    }
                    None => {
                        // 对象无 vtable 或无对应方法：报错
                        return Err(VmError::Runtime(format!(
                            "no virtual method #{} for object <ref#{}>",
                            method_idx, h
                        )));
                    }
                }
            }
            _ => {
                return Err(VmError::Runtime(
                    "method call on non-object value".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// 协程挂起（5.8）：保存当前帧栈到协程、将返回值放入挂起状态
    fn do_yield(&mut self, top: usize) -> Result<(), VmError> {
        let co_id = self.frames[top].coroutine_id;
        // 主线程（co_id=0）调用 Yield 视为 Halt
        if co_id == 0 {
            self.halt = true;
            return Ok(());
        }
        let ret_val = self.frames[top].stack.pop().unwrap_or(Value::Null);
        let frames = std::mem::take(&mut self.frames);
        self.coroutines.save_frames(co_id, frames, ret_val);
        self.halt = true;
        Ok(())
    }

    /// 恢复协程（5.8）：从栈顶弹出协程 ID 和入参，恢复协程帧栈并继续执行
    fn do_resume(&mut self, top: usize) -> Result<(), VmError> {
        let co_id_v = self.pop(top)?;
        let co_id = co_id_v.as_int() as usize;
        if co_id == 0 || co_id >= self.coroutines.active_count() + 1 {
            return Err(VmError::Runtime(format!("invalid coroutine id {}", co_id)));
        }
        // 从协程中恢复帧栈
        let frames = self
            .coroutines
            .restore_frames(co_id)
            .ok_or_else(|| VmError::Runtime(format!("coroutine #{} not found", co_id)))?;
        self.frames = frames;
        // 将栈顶入参压入当前帧的操作数栈
        // （ResumeCoroutine 指令之前已压入参数）
        Ok(())
    }

    /// 用户函数调用（含 JIT 原生派发）
    fn do_call(&mut self, top: usize, idx: usize, _from_jit: bool) -> Result<(), VmError> {
        if idx >= self.module.funcs.len() {
            return Err(VmError::Runtime(format!(
                "call to undefined function #{}",
                idx
            )));
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

        // Phase 1: AOT 预编译版本检查（设计文档 §7.2 步骤 2）。
        // 函数有 AOT 版本时直接派发到机器码，未命中则回退字节码解释。
        // 与 JIT 派发的区别：AOT 版本在加载期即就绪，无编译成本。
        if self.aot_runtime.has_entry(idx) {
            if let Some(ret) = self.try_call_aot(idx, &args) {
                self.frames[top].stack.push(ret);
                return Ok(());
            }
        }

        self.push_frame(idx, args)?;
        Ok(())
    }

    /// 尝试通过 AOT 机器码执行函数（Phase 1）
    ///
    /// 命中 [`AotRuntime`](crate::vm::aot_runtime::AotRuntime) 分发表则直接 `call`
    /// 到 mmap 的机器码（共享 JitValue ABI，零 FFI 开销）；未命中则回退
    /// 字节码解释。返回 `true` 表示已派发 AOT 版本。
    fn try_call_aot(&mut self, idx: usize, args: &[Value]) -> Option<Value> {
        let jit_args: Vec<crate::vm::abi::JitValue> =
            args.iter().map(crate::vm::abi::JitValue::from_value).collect();
        unsafe { self.aot_runtime.call_func_by_idx(idx, &jit_args) }.map(|v| v.to_value())
    }

    /// AOT 预编译函数调用（Phase 1）
    ///
    /// 从栈上收集参数 → 转换为 JitValue → 查 AOT 分发表直接 `call` 机器码。
    /// 未命中分发表（无 AOT 版本）时回退字节码解释，保证正确性。
    fn do_call_aot(&mut self, top: usize, idx: usize) -> Result<(), VmError> {
        if idx >= self.module.funcs.len() {
            return Err(VmError::Runtime(format!(
                "call to undefined function #{}",
                idx
            )));
        }
        let param_count = self.module.funcs[idx].param_count as usize;
        let args = self.pop_n(top, param_count)?;
        if let Some(ret) = self.try_call_aot(idx, &args) {
            self.frames[top].stack.push(ret);
        } else {
            self.push_frame(idx, args)?;
        }
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

        eprintln!("[vm] CallNative: {}, params={}", native.name, param_count);

        // P9: 如果指定了 FFI 库，先加载库
        if let Some(ref lib_name) = native.ffi_lib {
            self.ensure_lib_loaded(lib_name);
        }

        // P9: 获取库句柄（如果加载了）
        let lib_handle: Option<usize> = native.ffi_lib.as_ref().and_then(|lib| {
            #[cfg(windows)]
            {
                self.loaded_libs.get(lib).copied()
            }
            #[cfg(unix)]
            {
                self.loaded_libs.get(lib).map(|h| *h as usize)
            }
        });

        let result = if let Some(f) = self.natives.get(&native.name) {
            f(&args)
        } else if let Some(f) = self.natives.resolve_c_function(&native.name) {
            f(&args)
        } else {
            // P8.4: 尝试静态链接 — 使用库句柄解析 C 函数
            match static_call_c_with_lib(&native.name, &args, lib_handle) {
                Some(v) => v,
                None => {
                    eprintln!(
                        "[vm] 未链接的外部函数 `{}`，已忽略调用（参数: {:?}）",
                        native.name,
                        args.iter().map(|v| v.to_string()).collect::<Vec<_>>()
                    );
                    Value::Int(0)
                }
            }
        };
        self.frames[top].stack.push(result);
        Ok(())
    }

    /// P9: 确保动态库已加载
    #[cfg(windows)]
    fn ensure_lib_loaded(&mut self, lib_name: &str) {
        if self.loaded_libs.contains_key(lib_name) {
            return;
        }
        // 尝试多个可能的路径
        let paths = [
            lib_name.to_string(),
            format!("{}.dll", lib_name),
            format!(
                "D:\\Code\\AuraProjs\\SQLura\\sqlura-driver-rs\\target\\release\\{}.dll",
                lib_name
            ),
            format!(
                "D:\\Code\\AuraProjs\\SQLura\\sqlura-driver-rs\\target\\release\\{}",
                lib_name
            ),
        ];

        for path in &paths {
            let wide: Vec<u16> =
                std::ffi::OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();
            unsafe {
                let handle = LoadLibraryW(wide.as_ptr());
                if handle != 0 {
                    self.loaded_libs.insert(lib_name.to_string(), handle);
                    eprintln!("[vm] 已加载库: {} ({})", lib_name, path);
                    return;
                }
            }
        }
        eprintln!("[vm] 无法加载库: {}", lib_name);
    }

    #[cfg(unix)]
    fn ensure_lib_loaded(&mut self, lib_name: &str) {
        if self.loaded_libs.contains_key(lib_name) {
            return;
        }
        let paths = [
            lib_name.to_string(),
            format!("lib{}.so", lib_name),
            format!(
                "D:\\Code\\AuraProjs\\SQLura\\sqlura-driver-rs\\target\\release\\lib{}.so",
                lib_name
            ),
        ];

        for path in &paths {
            let c_path = std::ffi::CString::new(path.clone()).unwrap();
            unsafe {
                let handle = dlopen(c_path.as_ptr(), 2); // RTLD_NOW = 2
                if !handle.is_null() {
                    self.loaded_libs.insert(lib_name.to_string(), handle);
                    eprintln!("[vm] 已加载库: {} ({})", lib_name, path);
                    return;
                }
            }
        }
        eprintln!("[vm] 无法加载库: {}", lib_name);
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

/// P8.4: 静态链接 C 函数调用
///
/// 使用 `dlsym(NULL, name)` / `GetProcAddress` 解析 C 函数符号，
/// 将参数转换为 `i64` 数组（最多 4 个），调用 C 函数，返回结果。
/// 成功时返回 `Some(Value)`，失败时返回 `None`。
fn static_call_c(name: &str, args: &[Value]) -> Option<Value> {
    static_call_c_with_lib(name, args, None)
}

/// P9: 使用指定库句柄调用 C 函数
fn static_call_c_with_lib(name: &str, args: &[Value], lib_handle: Option<usize>) -> Option<Value> {
    use crate::vm::ffi::{CFuncInfo, CFuncPtr, CType, resolve_static_symbol};

    // 如果有库句柄，从库中解析符号
    let addr = if let Some(handle) = lib_handle {
        resolve_symbol_in_lib(handle, name)
    } else {
        resolve_static_symbol(name)
    }?;

    let ptr: CFuncPtr = unsafe { std::mem::transmute(addr) };

    // P9: 根据函数名确定返回类型和参数类型
    // SQLura 数据库 API 函数签名
    let (param_types, return_type) = match name {
        "sqlura_version" => {
            (vec![], CType::CString) // 返回字符串
        }
        "sqlura_open" => {
            (vec![CType::CString], CType::Ptr) // 参数: path, 返回: handle
        }
        "sqlura_close" => {
            (vec![CType::Ptr], CType::Int64) // 参数: handle, 返回: int
        }
        "sqlura_exec" => {
            (
                vec![
                    CType::Ptr,
                    CType::CString,
                ],
                CType::CString,
            ) // 参数: handle, sql, 返回: string
        }
        "sqlura_free_string" => {
            (vec![CType::CString], CType::Void) // 参数: ptr, 返回: void
        }
        "sqlura_error" => {
            (vec![CType::Ptr], CType::CString) // 参数: handle, 返回: string
        }
        _ => (vec![CType::Int64; args.len().min(8)], CType::Int64), // 默认
    };

    // P9: 创建 CString 对象保持生命周期
    let mut c_strings: Vec<std::ffi::CString> = Vec::new();

    // P9: 根据参数类型打包参数
    let c_args: [i64; 8] = [
        args.first()
            .map(|v| {
                let ty = param_types.first().unwrap_or(&CType::Int64);
                if *ty == CType::CString {
                    match v {
                        Value::Str(s) => {
                            // 创建 CString 并保持生命周期
                            if let Ok(cs) = std::ffi::CString::new(s.as_ref()) {
                                let ptr = cs.as_ptr() as i64;
                                c_strings.push(cs); // 保持生命周期
                                ptr
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    }
                } else {
                    ty.pack(v)
                }
            })
            .unwrap_or(0),
        args.get(1)
            .map(|v| {
                let ty = param_types.get(1).unwrap_or(&CType::Int64);
                if *ty == CType::CString {
                    match v {
                        Value::Str(s) => {
                            if let Ok(cs) = std::ffi::CString::new(s.as_ref()) {
                                let ptr = cs.as_ptr() as i64;
                                c_strings.push(cs);
                                ptr
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    }
                } else {
                    ty.pack(v)
                }
            })
            .unwrap_or(0),
        args.get(2)
            .map(|v| {
                let ty = param_types.get(2).unwrap_or(&CType::Int64);
                if *ty == CType::CString {
                    match v {
                        Value::Str(s) => {
                            if let Ok(cs) = std::ffi::CString::new(s.as_ref()) {
                                let ptr = cs.as_ptr() as i64;
                                c_strings.push(cs);
                                ptr
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    }
                } else {
                    ty.pack(v)
                }
            })
            .unwrap_or(0),
        args.get(3)
            .map(|v| {
                let ty = param_types.get(3).unwrap_or(&CType::Int64);
                if *ty == CType::CString {
                    match v {
                        Value::Str(s) => {
                            if let Ok(cs) = std::ffi::CString::new(s.as_ref()) {
                                let ptr = cs.as_ptr() as i64;
                                c_strings.push(cs);
                                ptr
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    }
                } else {
                    ty.pack(v)
                }
            })
            .unwrap_or(0),
        0,
        0,
        0,
        0, // 最多 4 个参数
    ];
    let result = unsafe {
        ptr(
            c_args[0], c_args[1], c_args[2], c_args[3], c_args[4], c_args[5], c_args[6], c_args[7],
        )
    };
    // c_strings 在此处才被销毁，确保 C 函数调用期间字符串有效
    eprintln!("[vm] FFI call {}: result={:x}", name, result);
    Some(return_type.unpack(result))
}

/// P9: 在指定库中解析符号
#[cfg(windows)]
fn resolve_symbol_in_lib(handle: usize, name: &str) -> Option<usize> {
    // Windows: GetProcAddress 使用 ANSI 字符串（char*）
    let c_name = std::ffi::CString::new(name).ok()?;
    unsafe {
        let ptr = GetProcAddress(handle, c_name.as_ptr());
        if ptr == 0 { None } else { Some(ptr) }
    }
}

#[cfg(unix)]
fn resolve_symbol_in_lib(handle: usize, name: &str) -> Option<usize> {
    let c_name = std::ffi::CString::new(name).ok()?;
    unsafe {
        let ptr = dlsym(handle as *mut std::os::raw::c_void, c_name.as_ptr());
        if ptr.is_null() { None } else { Some(ptr as usize) }
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
    // 字符串拼接：任一操作数为字符串时执行拼接
    if matches!(a, Value::Str(_)) || matches!(b, Value::Str(_)) {
        return Value::str_(&format!("{}{}", a, b));
    }
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
        if y == 0 { Value::Int(0) } else { Value::Int(x.wrapping_div(y)) }
    } else {
        Value::Float(a.as_float() / b.as_float())
    }
}

fn bin_rem(a: Value, b: Value) -> Value {
    if both_int(&a, &b) {
        let (x, y) = (a.as_int(), b.as_int());
        if y == 0 { Value::Int(0) } else { Value::Int(x.wrapping_rem(y)) }
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
