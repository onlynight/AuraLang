//! Bootstrap 运行时（Layer 0）：协程与最小 GC。
//!
//! - **协程**：`coroutine_yield` 由 VM 的 `Yield` 指令实现——协程入口
//!   函数执行到 `Yield` 时暂停并让出值（[`Step::Yielded`]），
//!   `resume` 恢复执行并把恢复值交付给 `Yield` 表达式；
//! - **GC**：最小 mark-sweep——登记分配对象、维护根集合，
//!   `collect` 清除未标记对象（对 bootstrap 层而言直接释放内存）。

use std::collections::{HashMap, HashSet};

use super::Trap;
use super::memory;
use super::vm_core::{BytecodeModule, FfiCache, Step, Value, Vm};

// ---------------------------------------------------------------------------
// 协程
// ---------------------------------------------------------------------------

/// 协程状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoroutineState {
    /// 已创建，尚未执行
    Fresh,
    /// 在 `coroutine_yield` 处暂停
    Suspended,
    /// 执行完成
    Finished,
}

/// 协程句柄：绑定一个 VM 实例与入口帧。
///
/// 用法：
/// ```ignore
/// let mut co = Coroutine::new(&module, &mut ffi, "gen", &[])?;
/// let step = co.resume(Value::Null)?;      // 首次恢复即启动
/// // step == Step::Yielded(Value::Int(0))
/// let step = co.resume(Value::Null)?;      // 继续执行
/// ```
pub struct Coroutine<'m> {
    vm: Vm<'m>,
    state: CoroutineState,
}

impl<'m> Coroutine<'m> {
    /// 创建协程：压入入口帧但暂不执行。
    pub fn new(
        module: &'m BytecodeModule,
        ffi: &'m mut FfiCache,
        entry: &str,
        args: &[Value],
    ) -> Result<Self, Trap> {
        let mut vm = Vm::new(module, ffi);
        vm.enter(entry, args)?;
        Ok(Self {
            vm,
            state: CoroutineState::Fresh,
        })
    }

    pub fn state(&self) -> CoroutineState {
        self.state
    }

    /// 恢复协程：`v` 交付给 `coroutine_yield` 表达式（首次恢复忽略）。
    pub fn resume(&mut self, v: Value) -> Result<Step, Trap> {
        if self.state == CoroutineState::Finished {
            return Err(Trap::new("coroutine already finished, cannot resume"));
        }
        // Fresh：入口帧已由 new() 压入，直接驱动执行（交付值无消费者）；
        // Suspended：值是 Yield 表达式的结果。
        let step = if self.state == CoroutineState::Fresh {
            self.vm.run_started()?
        } else {
            self.vm.resume(v)?
        };
        self.state = match &step {
            Step::Done(_) => CoroutineState::Finished,
            Step::Yielded(_) => CoroutineState::Suspended,
        };
        Ok(step)
    }
}

// ---------------------------------------------------------------------------
// 最小 GC（mark-sweep）
// ---------------------------------------------------------------------------

/// GC 统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcStats {
    /// 已执行的收集次数
    pub collections: u64,
    /// 本次/累计释放对象数
    pub freed_objects: usize,
    /// 本次/累计释放字节数
    pub freed_bytes: usize,
    /// 收集后存活对象数
    pub live_objects: usize,
}

/// 登记的堆对象。
#[derive(Clone, Copy, Debug)]
struct GcObject {
    addr: usize,
    size: usize,
    marked: bool,
}

/// 最小 mark-sweep 堆：登记 bootstrap 分配的对象，按根集合回收。
///
/// bootstrap 层的对象不含内部指针（值是立即量或 Rc），因此标记阶段
/// 只需把**根集合直接引用**的对象标活；不可达对象在清扫阶段释放。
pub struct GcHeap {
    objects: HashMap<usize, GcObject>,
    roots: HashSet<usize>,
    collections: u64,
    total_freed: usize,
    total_freed_bytes: usize,
}

impl GcHeap {
    pub fn new() -> Self {
        Self {
            objects: HashMap::new(),
            roots: HashSet::new(),
            collections: 0,
            total_freed: 0,
            total_freed_bytes: 0,
        }
    }

    /// 在 GC 管理下分配（malloc + 登记）。
    pub fn alloc(&mut self, size: usize) -> Result<Value, Trap> {
        let ptr = memory::malloc(size)?;
        self.objects.insert(
            ptr as usize,
            GcObject {
                addr: ptr as usize,
                size,
                marked: false,
            },
        );
        Ok(Value::Ptr(ptr as usize))
    }

    /// 加入根集合（对象保持存活）。
    pub fn add_root(&mut self, v: &Value) -> Result<(), Trap> {
        match v {
            Value::Ptr(p) if *p != 0 => {
                if !self.objects.contains_key(p) {
                    return Err(Trap::new("gc: root points to unregistered object"));
                }
                self.roots.insert(*p);
                Ok(())
            }
            _ => Err(Trap::new("gc: root must be a Pointer")),
        }
    }

    /// 移出根集合。
    pub fn remove_root(&mut self, v: &Value) {
        if let Value::Ptr(p) = v {
            self.roots.remove(p);
        }
    }

    /// 执行一次 mark-sweep 收集，返回统计。
    pub fn collect(&mut self) -> GcStats {
        // mark：根直接引用的对象标活
        for p in &self.roots {
            if let Some(o) = self.objects.get_mut(p) {
                o.marked = true;
            }
        }
        // sweep：未标记对象释放并移除登记
        let dead: Vec<GcObject> = self.objects.values().filter(|o| !o.marked).copied().collect();
        let mut freed_objects = 0usize;
        let mut freed_bytes = 0usize;
        for o in &dead {
            // SAFETY: 地址来自 memory::malloc 且未被释放过
            unsafe { memory::free(o.addr as *mut u8) };
            self.objects.remove(&o.addr);
            freed_objects += 1;
            freed_bytes += o.size;
        }
        // 清除标记位
        for o in self.objects.values_mut() {
            o.marked = false;
        }

        self.collections += 1;
        self.total_freed += freed_objects;
        self.total_freed_bytes += freed_bytes;
        GcStats {
            collections: self.collections,
            freed_objects,
            freed_bytes,
            live_objects: self.objects.len(),
        }
    }

    pub fn live_objects(&self) -> usize {
        self.objects.len()
    }

    pub fn total_freed(&self) -> (usize, usize) {
        (self.total_freed, self.total_freed_bytes)
    }
}

impl Default for GcHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GcHeap {
    fn drop(&mut self) {
        // 释放所有未回收的登记对象
        for o in self.objects.values() {
            // SAFETY: 地址来自 memory::malloc 且未被释放过
            unsafe { memory::free(o.addr as *mut u8) };
        }
    }
}
