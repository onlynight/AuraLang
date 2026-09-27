//! Aura VM 解释器执行循环（直接线程码分派）
//!
//! 主循环 `step()` 对栈顶帧逐条执行指令。`Call`/`Return` 切换调用帧；
//! `CallNative`/`CallC` 经原生注册表分发；`NewObject`/`GetField`/`SetField`
//! 经堆管理器操作对象；`IncRef`/`DecRef` 维护 ARC。

use crate::codegen::opcode::FfiAbi;
use crate::vm::value::Value;
use crate::vm::{Handler, Instr, Vm, VmError};

/// 未链接外部函数告警：**每个名字只报一次**，实参只做截断预览。
///
/// 原实现每次调用都 `args.iter().map(|v| v.to_string())`：当实参里含大列表时，
/// 单条日志就要构造数百 KB 字符串。实测 4 万次 `list.get(i)`（裸名 `get` 未注册
/// → 走本兜底）峰值内存 **23GB**、耗时超过一分钟 —— 「未链接告警」自己变成了
/// OOM 元凶。现改为名字去重 + 预览截断（80 字符 + 原始长度）。
fn warn_unlinked_once(name: &str, args: &[Value]) {
    use std::cell::RefCell;
    use std::collections::HashSet;
    thread_local! {
        static WARNED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    }
    let first = WARNED.with(|w| w.borrow_mut().insert(name.to_string()));
    if !first {
        return;
    }
    let preview: Vec<String> = args
        .iter()
        .take(4)
        .map(|v| {
            let s = v.to_string();
            let n = s.chars().count();
            if n > 80 {
                format!("{}…<{} chars>", s.chars().take(80).collect::<String>(), n)
            } else {
                s
            }
        })
        .collect();
    eprintln!(
        "[vm] Unlinked external function `{}`, call ignored (args: {:?})",
        name, preview
    );
}

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

/// 是否为「请求进程退出」的原生函数（`Process.exit` / `Process.exitProcess`）。
fn is_exit_native(name: &str) -> bool {
    name.ends_with("Process.exit") || name.ends_with("Process.exitProcess")
}

impl Vm {
    /// 执行一条指令（栈顶帧）
    pub(crate) fn step(&mut self) -> Result<(), VmError> {
        let top = self.frames.len() - 1;
        let func = self.frames[top].func;
        let ip = self.frames[top].ip;

        // ── 死循环看门狗（诊断）──────────────────────────────────────────
        // `AURA_VM_WATCH=N`：每执行 N 条指令打印一次当前帧栈（函数名 + ip）。
        //
        // 某些死循环**完全不调用任何 std 函数**（纯 Aura 计算），因此
        // stderr 上不会有任何 stdlib 派发日志可看 —— 自举编译器里
        // `EmitBuffer`（AOT 发射）就这类：实测 180s 只有 6 次
        // `StringBuilder.create`，其余全是空转。打开本开关后，
        // 输出尾部就是正在空转的函数与指令位置。
        //
        // 例：AURA_VM_WATCH=5000000 aura run build/auc/compiler/aura-compiler.auc
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            use std::sync::OnceLock;
            static WATCH_INTERVAL: OnceLock<u64> = OnceLock::new();
            static WATCH_TICK: AtomicU64 = AtomicU64::new(0);
            let interval = *WATCH_INTERVAL.get_or_init(|| {
                std::env::var("AURA_VM_WATCH")
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
            });
            if interval > 0 {
                let tick = WATCH_TICK.fetch_add(1, Ordering::Relaxed) + 1;
                if tick % interval == 0 {
                    let mut stack = String::new();
                    for (i, fr) in self.frames.iter().rev().enumerate() {
                        if i >= 16 {
                            stack.push_str("... ");
                            break;
                        }
                        let n = self
                            .module
                            .funcs
                            .get(fr.func)
                            .map(|f| f.name.as_str())
                            .unwrap_or("<bad>");
                        stack.push_str(&format!("{}@{} <- ", n, fr.ip));
                    }
                    eprintln!(
                        "[vm] watch: tick={} depth={} {}",
                        tick,
                        self.frames.len(),
                        stack
                    );
                    // 栈顶帧的局部变量（定位「参数异常」用，例如
                    // `StringBuilder.reserve(sb, need)` 里的 need 是否为天文数字）
                    if let Some(fr) = self.frames.last() {
                        let locs: Vec<String> = fr
                            .locals
                            .iter()
                            .take(6)
                            .map(|v| format!("{:?}", v))
                            .collect();
                        eprintln!("[vm]   locals: [{}]", locs.join(", "));
                    }
                }
            }
        }

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
            Instr::Eq => bin_op(self, top, value_eq_abi)?,
            Instr::Ne => bin_op(self, top, value_ne_abi)?,
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
            Instr::CallNativeArgs(idx, argc) => {
                self.do_call_native_args(top, idx as usize, argc as usize)?
            }
            Instr::Return => {
                let v = self.pop(top)?;
                self.pop_frame(v);
            }
            Instr::ReturnUnit => {
                self.pop_frame(Value::Null);
            }

            // ── 对象 / 数组 ──
            Instr::NewObject(type_tag) => {
                // P-K2：类对象挂虚方法表（open 方法动态分派）
                const NO_METHOD: u16 = u16::MAX;
                let h = match self.module.module.vtables.iter().find(|vt| vt.type_tag == type_tag) {
                    Some(vt) => {
                        let map: std::collections::HashMap<u16, usize> = vt
                            .slots
                            .iter()
                            .enumerate()
                            .filter_map(|(i, &f)| {
                                if f != NO_METHOD { Some((i as u16, f as usize)) } else { None }
                            })
                            .collect();
                        self.heap.alloc_object_with_vtable(type_tag, map)
                    }
                    None => self.heap.alloc_object(type_tag),
                };
                self.frames[top].stack.push(Value::Ref(h));
            }
            Instr::NewArray => {
                let len = self.pop(top)?.as_int().max(0) as usize;
                let h = self.heap.alloc_array(len);
                self.frames[top].stack.push(Value::Ref(h));
            }
            Instr::GetField(field) => {
                let obj = self.pop(top)?;
                let v = match &obj {
                    // 堆对象：类实例按字段查；**堆列表/堆映射**没有字段，
                    // 退回内建成员（`size`/`length`/`first`/`last`/`isEmpty`）。
                    // 背景：`mutableListOf(...)` 现为堆列表（`HeapData::List`），
                    // 而类型通道不总能把 `xs.size` 降级为 `LIST_LEN`。
                    Value::Ref(h) => {
                        let is_size = field == crate::codegen::emit::field_index("size")
                            || field == crate::codegen::emit::field_index("length");
                        let is_empty = field == crate::codegen::emit::field_index("isEmpty");
                        let is_first = field == crate::codegen::emit::field_index("first");
                        let is_last = field == crate::codegen::emit::field_index("last");
                        match self.heap.get_data(*h) {
                            Some(crate::vm::heap::HeapData::List(items)) => {
                                if is_size {
                                    Value::Int(items.len() as i64)
                                } else if is_empty {
                                    Value::Bool(items.is_empty())
                                } else if is_first {
                                    items.first().cloned().unwrap_or(Value::Null)
                                } else if is_last {
                                    items.last().cloned().unwrap_or(Value::Null)
                                } else {
                                    self.heap.get_field(*h, field)
                                }
                            }
                            Some(crate::vm::heap::HeapData::Map(m)) => {
                                if is_size {
                                    Value::Int(m.len() as i64)
                                } else if is_empty {
                                    Value::Bool(m.is_empty())
                                } else {
                                    self.heap.get_field(*h, field)
                                }
                            }
                            _ => self.heap.get_field(*h, field),
                        }
                    }
                    // 集合 / 字符串的内建成员（`xs.size` / `s.length` / `xs.first` ...）
                    other => builtin_member(other.clone(), field),
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
                    // P15: Value::List（listOf / pairOf 产生的内联列表）直接索引
                    Value::List(items) => {
                        let i = idx_v.as_int().max(0) as usize;
                        items.get(i).cloned().unwrap_or(Value::Null)
                    }
                    // 字符串索引：`s[i]` → 单字符字符串（越界返回 null）。
                    // 语义对齐 sema（`Ty::String` 索引结果为 `Char`，运行时以单字符串表示）。
                    Value::Str(s) => {
                        let i = idx_v.as_int().max(0) as usize;
                        match s.chars().nth(i) {
                            Some(c) => Value::str_(c.to_string()),
                            None => Value::Null,
                        }
                    }
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

            // ── 类型检查（Phase 2） ──
            Instr::InstanceOf(target_id) => {
                let v = self.pop(top)?;
                let is_match = self.is_instance_of(&v, target_id);
                self.frames[top].stack.push(Value::Bool(is_match));
            }
            Instr::CheckCast(target_id) => {
                let v = self.pop(top)?;
                if self.is_instance_of(&v, target_id) {
                    self.frames[top].stack.push(v);
                } else {
                    return Err(VmError::Runtime(format!(
                        "CheckCast failed: cannot cast value to class #{}",
                        target_id
                    )));
                }
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
                match obj {
                    Value::Ref(h) => self.heap.list_push(h, val),
                    // 内联列表（native 产出，值语义）无法原地追加：
                    // 此前 `.add` 根本无法编译到此处，故保持「不改变原值」不构成回归。
                    _ => {}
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
                // 同时兼容堆列表（`Value::Ref`）与内联列表（native 产出，如 `split`）
                let len = match obj {
                    Value::Ref(h) => self.heap.list_len(h),
                    Value::List(items) => items.len() as i64,
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

            // ── 异常处理（try/catch）──
            Instr::PushHandler(handler_ip, slot, catch_type) => {
                self.handlers.push(Handler {
                    frame_index: top,
                    ip: handler_ip,
                    stack_len: self.frames[top].stack.len(),
                    slot,
                    catch_type,
                });
            }
            Instr::PopHandler => {
                self.handlers.pop();
            }

            // ── Phase B: 并发运行时指令 ──
            Instr::ThreadSpawn(func_idx) => {
                // 创建线程执行函数（当前为占位实现）
                self.frames[top].stack.push(Value::Int(func_idx as i64));
            }
            Instr::ThreadJoin => {
                let tid = self.pop(top)?.as_int();
                self.frames[top].stack.push(Value::Int(tid));
            }
            Instr::ThreadSleep => {
                let ms = self.pop(top)?.as_int();
                if ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
                }
                self.frames[top].stack.push(Value::Null);
            }
            Instr::ThreadId => {
                // 返回当前线程 ID（简化为 0）
                self.frames[top].stack.push(Value::Int(0));
            }
            Instr::ThreadParallelism => {
                let cores =
                    std::thread::available_parallelism().map(|n| n.get() as i64).unwrap_or(1);
                self.frames[top].stack.push(Value::Int(cores));
            }
            Instr::MutexNew => {
                self.frames[top].stack.push(Value::Int(0)); // 占位
            }
            Instr::MutexLock => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::MutexUnlock => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::MutexTryLock => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Bool(true));
            }
            Instr::AtomicNew => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Int(0)); // 占位
            }
            Instr::AtomicLoad => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Int(0));
            }
            Instr::AtomicStore => {
                let _val = self.pop(top)?;
                let _handle = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::AtomicAdd => {
                let _delta = self.pop(top)?;
                let _handle = self.pop(top)?;
                self.frames[top].stack.push(Value::Int(0));
            }
            Instr::AtomicCas => {
                let _expected = self.pop(top)?;
                let _desired = self.pop(top)?;
                let _handle = self.pop(top)?;
                self.frames[top].stack.push(Value::Bool(false));
            }
            Instr::RwLockNew => {
                self.frames[top].stack.push(Value::Int(0));
            }
            Instr::RwLockReadLock => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::RwLockWriteLock => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::RwLockReadUnlock => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::RwLockWriteUnlock => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::ChannelNew => {
                let _cap = self.pop(top)?.as_int();
                self.frames[top].stack.push(Value::Int(0));
            }
            Instr::ChannelSend => {
                let _val = self.pop(top)?;
                let _handle = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::ChannelRecv => {
                let _handle = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::CondvarNew => {
                self.frames[top].stack.push(Value::Int(0));
            }
            Instr::CondvarWait => {
                let _mutex = self.pop(top)?;
                let _condvar = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::CondvarSignal => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
            }
            Instr::CondvarBroadcast => {
                let _ = self.pop(top)?;
                self.frames[top].stack.push(Value::Null);
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
                        VmError::Runtime(format!(
                            "MakeClosure: closure {} does not exist",
                            closure_idx
                        ))
                    })?;
                let closure_name = closure_info.name.clone();
                let param_count = closure_info.param_count;
                let locals = closure_info.locals;
                let capture_count = closure_info.capture_count as usize;
                let func_idx = closure_info.func_idx as usize;
                // 弹出捕获值（按序）：栈顶是最后一个捕获，弹出后需反转为声明顺序
                let mut captures = Vec::with_capacity(capture_count);
                for _ in 0..capture_count {
                    captures.push(self.pop(top)?);
                }
                captures.reverse();
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
                    .ok_or_else(|| VmError::Runtime("CallClosure: empty stack".to_string()))?;
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
                    VmError::Runtime(format!(
                        "CallExport: export symbol {} does not exist",
                        sym_idx
                    ))
                })?;
                let func_idx = export.func_idx.ok_or_else(|| {
                    VmError::Runtime(format!(
                        "CallExport: export symbol {} has no function index",
                        export.name
                    ))
                })?;
                self.do_call(top, func_idx as usize, false)?;
            }
            Instr::CallExternal(mod_idx, sym_idx) => {
                // 从导入表查找外部模块
                let import = self.module.module.imports.get(mod_idx as usize).ok_or_else(|| {
                    VmError::Runtime(format!(
                        "CallExternal: import module {} does not exist",
                        mod_idx
                    ))
                })?;
                // 在注册表中查找目标模块
                let target =
                    self.registry.find_export(&import.module, &import.symbol).ok_or_else(|| {
                        VmError::Runtime(format!(
                            "CallExternal: module {} not loaded or symbol {} does not exist",
                            import.module, import.symbol
                        ))
                    })?;
                // 从目标模块的导出索引获取函数索引
                let (_, func_idx) = target.export_index.get(&import.symbol).ok_or_else(|| {
                    VmError::Runtime(format!(
                        "CallExternal: symbol {} has no function index",
                        import.symbol
                    ))
                })?;
                self.do_call(top, *func_idx as usize, false)?;
            }
            // Phase 1: AOT 嵌入调用 —— 查 AotRuntime dispatch_table 后直接 call 机器码
            Instr::CallAot(idx) => self.do_call_aot(top, idx as usize)?,
        }
        Ok(())
    }

    /// 按类名查找类 ID（Phase 1：编译期分配的递增 ID）
    fn class_id_by_name(&self, name: &str) -> Option<u16> {
        self.module.module.classes.iter().position(|c| c.name == name).map(|p| p as u16)
    }

    /// 按类 ID 取类名
    fn class_name(&self, type_tag: u16) -> String {
        self.module
            .module
            .classes
            .get(type_tag as usize)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "Any".to_string())
    }

    /// 运行时类型检查（Phase 4）：沿继承链判断 `value` 是否为 `target_id` 的实例
    fn is_instance_of(&self, value: &Value, target_id: u16) -> bool {
        // class ID 0 是内置顶级类 Any：所有值都是它的实例
        if target_id == 0 {
            return true;
        }
        match value {
            Value::Ref(h) => {
                if let Some(crate::vm::heap::HeapData::Object {
                    type_tag, ..
                }) = self.heap.get_data(*h)
                {
                    let mut cur = Some(*type_tag);
                    while let Some(id) = cur {
                        if id == target_id {
                            return true;
                        }
                        if id as usize >= self.module.module.classes.len() {
                            break;
                        }
                        let parent = self.module.module.classes[id as usize].parent_id;
                        cur = if parent == u16::MAX { None } else { Some(parent) };
                    }
                    false
                } else {
                    let target = self.class_name(target_id);
                    value.type_name() == target
                }
            }
            _ => {
                let target = self.class_name(target_id);
                value.type_name() == target
            }
        }
    }

    /// 窄整型类型名（数值语义，不是类实例语义）。
    ///
    /// 这些名字在 `core` 里有同名包装类，会命中 `class_id_by_name`，
    /// 因此必须在类检查之前拦截做数值转换（见 `aura_cast`）。
    fn is_narrow_int_type_name(name: &str) -> bool {
        matches!(
            name,
            "Byte"
                | "Short"
                | "Char"
                | "UByte"
                | "UShort"
                | "Int8"
                | "Int16"
                | "UInt8"
                | "UInt16"
        )
    }

    /// 基本类型名匹配（`as?` 对基本类型做严格类型判断，而非数值转换）
    fn basic_type_matches(value: &Value, target_name: &str) -> bool {
        let tn = value.type_name();
        match target_name {
            "Int" | "Long" | "Int32" | "Int64" => tn == "Int",
            // 窄整型在 VM 里统一以 Int 承载（同 `native_cast`）
            "Byte" | "Short" | "Char" | "UByte" | "UShort" | "Int8" | "Int16" | "UInt8"
            | "UInt16" => tn == "Int",
            "Float" | "Double" | "Number" | "Float32" | "Float64" => tn == "Float",
            "Boolean" | "Bool" => tn == "Boolean",
            "String" => tn == "String",
            "Null" => tn == "Null",
            _ => false,
        }
    }

    /// Object 基类原生函数拦截（Phase 4）：`typeOf` / `aura_isOfType` /
    /// `aura_cast` / `aura_cast_safety` 需要访问堆对象的类标签，故在 VM 层处理。
    /// 返回 `None` 表示不是被拦截的函数，交由常规原生注册表分派。
    fn intercept_object_native(
        &self,
        name: &str,
        args: &[Value],
    ) -> Result<Option<Value>, VmError> {
        let result = match name {
            "typeOf" if !args.is_empty() => {
                let value = &args[0];
                match value {
                    Value::Ref(h) => {
                        if let Some(crate::vm::heap::HeapData::Object {
                            type_tag, ..
                        }) = self.heap.get_data(*h)
                        {
                            Value::str_(self.class_name(*type_tag))
                        } else {
                            Value::str_(value.type_name())
                        }
                    }
                    _ => Value::str_(value.type_name()),
                }
            }
            "aura_isOfType" if args.len() >= 2 => {
                let target_name = args[1].as_string();
                match self.class_id_by_name(&target_name) {
                    Some(id) => Value::Bool(self.is_instance_of(&args[0], id)),
                    None => Value::Bool(args[0].type_name() == target_name),
                }
            }
            "aura_cast" if args.len() >= 2 => {
                let target_name = args[1].as_string();
                // 窄整型（Byte/Short/Char/…）：先做**数值转换**，不要走下面的类实例检查。
                //
                // `core` 里存在 `Byte.aura` / `Short.aura` 等包装类，所以
                // `class_id_by_name("Byte")` 会**命中**，旧的 `Some(id)` 分支用
                // `is_instance_of` 严格判断 → `0 as Byte`（Int 值）被判为不可转换，
                // 运行期抛 `as cast failed: cannot cast value to class 'Byte'`。
                // 实测影响面（冻结种子下 10 行探针即可复现）：
                //   * 任何 `x as Byte` 都崩 —— 包括 `Memory.write(addr, 0 as Byte)`；
                //   * 内嵌 std 的 `StringBuilder.create()` 第一行就是 `0 as Byte`，
                //     于是自举编译器（Lexer 依赖 StringBuilder）一启动就死。
                // 注意 BASIC_TYPES 白名单只覆盖 `None` 分支，且缺 Byte/Short/Char。
                if Self::is_narrow_int_type_name(&target_name) {
                    return Ok(Some(crate::vm::native::native_cast(args)));
                }
                match self.class_id_by_name(&target_name) {
                    Some(id) => {
                        if self.is_instance_of(&args[0], id) {
                            args[0].clone()
                        } else {
                            return Err(VmError::Runtime(format!(
                                "as cast failed: cannot cast value to class '{}'",
                                target_name
                            )));
                        }
                    }
                    None => {
                        const BASIC_TYPES: [&str; 9] = [
                            "Int", "Long", "Float", "Double", "Number", "Boolean", "Bool",
                            "String", "Null",
                        ];
                        if BASIC_TYPES.contains(&target_name.as_str()) {
                            crate::vm::native::native_cast(args)
                        } else {
                            return Err(VmError::Runtime(format!(
                                "as cast failed: cannot cast value to class '{}'",
                                target_name
                            )));
                        }
                    }
                }
            }
            "aura_cast_safety" if args.len() >= 2 => {
                let target_name = args[1].as_string();
                match self.class_id_by_name(&target_name) {
                    Some(id) => {
                        if self.is_instance_of(&args[0], id) {
                            args[0].clone()
                        } else {
                            Value::Null
                        }
                    }
                    // 基本类型：严格判断实际类型，不匹配则返回 null（as? 语义）
                    None => {
                        if Self::basic_type_matches(&args[0], &target_name) {
                            args[0].clone()
                        } else {
                            Value::Null
                        }
                    }
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
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

        // Phase 2: object 单例拦截（字段读取 + 方法调用）
        let func_name = self.module.funcs[idx].name.clone();
        if let Some(dot_pos) = func_name.find('.') {
            let class_name = func_name[..dot_pos].to_string();
            let member_name = func_name[dot_pos + 1..].to_string();
            let singleton_val = self.singletons.get(&class_name).cloned();
            if let Some(singleton_val) = singleton_val {
                if let Value::Ref(handle) = singleton_val {
                    // 判断是字段读取还是方法调用
                    if param_count == 0 {
                        // 尝试作为字段读取
                        if let Some(type_id) = self.class_id_by_name(&class_name) {
                            let field_names =
                                self.module.module.classes[type_id as usize].field_names.clone();
                            if let Some(field_idx) =
                                field_names.iter().position(|n| n == &member_name)
                            {
                                let field_val = self.heap.get_field(handle, field_idx as u16);
                                // 字段一旦被赋值即以堆值为准；尚未赋值（单例创建时统一置为 Null）
                                // 时**不返回**，继续走常规调用路径执行 HIR 合成的
                                // `<Object>.<field>` 零参读取函数，从而返回字段声明的默认值。
                                // （`create_singletons` 只能把字段置为 Null，VM 侧拿不到默认值。）
                                if field_val != Value::Null {
                                    self.frames[top].stack.push(field_val);
                                    return Ok(());
                                }
                            }
                        }
                    } else {
                        // 方法调用：传递单例实例作为 self 参数
                        let args = self.pop_n(top, param_count - 1)?;
                        let mut all_args = vec![singleton_val];
                        all_args.extend(args);
                        self.push_frame(idx, all_args)?;
                        return Ok(());
                    }
                }
            }
        }

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

    /// 抛出异常：查找最近的异常处理器并展开到它；无处理器则为未捕获异常。
    ///
    /// 展开步骤（标准栈式异常处理）：
    /// 1. 从 handler 栈顶弹出最近的处理器；
    /// 2. 若 `catch_type != u16::MAX`，检查异常值是否为 catch_type 的实例；不匹配则跳过；
    /// 3. 若异常值是纯字符串且 catch_type 是 Exception/Throwable/其子类，自动包装为 Exception 对象；
    /// 4. 若 catch_type 是 String，从 Exception 对象中提取 message 字段；
    /// 5. 把帧栈截断到处理器所在帧（丢弃其间的调用帧）；
    /// 6. 异常值写入处理器的槽位（`u16::MAX` 表示无落点，退化为压栈）；
    /// 7. 跳转到处理器入口。
    fn raise(&mut self, value: Value) -> Result<(), VmError> {
        let original_value = value.clone();
        // 查找 Exception 类的 type_tag（用于自动包装字符串异常）
        let exception_type_tag = self.class_id_by_name("Exception").unwrap_or(u16::MAX);
        while let Some(h) = self.handlers.pop() {
            // 类型过滤：若 catch_type 不是 catch-all，检查异常值是否匹配
            if h.catch_type != u16::MAX {
                if !self.is_instance_of(&value, h.catch_type) {
                    continue; // 类型不匹配，继续向上查找
                }
            }
            if h.frame_index < self.frames.len() {
                // 计算槽位值：根据 catch_type 和值类型决定写入什么
                let slot_value = self.compute_slot_value(&value, h.catch_type, exception_type_tag);
                let slot = h.slot;
                let ip = h.ip;
                let stack_len = h.stack_len;
                self.frames.truncate(h.frame_index + 1);
                let frame = &mut self.frames[h.frame_index];
                frame.stack.truncate(stack_len);
                if slot != u16::MAX && (slot as usize) < frame.locals.len() {
                    frame.locals[slot as usize] = slot_value;
                } else {
                    frame.stack.push(slot_value);
                }
                frame.ip = ip;
                return Ok(());
            }
        }
        Err(VmError::Runtime(format!(
            "uncaught exception: {}",
            original_value
        )))
    }

    /// 计算异常槽位值：根据 catch 类型和异常值类型决定写入什么。
    ///
    /// - 如果值是纯字符串且 catch_type 是 Exception/Throwable/其子类：包装为 Exception 对象
    /// - 如果 catch_type 是 String 或 catch-all（u16::MAX）：从 Exception 对象提取 message
    /// - 其他情况：原样写入
    fn compute_slot_value(
        &mut self,
        value: &Value,
        catch_type: u16,
        exception_type_tag: u16,
    ) -> Value {
        let is_string_catch = self.is_string_type(catch_type);
        let is_exception_like = catch_type != u16::MAX && self.is_exception_like(catch_type);
        let is_catch_all = catch_type == u16::MAX;

        if let Value::Str(msg) = value {
            // 纯字符串异常
            if is_exception_like && exception_type_tag != u16::MAX {
                // 包装为 Exception 对象
                self.wrap_string_in_exception(msg, exception_type_tag)
            } else {
                // catch_type 是 String 或 catch-all：直接返回字符串
                value.clone()
            }
        } else {
            // 异常对象（堆引用或其他值）
            if is_string_catch || is_catch_all {
                // 从 Exception 对象提取 message（兼容 String catch 和 catch-all）
                self.extract_message(value)
            } else {
                value.clone()
            }
        }
    }

    /// 判断 class_id 是否表示 Exception 或其子类（Exception/Throwable/Error 等）
    fn is_exception_like(&self, class_id: u16) -> bool {
        if class_id == u16::MAX {
            return false;
        }
        let name = self.class_name(class_id);
        matches!(
            name.as_str(),
            "Exception"
                | "Throwable"
                | "Error"
                | "RuntimeException"
                | "IllegalArgumentException"
                | "IllegalStateException"
                | "NullPointerException"
                | "IndexOutOfBoundsException"
                | "ArrayIndexOutOfBoundsException"
                | "EmptyListException"
                | "UnsupportedOperationException"
                | "ArithmeticException"
                | "ClassCastException"
                | "IOException"
                | "FileNotFoundException"
                | "TimeoutException"
                | "AssertionError"
                | "OutOfMemoryError"
                | "StackOverflowError"
        )
    }

    /// 将字符串包装为 Exception 对象
    ///
    /// 分配一个新的堆对象（Exception 类型），将字符串写入 message 字段，
    /// 返回堆引用。用于 VM 层自动包装 `throw "string"` 为 Exception 对象。
    fn wrap_string_in_exception(&mut self, msg: &str, exception_type_tag: u16) -> Value {
        if exception_type_tag == u16::MAX {
            return Value::Str(std::rc::Rc::from(msg));
        }
        let h = self.heap.alloc_object(exception_type_tag);
        // message 字段索引由 FNV-1a 哈希计算（与 emit.rs::field_index 一致）
        let field_idx = crate::codegen::emit::field_index("message");
        self.heap.set_field(h, field_idx, Value::Str(std::rc::Rc::from(msg)));
        Value::Ref(h)
    }

    /// 判断 catch_type 是否表示 String 类型
    fn is_string_type(&self, class_id: u16) -> bool {
        if class_id == u16::MAX {
            return false;
        }
        self.class_name(class_id) == "String"
    }

    /// 从异常对象中提取 message 字段
    ///
    /// 如果 value 是 Exception 对象（堆引用），通过 fields 哈希表按字段名提取 message；
    /// 如果是纯字符串值，直接返回。
    fn extract_message(&self, value: &Value) -> Value {
        if let Value::Ref(handle) = value {
            if let Some(crate::vm::heap::HeapData::Object { fields, .. }) =
                self.heap.get_data(*handle)
            {
                // message 字段索引由 FNV-1a 哈希计算
                let field_idx = crate::codegen::emit::field_index("message");
                if let Some(msg) = fields.get(&field_idx) {
                    return msg.clone();
                }
                // 兜底：取第一个字段值
                if let Some(msg) = fields.values().next() {
                    return msg.clone();
                }
            }
        }
        value.clone()
    }

    /// 创建异常对象：根据类名查找 type_tag，分配堆对象，设置 message 字段。
    /// 用于 `__new_exception(type_name, message)` 原生函数。
    ///
    /// 注意：由于嵌入式标准库的类 ID 未与宿主模块合并，此处使用简单的堆对象
    /// （type_tag=0 表示 Any）来承载 message 字段。VM 的 catch-all 机制确保
    /// 无论类型如何都能被捕获。
    fn create_exception_object(&mut self, type_name: &str, msg: &str) -> Value {
        // 分配一个通用对象（type_tag=0 为 Any），设置 message 字段
        let h = self.heap.alloc_object(0);
        let field_idx = crate::codegen::emit::field_index("message");
        self.heap.set_field(h, field_idx, Value::Str(std::rc::Rc::from(msg)));
        Value::Ref(h)
    }

    /// 原位集合写入：`set(list, i, v)` / `listSet(list, i, v)`。
    ///
    /// 堆列表（`Value::Ref`）必须**原地**写；值列表（`Value::List`）只能返回值语义的
    /// 新列表（保持既有 native 语义）。返回 `Some(结果)` 表示已在解释器层处理完。
    ///
    /// 背景：前端把 `l.set(i, v)` 与 `arr[i] = v` 都重写为 `Collections.set(...)`，
    /// 而 `std_collections::nat_set` / `nat_list_set` 只认 `Value::List`，对堆列表
    /// 一律返回 `Null` —— photon 后端 `RegisterAllocator` 的
    /// `liveAtEnd.set(j, liveOut)` 因此既没有效果、还会把变量写成 null。
    fn try_inline_coll_set(&mut self, name: &str, args: &[Value]) -> Option<Value> {
        // ── Map 写入：`m.put(k, v)` → `Collections.hashMapPut` ──────────────
        //
        // 与下面的 `set` 同源：VM 有两套映射表示，只有 `Value::Ref`（堆表示，
        // `mutableMapOf()` 产出）**原位**写入才有效果。
        //   * 堆表示 → 本函数原地 `map_set` 并返回同一句柄；
        //   * 值表示 `Value::Map` → 交给 Rust native 走值语义（返回新映射，
        //     由调用方回赋），此处返回 `None` 放行；
        //   * 非映射接收者 → 同样放行，避免把调用静默吞掉。
        // 没有这一层时，`m.put(k, v)` 作为**语句**使用会毫无效果（返回值被丢弃）。
        let is_map_put = matches!(
            name,
            "hashMapPut" | "aura.lang.std.Collections.hashMapPut"
        );
        if is_map_put {
            let key = args.get(1).cloned().unwrap_or(Value::Null);
            let val = args.get(2).cloned().unwrap_or(Value::Null);
            return match args.first() {
                Some(Value::Ref(h)) => {
                    let h = *h;
                    if self.heap.is_map(h) {
                        self.heap.map_set(h, key, val);
                        Some(Value::Ref(h))
                    } else {
                        None
                    }
                }
                _ => None,
            };
        }

        let is_set = matches!(
            name,
            "set" | "listSet" | "aura.lang.std.Collections.set" | "aura.lang.std.Collections.listSet"
        );
        if !is_set {
            return None;
        }
        let idx = args.get(1).map(|v| v.as_int()).unwrap_or(-1);
        let val = args.get(2).cloned().unwrap_or(Value::Null);
        match args.first() {
            Some(Value::Ref(h)) => {
                let h = *h;
                if self.heap.is_map(h) {
                    // `m["k"] = v` → Map 键写入
                    let key = args.get(1).cloned().unwrap_or(Value::Null);
                    self.heap.map_set(h, key, val);
                } else if idx >= 0 {
                    // `l[i] = v` / `l.set(i, v)` → 列表下标写入
                    self.heap.list_set(h, idx as usize, val);
                }
                Some(Value::Ref(h))
            }
            Some(Value::List(items)) => {
                let mut items = items.clone();
                if idx >= 0 && (idx as usize) < items.len() {
                    items[idx as usize] = val;
                }
                Some(Value::List(items))
            }
            _ => Some(Value::Null),
        }
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

        // ── 原生调用追踪（诊断，默认关闭）─────────────────────────────────
        // `AURA_VM_TRACE_CALL=<子串>`：打印名字含该子串的原生调用。
        //
        // 用途：定位「同一 std 方法在 main 作用域正常、在 object 方法体内失效」
        // 这类**作用域相关的调用名差异** —— 前端在不同作用域可能生成不同的 callee
        // 名，导致 `stdlib_func_map`（嵌入 .auc 的 Aura 实现）命中或落空；落空时
        // 退回 Rust native，而两套实现**句柄语义不兼容**（Aura 侧是裸内存，
        // Rust 侧是注册表下标），表现为句柄为 0 / 读到垃圾。
        // 实测：`StringBuilder.create` 在 main 里派发到 Aura 实现（句柄=裸内存），
        // 在 object 方法体内落到 Rust native（返回下标 0）→ `appendN` 立刻 `sb==0`。
        if trace_call_enabled(&native.name) {
            eprintln!(
                "[vm] native-call: {} param_count={} argc={}",
                native.name,
                param_count,
                args.len()
            );
        }

        // `throw expr` 由 HIR 降级为 `__throw(expr)`：在原生派发前拦截，
        // 展开到最近的异常处理器（`try/catch`），无处理器则报未捕获异常。
        if native.name == "__throw" {
            let v = args.into_iter().next().unwrap_or(Value::Null);
            return self.raise(v);
        }

        // `__new_exception(type_name, message)`：创建异常对象
        if native.name == "__new_exception" {
            let type_name = match args.first() {
                Some(Value::Str(s)) => s.to_string(),
                _ => "Exception".to_string(),
            };
            let msg = match args.get(1) {
                Some(Value::Str(s)) => s.to_string(),
                _ => String::new(),
            };
            let obj = self.create_exception_object(&type_name, &msg);
            self.frames[top].stack.push(obj);
            return Ok(());
        }

        // `Process.exit(code)`：记录退出码并干净地停止 VM（由 CLI 设置进程退出码）。
        if is_exit_native(&native.name) {
            let code = crate::std::std_process::last_int_arg(&args).unwrap_or(0) as i32;
            self.request_exit(code);
            return Ok(());
        }

        // ── 可变集合工厂 → 堆对象 ──
        //
        // VM 有两套列表/映射表示：
        //   * 堆表示 `Value::Ref(h)` —— `LIST_PUSH` / `LIST_POP` / `MAP_SET` 等
        //     **原位**指令只支持它（见 `Instr::ListPush` 的 `_ => {}` 分支）；
        //   * 值表示 `Value::List(Vec<Value>)` —— 纯函数式 native（`listOf`、
        //     `split` 等）产出，只适合读操作。
        //
        // `mutableListOf()` 若走 native 会拿到**值表示**，于是 `l.add(x)` 的
        // `LIST_PUSH` 静默丢弃（列表永远为空）→ `l.size` 恒为 0、`l[0]` 恒为
        // null、`l.set(i, v)` 无效果。photon 后端 `RegisterAllocator` 的
        // `liveAtEnd.set(j, liveOut)` 正踩在此处（表现为 VM 段错误或静默空值）。
        //
        // 因此在解释器层把可变工厂改写为堆对象，后续原位指令即可正常工作。
        let is_mut_list = matches!(
            native.name.as_str(),
            "mutableListOf"
                | "arrayListOf"
                | "aura.lang.std.Collections.mutableListOf"
                | "aura.lang.std.Collections.arrayListOf"
        );
        if is_mut_list {
            let h = self.heap.alloc_list(args.len().max(1));
            for a in args {
                self.heap.list_push(h, a);
            }
            self.frames[top].stack.push(Value::Ref(h));
            return Ok(());
        }
        let is_mut_map = matches!(
            native.name.as_str(),
            "mutableMapOf" | "aura.lang.std.Collections.mutableMapOf"
        );
        if is_mut_map {
            let h = self.heap.alloc_map();
            self.frames[top].stack.push(Value::Ref(h));
            return Ok(());
        }

        // 原位集合写入（`l.set(i, v)` / `arr[i] = v` → `Collections.set`）：
        // 堆列表必须原地写，否则调用方丢弃返回值后毫无效果。
        if let Some(v) = self.try_inline_coll_set(&native.name, &args) {
            self.frames[top].stack.push(v);
            return Ok(());
        }

        // Phase D: Aura 编译的标准库函数版本优先。
        // 始终先查 `stdlib_func_map`（嵌入 .auc 的 Aura 编译函数），
        // 仅当未找到或函数为 native 声明（is_native=true，无 Aura 实现体）时才回退到 Rust native。
        // 这确保纯逻辑模块（Math.abs / String.contains / Collections.listOf …）
        // 使用 Aura 实现，而 libm / syscall 等 native 声明仍走 Rust 实现。
        let std_lookup = self.find_stdlib_func(&native.name, param_count).filter(|&(idx, _)| {
            let func = &self.module.funcs[idx];
            !func.is_native
        });
        if let Some((std_func_idx, needs_self)) = std_lookup {
            eprintln!(
                "[vm] stdlib-aura: {} → Aura compiled func #{} (self={})",
                native.name, std_func_idx, needs_self
            );

            if needs_self {
                // 注入 singleton 对象作为 self 参数
                let object_name = self.extract_object_name(&native.name);
                let self_value = if let Some(obj_name) = object_name {
                    self.singletons.get(obj_name).cloned().unwrap_or_else(|| {
                        eprintln!(
                            "[vm] stdlib-aura: singleton '{}' not found, using Null for {}",
                            obj_name, native.name
                        );
                        Value::Null
                    })
                } else {
                    eprintln!(
                        "[vm] stdlib-aura: cannot extract object name from {}, using Null",
                        native.name
                    );
                    Value::Null
                };
                let mut new_args = Vec::with_capacity(args.len() + 1);
                new_args.push(self_value);
                new_args.extend(args);
                self.push_frame(std_func_idx, new_args)?;
                return Ok(());
            } else {
                // 参数完全匹配，直接调用
                self.push_frame(std_func_idx, args)?;
                return Ok(());
            }
        }

        let result = if let Some(v) = self.intercept_object_native(&native.name, &args)? {
            v
        } else if let Some(f) = self.natives.get(&native.name) {
            f(&args)
        } else if let Some(f) = self.natives.resolve_c_function(&native.name) {
            f(&args)
        } else if native.ffi_abi == FfiAbi::Aura {
            // extern interface: AOT 直调
            self.call_aot_ffi(&native, &args).unwrap_or_else(|| {
                eprintln!("[vm] AOT interface call failed: `{}`", native.name);
                Value::Int(0)
            })
        } else {
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
            // P8.4: 尝试静态链接 — 使用库句柄解析 C 函数
            match static_call_c_with_lib(
                &native.name,
                &args,
                lib_handle,
                &native.param_types,
                native.ret_type,
            ) {
                Some(v) => v,
                None => {
                    warn_unlinked_once(&native.name, &args);
                    Value::Int(0)
                }
            }
        };
        self.frames[top].stack.push(result);
        Ok(())
    }

    /// 原生 / FFI 函数调用（带实际参数个数，用于变长函数）
    fn do_call_native_args(&mut self, top: usize, idx: usize, argc: usize) -> Result<(), VmError> {
        if idx >= self.module.natives.len() {
            return Err(VmError::Runtime(format!(
                "call to undefined native #{}",
                idx
            )));
        }
        let native = self.module.natives[idx].clone();
        // 使用实际参数个数而非声明的 param_count
        let mut args = self.pop_n(top, argc)?;

        // 原生调用追踪（诊断，默认关闭；见 `trace_call_enabled` 的说明）
        if trace_call_enabled(&native.name) {
            eprintln!(
                "[vm] native-call(args): {} param_count={} argc={}",
                native.name, native.param_count, argc
            );
        }

        // 对象单例方法（object 上的 `Class.method(...)`）会在调用点注入 self 作为首参，
        // 使 `argc = 声明参数个数 + 1`。原生实现按声明签名取值，此处剥离注入的 self，
        // 否则 `FileSystem.writeText(path, content)` 之类会整体错位（写出到空路径等）。
        // 注意：变长原生（`println`/`listOf` 等）声明 param_count 为 0，不受此规则影响。
        let mut eff_argc = argc;
        if native.param_count as usize >= 1
            && argc == native.param_count as usize + 1
            && !args.is_empty()
        {
            args.remove(0);
            eff_argc = argc - 1;
        }

        // 同 `do_call_native`：`throw` 走异常展开路径
        if native.name == "__throw" {
            let v = args.into_iter().next().unwrap_or(Value::Null);
            return self.raise(v);
        }

        // 同 `do_call_native`：`__new_exception` 创建异常对象
        if native.name == "__new_exception" {
            let type_name = match args.first() {
                Some(Value::Str(s)) => s.to_string(),
                _ => "Exception".to_string(),
            };
            let msg = match args.get(1) {
                Some(Value::Str(s)) => s.to_string(),
                _ => String::new(),
            };
            let obj = self.create_exception_object(&type_name, &msg);
            self.frames[top].stack.push(obj);
            return Ok(());
        }

        // 同 `do_call_native`：`Process.exit(code)` 请求退出
        if is_exit_native(&native.name) {
            let code = crate::std::std_process::last_int_arg(&args).unwrap_or(0) as i32;
            self.request_exit(code);
            return Ok(());
        }

        // ── 可变集合工厂 → 堆对象（与 `do_call_native` 中的同名逻辑保持一致）──
        //
        // `mutableListOf()` 等由字节码以 `CALL_NATIVE_ARGS` 调用，走的是本函数。
        // 若交给纯函数式 native，会得到**值表示** `Value::List`，而 `LIST_PUSH`
        // 只对**堆表示** `Value::Ref` 做原位操作 → `l.add(x)` 静默失效
        // （`l.size` 恒 0、`l[0]` 恒 null、`l.set(i, v)` 无效果）。
        let is_mut_list = matches!(
            native.name.as_str(),
            "mutableListOf"
                | "arrayListOf"
                | "aura.lang.std.Collections.mutableListOf"
                | "aura.lang.std.Collections.arrayListOf"
        );
        if is_mut_list {
            let h = self.heap.alloc_list(args.len().max(1));
            for a in args {
                self.heap.list_push(h, a);
            }
            self.frames[top].stack.push(Value::Ref(h));
            return Ok(());
        }
        let is_mut_map = matches!(
            native.name.as_str(),
            "mutableMapOf" | "aura.lang.std.Collections.mutableMapOf"
        );
        if is_mut_map {
            let h = self.heap.alloc_map();
            self.frames[top].stack.push(Value::Ref(h));
            return Ok(());
        }

        // 原位集合写入（`l.set(i, v)` / `arr[i] = v` → `Collections.set`）：
        // 堆列表必须原地写，否则调用方丢弃返回值后毫无效果。
        if let Some(v) = self.try_inline_coll_set(&native.name, &args) {
            self.frames[top].stack.push(v);
            return Ok(());
        }

        // Phase D: Aura 编译的标准库函数版本优先（同 do_call_native，始终先查 stdlib_func_map）
        let args_std_lookup = self.find_stdlib_func(&native.name, eff_argc).filter(|&(idx, _)| {
            let func = &self.module.funcs[idx];
            !func.is_native
        });
        if let Some((std_func_idx, needs_self)) = args_std_lookup {
            eprintln!(
                "[vm] stdlib-aura: {} (argc={}) → Aura compiled func #{} (self={})",
                native.name, eff_argc, std_func_idx, needs_self
            );

            if needs_self {
                let object_name = self.extract_object_name(&native.name);
                let self_value = if let Some(obj_name) = object_name {
                    self.singletons.get(obj_name).cloned().unwrap_or(Value::Null)
                } else {
                    Value::Null
                };
                let mut new_args = Vec::with_capacity(argc + 1);
                new_args.push(self_value);
                new_args.extend(args);
                self.push_frame(std_func_idx, new_args)?;
                return Ok(());
            } else {
                self.push_frame(std_func_idx, args)?;
                return Ok(());
            }
        }

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

        let result = if let Some(v) = self.intercept_object_native(&native.name, &args)? {
            v
        } else if let Some(f) = self.natives.get(&native.name) {
            f(&args)
        } else if let Some(f) = self.natives.resolve_c_function(&native.name) {
            f(&args)
        } else if native.ffi_abi == FfiAbi::Aura {
            // extern interface: AOT 直调
            self.call_aot_ffi(&native, &args).unwrap_or_else(|| {
                eprintln!("[vm] AOT interface call failed: `{}`", native.name);
                Value::Int(0)
            })
        } else {
            // P8.4: 尝试静态链接 — 使用库句柄解析 C 函数
            match static_call_c_with_lib(
                &native.name,
                &args,
                lib_handle,
                &native.param_types,
                native.ret_type,
            ) {
                Some(v) => v,
                None => {
                    warn_unlinked_once(&native.name, &args);
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
            format!("libs/{}.dll", lib_name),
            format!("{}.dll", lib_name),
        ];

        for path in &paths {
            let wide: Vec<u16> =
                std::ffi::OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();
            unsafe {
                let handle = LoadLibraryW(wide.as_ptr());
                if handle != 0 {
                    self.loaded_libs.insert(lib_name.to_string(), handle);
                    eprintln!("[vm] Loaded library: {} ({})", lib_name, path);
                    return;
                }
            }
        }
        eprintln!("[vm] Cannot load library: {}", lib_name);
    }

    #[cfg(unix)]
    fn ensure_lib_loaded(&mut self, lib_name: &str) {
        if self.loaded_libs.contains_key(lib_name) {
            return;
        }
        let paths = [
            lib_name.to_string(),
            format!("libs/lib{}.so", lib_name),
            format!("lib{}.so", lib_name),
        ];

        for path in &paths {
            let c_path = std::ffi::CString::new(path.clone()).unwrap();
            unsafe {
                let handle = dlopen(c_path.as_ptr(), 2); // RTLD_NOW = 2
                if !handle.is_null() {
                    self.loaded_libs.insert(lib_name.to_string(), handle);
                    eprintln!("[vm] Loaded library: {} ({})", lib_name, path);
                    return;
                }
            }
        }
        eprintln!("[vm] Cannot load library: {}", lib_name);
    }

    /// extern interface: 确保 AOT 库已加载
    fn ensure_aot_lib_loaded(&mut self, lib_name: &str) -> Option<u32> {
        if let Some(&id) = self.aot_module_map.get(lib_name) {
            return Some(id);
        }
        // 与 C FFI 一致的库查找逻辑：依次尝试多种路径
        let lib_path = if std::path::Path::new(lib_name).exists() {
            lib_name.to_string()
        } else {
            let paths = Self::candidate_lib_paths(lib_name);
            paths.into_iter().find(|p| std::path::Path::new(p).exists())?
        };

        #[cfg(all(feature = "llvm", feature = "dynamic-ffi"))]
        {
            match self.aot_runtime.load_shared_library(&lib_path) {
                Ok(module_id) => {
                    eprintln!(
                        "[vm] AOT interface library loaded: {} ({}) → module_id={}",
                        lib_name, lib_path, module_id
                    );
                    self.aot_module_map.insert(lib_name.to_string(), module_id);
                    Some(module_id)
                }
                Err(e) => {
                    eprintln!(
                        "[vm] AOT interface library load failed: {} ({})",
                        lib_path, e
                    );
                    None
                }
            }
        }
        #[cfg(not(all(feature = "llvm", feature = "dynamic-ffi")))]
        {
            eprintln!("[vm] AOT interface library loading requires llvm+dynamic-ffi feature");
            None
        }
    }

    /// 生成库候选路径列表（平台感知）
    fn candidate_lib_paths(lib_name: &str) -> Vec<String> {
        let mut paths = Vec::new();
        #[cfg(windows)]
        {
            paths.push(format!("libs/{}.dll", lib_name));
            paths.push(format!("{}.dll", lib_name));
            paths.push(format!("target/build/libs/{}/{}.dll", lib_name, lib_name));
        }
        #[cfg(unix)]
        {
            paths.push(format!("libs/lib{}.so", lib_name));
            paths.push(format!("lib{}.so", lib_name));
            paths.push(format!("libs/lib{}.dylib", lib_name));
            paths.push(format!("lib{}.dylib", lib_name));
            paths.push(format!("target/build/libs/{}/lib{}.so", lib_name, lib_name));
        }
        paths
    }

    /// extern interface: AOT 直调
    fn call_aot_ffi(
        &mut self,
        native: &crate::codegen::opcode::BytecodeNative,
        args: &[Value],
    ) -> Option<Value> {
        let lib_name = native.ffi_lib.as_deref()?;
        let module_id = self.ensure_aot_lib_loaded(lib_name)?;

        // 从函数名提取实际函数名（"Utils.add" → "add"）
        let func_name = native.name.split('.').last().unwrap_or(&native.name);
        let func_idx = self.aot_runtime.lookup_func_idx(module_id, func_name)?;

        eprintln!(
            "[vm] AOT interface call: {} (module={}, func_idx={})",
            native.name, module_id, func_idx
        );

        let jit_args: Vec<crate::vm::abi::JitValue> =
            args.iter().map(crate::vm::abi::JitValue::from_value).collect();
        unsafe { self.aot_runtime.call_func(module_id, func_idx, &jit_args).ok() }
            .map(crate::vm::abi::JitValue::to_value)
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
        // 可变参数函数（如 listOf）：实际参数数可能少于声明的 param_count。
        // 此时只弹栈实际存在的参数数，避免栈下溢。
        let actual_n = n.min(stack.len());
        if actual_n == 0 && n > 0 {
            let func_name = self.module.funcs[self.frames[top].func].name.clone();
            let ip = self.frames[top].ip;
            return Err(VmError::Runtime(format!(
                "operand stack underflow (need {} args) in `{}` at ip={}",
                n, func_name, ip
            )));
        }
        let start = stack.len() - actual_n;
        Ok(stack.split_off(start))
    }
}

/// P8.4: 静态链接 C 函数调用（无类型信息，回退到硬编码匹配）
fn static_call_c(name: &str, args: &[Value]) -> Option<Value> {
    static_call_c_with_lib(name, args, None, &[], 6) // ret_type=6 (void) 仅作占位
}

/// P9: 使用指定库句柄调用 C 函数
///
/// `param_types` 和 `ret_type` 是 CType ID（u8），用于确定参数和返回值的实际类型。
fn static_call_c_with_lib(
    name: &str,
    args: &[Value],
    lib_handle: Option<usize>,
    param_types: &[u8],
    ret_type: u8,
) -> Option<Value> {
    use crate::vm::ffi::{CFuncInfo, CFuncPtr, CType, resolve_static_symbol};

    // 如果有库句柄，从库中解析符号；否则从当前进程解析
    // 对于 Aura FFI 函数，先尝试原始名称，再尝试 aura_c_ 前缀
    let addr = if let Some(handle) = lib_handle {
        // 先尝试原始名称（兼容 sqlura_* 等非 Aura 函数）
        let direct = resolve_symbol_in_lib(handle, name);
        if direct.is_some() {
            direct
        } else {
            // 尝试 aura_c_ 前缀（Aura C ABI 包装函数）
            let prefixed = format!("aura_c_{}", name);
            resolve_symbol_in_lib(handle, &prefixed)
        }
    } else {
        let direct = resolve_static_symbol(name);
        if direct.is_some() {
            direct
        } else {
            let prefixed = format!("aura_c_{}", name);
            resolve_static_symbol(&prefixed)
        }
    }?;

    let ptr: CFuncPtr = unsafe { std::mem::transmute(addr) };

    // 根据传入的类型信息确定参数类型和返回类型
    let param_types_c: Vec<CType> = if param_types.is_empty() {
        // 无类型信息时回退到硬编码匹配（兼容旧版 .auc 文件）
        match name {
            "sqlura_version" => vec![],
            "sqlura_open" => vec![CType::CString],
            "sqlura_close" => vec![CType::Ptr],
            "sqlura_exec" => vec![
                CType::Ptr,
                CType::CString,
            ],
            "sqlura_free_string" => vec![CType::CString],
            "sqlura_error" => vec![CType::Ptr],
            _ => vec![CType::Int64; args.len().min(8)],
        }
    } else {
        param_types
            .iter()
            .map(|&id| match id {
                0 => CType::Int32,
                1 => CType::Int64,
                2 => CType::Float64,
                3 => CType::Bool,
                4 => CType::CString,
                5 => CType::Ptr,
                _ => CType::Int64,
            })
            .collect()
    };

    let return_type = if param_types.is_empty() {
        // 无类型信息时回退到硬编码匹配
        match name {
            "sqlura_version" => CType::CString,
            "sqlura_open" => CType::Ptr,
            "sqlura_close" => CType::Int64,
            "sqlura_exec" => CType::CString,
            "sqlura_free_string" => CType::Void,
            "sqlura_error" => CType::CString,
            _ => CType::Int64,
        }
    } else {
        match ret_type {
            0 => CType::Int32,
            1 => CType::Int64,
            2 => CType::Float64,
            3 => CType::Bool,
            4 => CType::CString,
            5 => CType::Ptr,
            6 => CType::Void,
            _ => CType::Int64,
        }
    };

    // P9: 创建 CString 对象保持生命周期
    let mut c_strings: Vec<std::ffi::CString> = Vec::new();

    // P9: 根据参数类型打包参数
    let c_args: [i64; 8] = [
        args.first()
            .map(|v| {
                let ty = param_types_c.first().unwrap_or(&CType::Int64);
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
                let ty = param_types_c.get(1).unwrap_or(&CType::Int64);
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
                let ty = param_types_c.get(2).unwrap_or(&CType::Int64);
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
                let ty = param_types_c.get(3).unwrap_or(&CType::Int64);
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

/// `AURA_VM_TRACE_CALL=<子串>`：是否追踪该原生调用（诊断用，默认关闭）。
///
/// 见 `do_call_native` 里的说明：用于定位作用域相关的「调用名差异」——
/// 前端在 main 与 object 方法体内可能给同一 std 方法生成不同的 callee 名，
/// 前者命中 `stdlib_func_map`（嵌入的 Aura 实现），后者落空退回 Rust native，
/// 而两套实现的句柄语义不兼容。
fn trace_call_enabled(name: &str) -> bool {
    use std::sync::OnceLock;
    static FILTER: OnceLock<Option<String>> = OnceLock::new();
    match FILTER.get_or_init(|| std::env::var("AURA_VM_TRACE_CALL").ok()) {
        Some(sub) if !sub.is_empty() => name.contains(sub.as_str()),
        _ => false,
    }
}

/// `==` 的 ABI 适配：**任一侧是指针**（`Ptr`）时按地址比较。
///
/// 项目 ABI 是「指针即地址」：`Memory.alloc` / `Allocator.malloc` 返回 `Ptr`，
/// 而既有代码普遍用 `addr == 0` / `addr != 0` 判空。`Value` 的派生 `PartialEq`
/// 会让 `Ptr(0) == Int(0)` 为 false，因此这里对含指针的比较统一取地址比数值；
/// 其余情况保持派生语义（`Int`/`Str`/`Null` 等互不相等的行为不变）。
fn value_eq_abi(a: Value, b: Value) -> Value {
    if matches!(a, Value::Ptr(_)) || matches!(b, Value::Ptr(_)) {
        return Value::Bool(a.as_int() == b.as_int());
    }
    Value::Bool(a == b)
}

/// `!=` 的 ABI 适配（`value_eq_abi` 取反）。
fn value_ne_abi(a: Value, b: Value) -> Value {
    match value_eq_abi(a, b) {
        Value::Bool(v) => Value::Bool(!v),
        _ => Value::Bool(false),
    }
}

/// 集合 / 字符串的内建成员访问（`size` / `length` / `first` / `last` / `isEmpty`）。
///
/// 背景：`GetField` 指令只携带字段名的 FNV-1a 哈希（见 `codegen::emit::field_index`），
/// 且 sema 表达式类型通道对裸标识符不可靠。这里在**运行期**按哈希识别内建成员，
/// 使 `xs.size` / `s.first` / `m.isEmpty` 等直接可用。
///
/// 安全性：`Value::Ref`（类实例）走堆字段路径，不经过本函数，因此类字段语义不受影响；
/// 只有 List / Map / Str 这三种“无字段”的内联值会被改写，原先一律返回 Null。
fn builtin_member(obj: Value, field: u16) -> Value {
    use crate::codegen::emit::field_index;
    let is_size = field == field_index("size") || field == field_index("length");
    let is_first = field == field_index("first");
    let is_last = field == field_index("last");
    let is_empty = field == field_index("isEmpty");

    match obj {
        Value::List(items) => {
            if is_size {
                return Value::Int(items.len() as i64);
            }
            if is_empty {
                return Value::Bool(items.is_empty());
            }
            if is_first {
                return items.first().cloned().unwrap_or(Value::Null);
            }
            if is_last {
                return items.last().cloned().unwrap_or(Value::Null);
            }
            Value::Null
        }
        Value::Map(map) => {
            if is_size {
                return Value::Int(map.len() as i64);
            }
            if is_empty {
                return Value::Bool(map.is_empty());
            }
            Value::Null
        }
        Value::Str(s) => {
            if is_size {
                return Value::Int(s.chars().count() as i64);
            }
            if is_empty {
                return Value::Bool(s.is_empty());
            }
            if is_first {
                return s.chars().next().map(|c| Value::str_(c.to_string())).unwrap_or(Value::Null);
            }
            if is_last {
                return s.chars().last().map(|c| Value::str_(c.to_string())).unwrap_or(Value::Null);
            }
            Value::Null
        }
        _ => Value::Null,
    }
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
    // ── 指针算术（"指针即地址" ABI）──
    // 与 `value_eq_abi` 同源：含 `Ptr` 的 `+` 一律按**地址**做整数运算，结果取 `Int`。
    //
    // 此前含 `Ptr` 的加法会掉进下面的 Float 分支（`as_float()` 对 `Ptr` 返回 0.0）：
    //   `ptr + 0` → `Float(0.0)`，`ptr + 8` → `Float(8.0)`
    // 于是「指针 + 偏移」退化成小浮点地址。实测后果（2026-09-23）：
    //   `CString("hello")` 产出 `Ptr`，经 `Long` 形参进入嵌入 std 的
    //   `StringBuilder.append` 后，`Memory.read(text + n)` 读的是地址 0
    //   → NUL 扫描恒得 0 → 发射缓冲恒空 → 自举编译器打印不出字节码。
    // 注意与 `Memory.alloc` 的差异：后者特意返回 `Int`（见 `native_memory_alloc`），
    // 所以「句柄 + 偏移」一直是正常的；只有 `CString` 这条 `Ptr` 路径会踩到 Float 分支。
    if matches!(a, Value::Ptr(_)) || matches!(b, Value::Ptr(_)) {
        return Value::Int(a.as_int().wrapping_add(b.as_int()));
    }
    if both_int(&a, &b) {
        Value::Int(a.as_int().wrapping_add(b.as_int()))
    } else {
        Value::Float(a.as_float() + b.as_float())
    }
}

fn bin_sub(a: Value, b: Value) -> Value {
    // 指针差值（同 `bin_add` 的 ABI 适配：按地址做整数运算）
    if matches!(a, Value::Ptr(_)) || matches!(b, Value::Ptr(_)) {
        return Value::Int(a.as_int().wrapping_sub(b.as_int()));
    }
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
