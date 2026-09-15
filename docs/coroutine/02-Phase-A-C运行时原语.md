# Phase A: C 运行时原语

## 目标

在 `aura_syscalls.c` 中实现跨平台（POSIX/Windows）线程和同步原语封装，为 VM/AOT/JIT 提供底层 C 接口。

## 文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `compiler/src/std/cffi/aura_syscalls.c` | 修改 | 新增线程/同步原语 |
| `compiler/src/std/cffi/aura_syscalls.h` | 修改 | 新增声明 |
| `compiler/src/std/cffi/aura_std_cffi.c` | 修改 | 并发函数对接真实实现 |

## 设计

### 1. 线程原语

```c
// ── 线程 ──
typedef struct {
    void (*fn)(void *arg);
    void *arg;
    void *result;
} AuraThreadArg;

#ifdef _WIN32
typedef unsigned int AuraThreadId;
typedef CRITICAL_SECTION AuraMutex;
typedef SRWLOCK AuraRwLock;
typedef CONDITION_VARIABLE AuraCondvar;
#else
typedef pthread_t AuraThreadId;
typedef pthread_mutex_t AuraMutex;
typedef pthread_rwlock_t AuraRwLock;
typedef pthread_cond_t AuraCondvar;
#endif

AuraThreadId aura_thread_create(void (*fn)(void *), void *arg);
int aura_thread_join(AuraThreadId tid);
void aura_thread_exit(void *result);
void aura_thread_sleep(int64_t ms);
int64_t aura_thread_id(AuraThreadId tid);
void aura_thread_park(void);
void aura_thread_unpark(int64_t tid);

// 线程本地存储
void aura_tls_key_create(void);
int64_t aura_tls_get(void);
void aura_tls_set(int64_t val);
```

### 2. Mutex 互斥锁

```c
AuraMutex *aura_mutex_new(void);
void aura_mutex_lock(AuraMutex *m);
void aura_mutex_unlock(AuraMutex *m);
int aura_mutex_trylock(AuraMutex *m);  // 1=成功, 0=失败
void aura_mutex_free(AuraMutex *m);
```

### 3. RwLock 读写锁

```c
AuraRwLock *aura_rwlock_new(void);
void aura_rwlock_read_lock(AuraRwLock *m);
void aura_rwlock_write_lock(AuraRwLock *m);
void aura_rwlock_read_unlock(AuraRwLock *m);
void aura_rwlock_write_unlock(AuraRwLock *m);
void aura_rwlock_free(AuraRwLock *m);
```

### 4. 条件变量

```c
AuraCondvar *aura_condvar_new(void);
void aura_condvar_wait(AuraCondvar *cv, AuraMutex *mutex);
void aura_condvar_signal(AuraCondvar *cv);
void aura_condvar_broadcast(AuraCondvar *cv);
void aura_condvar_wait_timeout(AuraCondvar *cv, AuraMutex *mutex, int64_t ms);
void aura_condvar_free(AuraCondvar *cv);
```

### 5. 原子操作

```c
int64_t aura_atomic_load(volatile int64_t *addr);
void aura_atomic_store(volatile int64_t *addr, int64_t val);
int64_t aura_atomic_add(volatile int64_t *addr, int64_t delta);
int64_t aura_atomic_sub(volatile int64_t *addr, int64_t delta);
int64_t aura_atomic_cas(volatile int64_t *addr, int64_t expected, int64_t desired);
int aura_atomic_cas_bool(volatile int64_t *addr, int64_t expected, int64_t desired);
int64_t aura_atomic_rmw_add(volatile int64_t *addr, int64_t delta);
int64_t aura_atomic_rmw_sub(volatile int64_t *addr, int64_t delta);
```

### 6. Barrier 屏障

```c
void *aura_barrier_new(int64_t count);
int64_t aura_barrier_wait(void *barrier);
void aura_barrier_free(void *barrier);
```

## POSIX 实现要点

```c
// ── 线程 ──
AuraThreadId aura_thread_create(void (*fn)(void *), void *arg) {
    pthread_t tid;
    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_create(&tid, &attr, fn, arg);
    pthread_attr_destroy(&attr);
    return tid;
}

// ── Mutex ──
AuraMutex *aura_mutex_new(void) {
    AuraMutex *m = (AuraMutex *)malloc(sizeof(AuraMutex));
    pthread_mutex_init(m, NULL);
    return m;
}

void aura_mutex_lock(AuraMutex *m) { pthread_mutex_lock(m); }
void aura_mutex_unlock(AuraMutex *m) { pthread_mutex_unlock(m); }
int aura_mutex_trylock(AuraMutex *m) { return pthread_mutex_trylock(m) == 0; }

// ── 原子操作 ──
int64_t aura_atomic_add(volatile int64_t *addr, int64_t delta) {
    return __sync_fetch_and_add(addr, delta);
}

int64_t aura_atomic_cas(volatile int64_t *addr, int64_t expected, int64_t desired) {
    return __sync_val_compare_and_swap(addr, expected, desired);
}
```

## Windows 实现要点

```c
// ── 线程 ──
typedef unsigned int AuraThreadId;

AuraThreadId aura_thread_create(void (*fn)(void *), void *arg) {
    HANDLE h = CreateThread(NULL, 0, 
        (LPTHREAD_START_ROUTINE)fn, arg, 0, NULL);
    return (AuraThreadId)h;
}

// ── Mutex ──
typedef CRITICAL_SECTION AuraMutex;

AuraMutex *aura_mutex_new(void) {
    AuraMutex *m = (AuraMutex *)malloc(sizeof(AuraMutex));
    InitializeCriticalSection(m);
    return m;
}

void aura_mutex_lock(AuraMutex *m) { EnterCriticalSection(m); }
void aura_mutex_unlock(AuraMutex *m) { LeaveCriticalSection(m); }

// ── 原子操作 ──
int64_t aura_atomic_add(volatile int64_t *addr, int64_t delta) {
    return InterlockedAdd64((volatile LONG64*)addr, delta);
}

int64_t aura_atomic_cas(volatile int64_t *addr, int64_t expected, int64_t desired) {
    return InterlockedCompareExchange64((volatile LONG64*)addr, desired, expected);
}
```

## 测试计划

### 单元测试（C 层）

```c
// 新增测试文件: compiler/tests/aura_syscalls_c_tests.c
// 通过 cargo test 的 run 模式调用

TEST(thread_create_join) { ... }
TEST(mutex_lock_unlock) { ... }
TEST(mutex_trylock) { ... }
TEST(rwlock_read_write) { ... }
TEST(condvar_signal_broadcast) { ... }
TEST(atomic_add_sub) { ... }
TEST(atomic_cas) { ... }
TEST(barrier) { ... }
TEST(tls) { ... }
TEST(multi_thread_atomic) { ... }
TEST(thread_safety_stress) { ... }
```

### Rust 集成测试

```rust
// compiler/tests/concurrent_native_tests.rs
#[test]
fn test_thread_create_join_native() { ... }
#[test]
fn test_mutex_native() { ... }
#[test]
fn test_atomic_native() { ... }
```

## 依赖关系

```
Phase A 无前置依赖，是后续所有 Phase 的基础
```

## 风险

| 风险 | 缓解 |
|------|------|
| Windows 线程 ID 处理 | HANDLE 转 int64_t，统一接口 |
| 跨平台原子操作差异 | 使用编译器内建（__sync_* / Interlocked*） |
| CriticalSection 无 trylock | 使用 SRWLOCK 或封装 |
| pthread_t 在不同平台大小不同 | 统一用 int64_t 作为句柄 |
