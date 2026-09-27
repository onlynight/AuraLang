//! Phase B: 并发原生函数（Thread / Mutex / Atomic / RwLock / Condvar / Barrier）
//!
//! 通过 Rust 标准库原语（std::sync / std::thread / std::sync::atomic）
//! 在 VM 层面暴露为原生函数，不依赖 C FFI 库。
//!
//! 调用约定：所有函数接收 `&[Value]` 参数，返回 `Value`。

use crate::vm::value::Value;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

// ─────────────────────────────────────────────────────────────────────────────
// Mutex 句柄管理
// ─────────────────────────────────────────────────────────────────────────────
// RawMutex 实现（使用 AtomicBool + Condvar）
// ─────────────────────────────────────────────────────────────────────────────

struct RawMutex {
    locked: AtomicBool,
    cv: std::sync::Condvar,
    guard: std::sync::Mutex<()>,
}

impl RawMutex {
    fn new() -> Self {
        RawMutex {
            locked: AtomicBool::new(false),
            cv: std::sync::Condvar::new(),
            guard: std::sync::Mutex::new(()),
        }
    }

    fn lock(&self) {
        loop {
            if self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
            let _guard = self.guard.lock().unwrap();
            self.cv.wait(_guard).unwrap();
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
        self.cv.notify_all();
    }

    fn trylock(&self) -> bool {
        self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }
}

static RAW_MUTEX_REGISTRY: Mutex<Vec<Option<RawMutex>>> = Mutex::new(Vec::new());

fn raw_mutex_alloc() -> usize {
    let mut reg = RAW_MUTEX_REGISTRY.lock().unwrap();
    if let Some(pos) = reg.iter().position(|s| s.is_none()) {
        reg[pos] = Some(RawMutex::new());
        pos
    } else {
        reg.push(Some(RawMutex::new()));
        reg.len() - 1
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Atomic 值存储
// ─────────────────────────────────────────────────────────────────────────────

static ATOMIC_REGISTRY: Mutex<Vec<Option<AtomicI64>>> = Mutex::new(Vec::new());

/// 分配一个新的原子值，返回 ID
fn atomic_alloc(initial: i64) -> usize {
    let mut reg = ATOMIC_REGISTRY.lock().unwrap();
    if let Some(pos) = reg.iter().position(|s| s.is_none()) {
        reg[pos] = Some(AtomicI64::new(initial));
        pos
    } else {
        reg.push(Some(AtomicI64::new(initial)));
        reg.len() - 1
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RwLock 实现
// ─────────────────────────────────────────────────────────────────────────────

struct RawRwLock {
    readers: AtomicI64,
    writers: AtomicI64,
    cv: std::sync::Condvar,
    guard: std::sync::Mutex<()>,
}

impl RawRwLock {
    fn new() -> Self {
        RawRwLock {
            readers: AtomicI64::new(0),
            writers: AtomicI64::new(0),
            cv: std::sync::Condvar::new(),
            guard: std::sync::Mutex::new(()),
        }
    }

    fn read_lock(&self) {
        loop {
            if self.writers.load(Ordering::Acquire) == 0 {
                self.readers.fetch_add(1, Ordering::AcqRel);
                return;
            }
            let _guard = self.guard.lock().unwrap();
            self.cv.wait(_guard).unwrap();
        }
    }

    fn read_unlock(&self) {
        self.readers.fetch_sub(1, Ordering::Release);
        if self.readers.load(Ordering::Acquire) == 0 {
            self.cv.notify_all();
        }
    }

    fn write_lock(&self) {
        loop {
            if self.readers.load(Ordering::Acquire) == 0
                && self.writers.load(Ordering::Acquire) == 0
            {
                self.writers.store(1, Ordering::Release);
                return;
            }
            let _guard = self.guard.lock().unwrap();
            self.cv.wait(_guard).unwrap();
        }
    }

    fn write_unlock(&self) {
        self.writers.store(0, Ordering::Release);
        self.cv.notify_all();
    }
}

static RWLOCK_REGISTRY: Mutex<Vec<Option<RawRwLock>>> = Mutex::new(Vec::new());

fn rwlock_alloc() -> usize {
    let mut reg = RWLOCK_REGISTRY.lock().unwrap();
    if let Some(pos) = reg.iter().position(|s| s.is_none()) {
        reg[pos] = Some(RawRwLock::new());
        pos
    } else {
        reg.push(Some(RawRwLock::new()));
        reg.len() - 1
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Condvar 实现
// ─────────────────────────────────────────────────────────────────────────────

struct RawCondvar {
    cv: std::sync::Condvar,
    guard: std::sync::Mutex<()>,
}

impl RawCondvar {
    fn new() -> Self {
        RawCondvar {
            cv: std::sync::Condvar::new(),
            guard: std::sync::Mutex::new(()),
        }
    }

    fn signal(&self) {
        self.cv.notify_one();
    }

    fn broadcast(&self) {
        self.cv.notify_all();
    }

    fn wait(&self, mutex_id: usize) {
        // 获取关联的 Mutex 的 guard
        let reg = RAW_MUTEX_REGISTRY.lock().unwrap();
        if let Some(Some(m)) = reg.get(mutex_id) {
            let _guard = m.guard.lock().unwrap();
            self.cv.wait(_guard).unwrap();
        }
    }
}

static CONDVAR_REGISTRY: Mutex<Vec<Option<RawCondvar>>> = Mutex::new(Vec::new());

fn condvar_alloc() -> usize {
    let mut reg = CONDVAR_REGISTRY.lock().unwrap();
    if let Some(pos) = reg.iter().position(|s| s.is_none()) {
        reg[pos] = Some(RawCondvar::new());
        pos
    } else {
        reg.push(Some(RawCondvar::new()));
        reg.len() - 1
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Barrier 实现
// ─────────────────────────────────────────────────────────────────────────────

struct RawBarrier {
    count: i64,
    remaining: AtomicI64,
    cv: std::sync::Condvar,
    guard: std::sync::Mutex<()>,
}

impl RawBarrier {
    fn new(count: i64) -> Self {
        RawBarrier {
            count,
            remaining: AtomicI64::new(count),
            cv: std::sync::Condvar::new(),
            guard: std::sync::Mutex::new(()),
        }
    }

    fn wait(&self) -> i64 {
        let old = self.remaining.fetch_sub(1, Ordering::AcqRel);
        if old == 1 {
            let _guard = self.guard.lock().unwrap();
            self.cv.notify_all();
            0
        } else {
            loop {
                let mut guard = self.guard.lock().unwrap();
                guard = self.cv.wait(guard).unwrap();
                if self.remaining.load(Ordering::Acquire) == 0 {
                    return old - 1;
                }
            }
        }
    }
}

static BARRIER_REGISTRY: Mutex<Vec<Option<RawBarrier>>> = Mutex::new(Vec::new());

fn barrier_alloc(count: i64) -> usize {
    let mut reg = BARRIER_REGISTRY.lock().unwrap();
    if let Some(pos) = reg.iter().position(|s| s.is_none()) {
        reg[pos] = Some(RawBarrier::new(count));
        pos
    } else {
        reg.push(Some(RawBarrier::new(count)));
        reg.len() - 1
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Semaphore 实现
// ─────────────────────────────────────────────────────────────────────────────

/// 计数信号量（许可计数 + 条件变量）。
///
/// 注册表存 `Arc`，取用时克隆后再释放注册表锁，避免阻塞等待期间持锁造成死锁。
struct RawSemaphore {
    count: std::sync::Mutex<i64>,
    cv: std::sync::Condvar,
}

impl RawSemaphore {
    fn new(permits: i64) -> Self {
        RawSemaphore {
            count: std::sync::Mutex::new(if permits < 0 { 0 } else { permits }),
            cv: std::sync::Condvar::new(),
        }
    }

    fn acquire(&self) {
        let mut c = self.count.lock().unwrap();
        while *c <= 0 {
            c = self.cv.wait(c).unwrap();
        }
        *c -= 1;
    }

    fn try_acquire(&self) -> bool {
        let mut c = self.count.lock().unwrap();
        if *c <= 0 {
            false
        } else {
            *c -= 1;
            true
        }
    }

    fn release(&self) {
        let mut c = self.count.lock().unwrap();
        *c += 1;
        self.cv.notify_one();
    }

    fn count(&self) -> i64 {
        *self.count.lock().unwrap()
    }
}

static SEMAPHORE_REGISTRY: Mutex<Vec<Option<std::sync::Arc<RawSemaphore>>>> =
    Mutex::new(Vec::new());

fn semaphore_alloc(permits: i64) -> usize {
    let mut reg = SEMAPHORE_REGISTRY.lock().unwrap();
    let slot = std::sync::Arc::new(RawSemaphore::new(permits));
    if let Some(pos) = reg.iter().position(|s| s.is_none()) {
        reg[pos] = Some(slot);
        pos
    } else {
        reg.push(Some(slot));
        reg.len() - 1
    }
}

/// 取出信号量句柄（克隆 Arc 后释放注册表锁）。
fn semaphore_get(id: usize) -> Option<std::sync::Arc<RawSemaphore>> {
    SEMAPHORE_REGISTRY.lock().unwrap().get(id).and_then(|s| s.clone())
}

// ─────────────────────────────────────────────────────────────────────────────
// 原生函数实现
// ─────────────────────────────────────────────────────────────────────────────

/// Thread.sleep(ms) → Unit：休眠指定毫秒
pub fn native_thread_sleep(args: &[Value]) -> Value {
    let ms = args.first().map(|v| v.as_int()).unwrap_or(0);
    if ms > 0 {
        std::thread::sleep(Duration::from_millis(ms as u64));
    }
    Value::Null
}

/// Thread.id() → Int：获取当前线程 ID
pub fn native_thread_id(_args: &[Value]) -> Value {
    // 返回当前线程的标识（使用线程局部存储，简化为 1）
    Value::Int(1)
}

/// Thread.parallelism() → Int：获取可用并行度
pub fn native_thread_parallelism(_args: &[Value]) -> Value {
    let cores = std::thread::available_parallelism().map(|n| n.get() as i64).unwrap_or(1);
    Value::Int(cores)
}

/// Thread.availableCores() → Int：获取 CPU 核心数
pub fn native_thread_available_cores(_args: &[Value]) -> Value {
    let cores = std::thread::available_parallelism().map(|n| n.get() as i64).unwrap_or(1);
    Value::Int(cores)
}

// ─────────────────────────────────────────────────────────────────────────────
// Thread 线程管理（OS 级线程调度）
// ─────────────────────────────────────────────────────────────────────────────

/// 线程注册表条目
struct ThreadEntry {
    handle: Option<JoinHandle<()>>,
    result_rx: mpsc::Receiver<i64>,
}

/// 全局线程注册表（线程 ID → 线程条目）
static THREAD_REGISTRY: std::sync::OnceLock<Mutex<std::collections::HashMap<i64, ThreadEntry>>> =
    std::sync::OnceLock::new();

/// 线程 ID 自增计数器
static NEXT_THREAD_ID: AtomicI64 = AtomicI64::new(1);

fn thread_registry() -> &'static Mutex<std::collections::HashMap<i64, ThreadEntry>> {
    THREAD_REGISTRY.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Thread.spawn(fnId, arg) → Int：创建新 OS 线程执行 Aura 函数
///
/// 通过 `std::thread::spawn` 创建真正的 OS 线程。新线程拥有独立的 VM 实例，
/// 执行指定函数（fnId）并传入单个参数（arg）。函数返回值通过 mpsc 通道
/// 回传，`Thread.join` 阻塞等待并返回该结果。
///
/// - 参数 `fnId`：VM 函数索引（字节码函数表中的位置）
/// - 参数 `arg`：传递给目标函数的单个整数值
/// - 返回：新线程 ID（`>0` 成功，`0` 失败）
pub fn native_thread_spawn(args: &[Value]) -> Value {
    let fn_id = args.first().map(|v| v.as_int()).unwrap_or(0);
    let arg = args.get(1).map(|v| v.as_int()).unwrap_or(0);

    // 获取当前 VM 的模块克隆（新线程需要独立 VM）
    let module = match crate::vm::native::current_vm_module_clone() {
        Some(m) => m,
        None => return Value::Int(0),
    };

    // 创建结果通道（线程 → 调用者）
    let (result_tx, result_rx) = mpsc::channel();

    // 创建新 OS 线程
    let handle = thread::spawn(move || {
        let opts = crate::vm::VmOptions::default();
        let mut vm = match crate::vm::Vm::new(&module, opts) {
            Ok(vm) => vm,
            Err(e) => {
                eprintln!("[thread] VM creation failed: {}", e);
                let _ = result_tx.send(0);
                return;
            }
        };

        // 设置当前线程的 VM 引用（供原生函数访问）
        crate::vm::native::set_vm_ref(&mut vm as *mut _ as *mut ());

        // 执行目标函数
        let result = match vm.run_function(fn_id as usize, vec![Value::Int(arg)]) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[thread] run_function failed: {}", e);
                Value::Int(0)
            }
        };

        // 清理 VM 引用
        crate::vm::native::clear_vm_ref();

        // 发送结果（转换为 i64）
        let _ = result_tx.send(result.as_int());
    });

    // 注册线程
    let tid = NEXT_THREAD_ID.fetch_add(1, Ordering::SeqCst);
    {
        let mut reg = thread_registry().lock().unwrap();
        reg.insert(
            tid,
            ThreadEntry {
                handle: Some(handle),
                result_rx,
            },
        );
    }

    Value::Int(tid)
}

/// Thread.join(threadId) → Int：等待线程完成并返回结果
///
/// 阻塞等待指定线程结束，返回该线程执行函数的返回值（转换为 i64）。
/// 线程完成后从注册表中移除，后续调用返回 0。
///
/// - 参数 `threadId`：`Thread.spawn` 返回的线程 ID
/// - 返回：线程的返回值（`0` 表示线程不存在或失败）
pub fn native_thread_join(args: &[Value]) -> Value {
    let tid = args.first().map(|v| v.as_int()).unwrap_or(0);

    // 取出线程条目（避免持锁等待）
    let entry = {
        let mut reg = thread_registry().lock().unwrap();
        reg.remove(&tid)
    };

    if let Some(entry) = entry {
        // 等待线程结束
        if let Some(handle) = entry.handle {
            if let Err(e) = handle.join() {
                eprintln!("[thread] thread panicked: {:?}", e);
            }
        }

        // 接收结果
        if let Ok(result) = entry.result_rx.recv() {
            return Value::Int(result);
        }
    }

    Value::Int(0)
}

/// Mutex.new() → Int：创建互斥锁
pub fn native_mutex_new(_args: &[Value]) -> Value {
    Value::Int(raw_mutex_alloc() as i64)
}

/// Mutex.lock(id) → Unit：加锁
pub fn native_mutex_lock(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = RAW_MUTEX_REGISTRY.lock().unwrap();
        if let Some(Some(m)) = reg.get(id) {
            m.lock();
        }
    }
    Value::Null
}

/// Mutex.unlock(id) → Unit：解锁
pub fn native_mutex_unlock(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = RAW_MUTEX_REGISTRY.lock().unwrap();
        if let Some(Some(m)) = reg.get(id) {
            m.unlock();
        }
    }
    Value::Null
}

/// Mutex.tryLock(id) → Boolean：尝试加锁
pub fn native_mutex_trylock(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = RAW_MUTEX_REGISTRY.lock().unwrap();
        if let Some(Some(m)) = reg.get(id) { Value::Bool(m.trylock()) } else { Value::Bool(false) }
    } else {
        Value::Bool(false)
    }
}

/// Mutex.destroy(id) → Unit：销毁互斥锁
pub fn native_mutex_destroy(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let mut reg = RAW_MUTEX_REGISTRY.lock().unwrap();
        if let Some(slot) = reg.get_mut(id) {
            *slot = None;
        }
    }
    Value::Null
}

/// Atomic.new(initial) → Int：创建原子计数器
pub fn native_atomic_new(args: &[Value]) -> Value {
    let initial = args.first().map(|v| v.as_int()).unwrap_or(0);
    Value::Int(atomic_alloc(initial) as i64)
}

/// Atomic.load(id) → Int：原子读取
pub fn native_atomic_load(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = ATOMIC_REGISTRY.lock().unwrap();
        if let Some(Some(a)) = reg.get(id) {
            Value::Int(a.load(Ordering::SeqCst))
        } else {
            Value::Int(0)
        }
    } else {
        Value::Int(0)
    }
}

/// Atomic.store(id, val) → Unit：原子写入
pub fn native_atomic_store(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let id = args[0].as_int() as usize;
        let val = args[1].as_int();
        let reg = ATOMIC_REGISTRY.lock().unwrap();
        if let Some(Some(a)) = reg.get(id) {
            a.store(val, Ordering::SeqCst);
        }
    }
    Value::Null
}

/// Atomic.add(id, delta) → Int：原子加法，返回新值
pub fn native_atomic_add(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let id = args[0].as_int() as usize;
        let delta = args[1].as_int();
        let reg = ATOMIC_REGISTRY.lock().unwrap();
        if let Some(Some(a)) = reg.get(id) {
            Value::Int(a.fetch_add(delta, Ordering::SeqCst) + delta)
        } else {
            Value::Int(delta)
        }
    } else {
        Value::Int(0)
    }
}

/// Atomic.sub(id, delta) → Int：原子减法，返回新值
pub fn native_atomic_sub(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let id = args[0].as_int() as usize;
        let delta = args[1].as_int();
        let reg = ATOMIC_REGISTRY.lock().unwrap();
        if let Some(Some(a)) = reg.get(id) {
            Value::Int(a.fetch_sub(delta, Ordering::SeqCst) - delta)
        } else {
            Value::Int(-delta)
        }
    } else {
        Value::Int(0)
    }
}

/// Atomic.cas(id, expected, desired) → Boolean：比较交换
pub fn native_atomic_cas(args: &[Value]) -> Value {
    if args.len() >= 3 {
        let id = args[0].as_int() as usize;
        let expected = args[1].as_int();
        let desired = args[2].as_int();
        let reg = ATOMIC_REGISTRY.lock().unwrap();
        if let Some(Some(a)) = reg.get(id) {
            Value::Bool(
                a.compare_exchange(expected, desired, Ordering::SeqCst, Ordering::SeqCst).is_ok(),
            )
        } else {
            Value::Bool(false)
        }
    } else {
        Value::Bool(false)
    }
}

/// RwLock.new() → Int：创建读写锁
pub fn native_rwlock_new(_args: &[Value]) -> Value {
    Value::Int(rwlock_alloc() as i64)
}

/// RwLock.readLock(id) → Unit：获取读锁
pub fn native_rwlock_read_lock(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = RWLOCK_REGISTRY.lock().unwrap();
        if let Some(Some(rw)) = reg.get(id) {
            rw.read_lock();
        }
    }
    Value::Null
}

/// RwLock.writeLock(id) → Unit：获取写锁
pub fn native_rwlock_write_lock(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = RWLOCK_REGISTRY.lock().unwrap();
        if let Some(Some(rw)) = reg.get(id) {
            rw.write_lock();
        }
    }
    Value::Null
}

/// RwLock.readUnlock(id) → Unit：释放读锁
pub fn native_rwlock_read_unlock(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = RWLOCK_REGISTRY.lock().unwrap();
        if let Some(Some(rw)) = reg.get(id) {
            rw.read_unlock();
        }
    }
    Value::Null
}

/// RwLock.writeUnlock(id) → Unit：释放写锁
pub fn native_rwlock_write_unlock(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = RWLOCK_REGISTRY.lock().unwrap();
        if let Some(Some(rw)) = reg.get(id) {
            rw.write_unlock();
        }
    }
    Value::Null
}

/// RwLock.destroy(id) → Unit：销毁读写锁
pub fn native_rwlock_destroy(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let mut reg = RWLOCK_REGISTRY.lock().unwrap();
        if let Some(slot) = reg.get_mut(id) {
            *slot = None;
        }
    }
    Value::Null
}

/// Condvar.new() → Int：创建条件变量
pub fn native_condvar_new(_args: &[Value]) -> Value {
    Value::Int(condvar_alloc() as i64)
}

/// Condvar.wait(condvarId, mutexId) → Unit：等待条件变量
pub fn native_condvar_wait(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let cv_id = args[0].as_int() as usize;
        let mutex_id = args[1].as_int() as usize;
        let reg = CONDVAR_REGISTRY.lock().unwrap();
        if let Some(Some(cv)) = reg.get(cv_id) {
            cv.wait(mutex_id);
        }
    }
    Value::Null
}

/// Condvar.signal(id) → Unit：唤醒一个
pub fn native_condvar_signal(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = CONDVAR_REGISTRY.lock().unwrap();
        if let Some(Some(cv)) = reg.get(id) {
            cv.signal();
        }
    }
    Value::Null
}

/// Condvar.broadcast(id) → Unit：唤醒所有
pub fn native_condvar_broadcast(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = CONDVAR_REGISTRY.lock().unwrap();
        if let Some(Some(cv)) = reg.get(id) {
            cv.broadcast();
        }
    }
    Value::Null
}

/// Condvar.destroy(id) → Unit：销毁条件变量
pub fn native_condvar_destroy(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let mut reg = CONDVAR_REGISTRY.lock().unwrap();
        if let Some(slot) = reg.get_mut(id) {
            *slot = None;
        }
    }
    Value::Null
}

/// Barrier.new(count) → Int：创建屏障
pub fn native_barrier_new(args: &[Value]) -> Value {
    let count = args.first().map(|v| v.as_int()).unwrap_or(0);
    Value::Int(barrier_alloc(count) as i64)
}

/// Barrier.wait(id) → Int：等待屏障
pub fn native_barrier_wait(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let reg = BARRIER_REGISTRY.lock().unwrap();
        if let Some(Some(b)) = reg.get(id) { Value::Int(b.wait()) } else { Value::Int(0) }
    } else {
        Value::Int(0)
    }
}

/// Barrier.destroy(id) → Unit：销毁屏障
pub fn native_barrier_destroy(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let mut reg = BARRIER_REGISTRY.lock().unwrap();
        if let Some(slot) = reg.get_mut(id) {
            *slot = None;
        }
    }
    Value::Null
}

/// Semaphore.new(permits) → Int：创建信号量
pub fn native_semaphore_new(args: &[Value]) -> Value {
    let permits = args.first().map(|v| v.as_int()).unwrap_or(0);
    Value::Int(semaphore_alloc(permits) as i64)
}

/// Semaphore.acquire(id) → Unit：获取一个许可（无可用许可时阻塞）
pub fn native_semaphore_acquire(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        if let Some(sem) = semaphore_get(v.as_int() as usize) {
            sem.acquire();
        }
    }
    Value::Null
}

/// Semaphore.tryAcquire(id) → Boolean：尝试获取一个许可（非阻塞）
pub fn native_semaphore_try_acquire(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        if let Some(sem) = semaphore_get(v.as_int() as usize) {
            return Value::Bool(sem.try_acquire());
        }
    }
    Value::Bool(false)
}

/// Semaphore.release(id) → Unit：归还一个许可
pub fn native_semaphore_release(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        if let Some(sem) = semaphore_get(v.as_int() as usize) {
            sem.release();
        }
    }
    Value::Null
}

/// Semaphore.count(id) → Int：当前可用许可数
pub fn native_semaphore_count(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        if let Some(sem) = semaphore_get(v.as_int() as usize) {
            return Value::Int(sem.count());
        }
    }
    Value::Int(0)
}

/// Semaphore.destroy(id) → Unit：销毁信号量
pub fn native_semaphore_destroy(args: &[Value]) -> Value {
    if let Some(v) = args.first() {
        let id = v.as_int() as usize;
        let mut reg = SEMAPHORE_REGISTRY.lock().unwrap();
        if let Some(slot) = reg.get_mut(id) {
            *slot = None;
        }
    }
    Value::Null
}

// ─────────────────────────────────────────────────────────────────────────────
// Future（基于 Thread 的异步执行）
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::{Mutex as StdMutex, OnceLock};

/// Future 注册表（全局）
static FUTURE_REGISTRY: OnceLock<StdMutex<std::collections::HashMap<i64, FutureEntry>>> =
    OnceLock::new();

static FUTURE_ID_COUNTER: AtomicI64 = AtomicI64::new(0);

/// Future 条目：记录线程 ID 和完成状态
struct FutureEntry {
    thread_id: i64,
    done: AtomicBool,
    result: StdMutex<i64>,
}

fn future_registry() -> &'static StdMutex<std::collections::HashMap<i64, FutureEntry>> {
    FUTURE_REGISTRY.get_or_init(|| StdMutex::new(std::collections::HashMap::new()))
}

fn native_future_spawn(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    let fn_id = args[0].as_int();
    let arg = args[1].as_int();

    // 调用 Thread.spawn 创建线程
    let thread_id = native_thread_spawn(&[
        Value::Int(fn_id),
        Value::Int(arg),
    ]);
    if let Value::Int(tid) = thread_id {
        let future_id = FUTURE_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        let entry = FutureEntry {
            thread_id: tid,
            done: AtomicBool::new(false),
            result: StdMutex::new(0),
        };
        let mut reg = future_registry().lock().unwrap();
        reg.insert(future_id, entry);
        Value::Int(future_id)
    } else {
        Value::Null
    }
}

fn native_future_await(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Null;
    }
    let future_id = args[0].as_int();

    // 提取 thread_id 并释放锁（避免持锁等待）
    let thread_id = {
        let reg = future_registry().lock().unwrap();
        match reg.get(&future_id) {
            Some(e) => e.thread_id,
            None => return Value::Null,
        }
    };

    // 等待线程完成（无锁阻塞）
    let result = native_thread_join(&[Value::Int(thread_id)]);

    // 更新 Future 状态
    {
        let mut reg = future_registry().lock().unwrap();
        if let Some(entry) = reg.get_mut(&future_id) {
            entry.done.store(true, Ordering::SeqCst);
            let mut r = entry.result.lock().unwrap();
            if let Value::Int(v) = result {
                *r = v;
            }
        }
    }

    // 返回结果
    let reg = future_registry().lock().unwrap();
    if let Some(entry) = reg.get(&future_id) {
        Value::Int(*entry.result.lock().unwrap())
    } else {
        Value::Null
    }
}

fn native_future_is_done(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Bool(false);
    }
    let future_id = args[0].as_int();
    let reg = future_registry().lock().unwrap();
    match reg.get(&future_id) {
        Some(e) => Value::Bool(e.done.load(Ordering::SeqCst)),
        None => Value::Bool(false),
    }
}

fn native_future_all(args: &[Value]) -> Value {
    let items = match args.first() {
        Some(Value::List(items)) => items.clone(),
        _ => return Value::List(vec![]),
    };
    let mut results = Vec::new();
    for item in items {
        if let Value::Int(fid) = item {
            let result = native_future_await(&[Value::Int(fid)]);
            results.push(result);
        }
    }
    Value::List(results)
}

fn native_future_any(args: &[Value]) -> Value {
    let items = match args.first() {
        Some(Value::List(items)) => items.clone(),
        _ => return Value::Null,
    };
    for item in items {
        if let Value::Int(fid) = item {
            let result = native_future_await(&[Value::Int(fid)]);
            return result;
        }
    }
    Value::Null
}

fn native_future_cancel(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Null;
    }
    let future_id = args[0].as_int();
    let mut reg = future_registry().lock().unwrap();
    if let Some(mut entry) = reg.remove(&future_id) {
        entry.done.store(true, Ordering::SeqCst);
    }
    Value::Null
}

// ─────────────────────────────────────────────────────────────────────────────
// 注册函数
// ─────────────────────────────────────────────────────────────────────────────

/// 注册所有并发原生函数到 NativeRegistry
pub fn register_all(r: &mut crate::vm::NativeRegistry) {
    // Thread
    r.register("aura.lang.concurrent.Thread.spawn", native_thread_spawn);
    r.register("aura.lang.concurrent.Thread.join", native_thread_join);
    r.register("aura.lang.concurrent.Thread.sleep", native_thread_sleep);
    r.register("aura.lang.concurrent.Thread.id", native_thread_id);
    r.register(
        "aura.lang.concurrent.Thread.parallelism",
        native_thread_parallelism,
    );
    r.register(
        "aura.lang.concurrent.Thread.availableCores",
        native_thread_available_cores,
    );

    // Mutex
    r.register("aura.lang.concurrent.Mutex.new", native_mutex_new);
    r.register("aura.lang.concurrent.Mutex.lock", native_mutex_lock);
    r.register("aura.lang.concurrent.Mutex.unlock", native_mutex_unlock);
    r.register("aura.lang.concurrent.Mutex.tryLock", native_mutex_trylock);
    r.register("aura.lang.concurrent.Mutex.destroy", native_mutex_destroy);

    // Atomic
    r.register("aura.lang.concurrent.Atomic.new", native_atomic_new);
    r.register("aura.lang.concurrent.Atomic.load", native_atomic_load);
    r.register("aura.lang.concurrent.Atomic.store", native_atomic_store);
    r.register("aura.lang.concurrent.Atomic.add", native_atomic_add);
    r.register("aura.lang.concurrent.Atomic.sub", native_atomic_sub);
    r.register("aura.lang.concurrent.Atomic.cas", native_atomic_cas);

    // RwLock
    r.register("aura.lang.concurrent.RwLock.new", native_rwlock_new);
    r.register(
        "aura.lang.concurrent.RwLock.readLock",
        native_rwlock_read_lock,
    );
    r.register(
        "aura.lang.concurrent.RwLock.writeLock",
        native_rwlock_write_lock,
    );
    r.register(
        "aura.lang.concurrent.RwLock.readUnlock",
        native_rwlock_read_unlock,
    );
    r.register(
        "aura.lang.concurrent.RwLock.writeUnlock",
        native_rwlock_write_unlock,
    );
    r.register("aura.lang.concurrent.RwLock.destroy", native_rwlock_destroy);

    // Condvar
    r.register("aura.lang.concurrent.Condvar.new", native_condvar_new);
    r.register("aura.lang.concurrent.Condvar.wait", native_condvar_wait);
    r.register("aura.lang.concurrent.Condvar.signal", native_condvar_signal);
    r.register(
        "aura.lang.concurrent.Condvar.broadcast",
        native_condvar_broadcast,
    );
    r.register(
        "aura.lang.concurrent.Condvar.destroy",
        native_condvar_destroy,
    );

    // Barrier
    r.register("aura.lang.concurrent.Barrier.new", native_barrier_new);
    r.register("aura.lang.concurrent.Barrier.wait", native_barrier_wait);
    r.register(
        "aura.lang.concurrent.Barrier.destroy",
        native_barrier_destroy,
    );

    // Semaphore（纯 Aura 版本见 aura/lang/concurrent/Semaphore.aura；
    // 此处为运行库实现，供尚未启用嵌入路径的运行时回退）
    r.register("aura.lang.concurrent.Semaphore.new", native_semaphore_new);
    r.register(
        "aura.lang.concurrent.Semaphore.acquire",
        native_semaphore_acquire,
    );
    r.register(
        "aura.lang.concurrent.Semaphore.tryAcquire",
        native_semaphore_try_acquire,
    );
    r.register(
        "aura.lang.concurrent.Semaphore.release",
        native_semaphore_release,
    );
    r.register(
        "aura.lang.concurrent.Semaphore.count",
        native_semaphore_count,
    );
    r.register(
        "aura.lang.concurrent.Semaphore.destroy",
        native_semaphore_destroy,
    );

    // Future (wrapper around Thread)
    r.register("aura.lang.concurrent.Future.spawn", native_future_spawn);
    r.register("aura.lang.concurrent.Future.await", native_future_await);
    r.register("aura.lang.concurrent.Future.isDone", native_future_is_done);
    r.register("aura.lang.concurrent.Future.all", native_future_all);
    r.register("aura.lang.concurrent.Future.any", native_future_any);
    r.register("aura.lang.concurrent.Future.cancel", native_future_cancel);
}
