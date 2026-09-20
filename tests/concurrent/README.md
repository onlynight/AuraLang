# Aura 并发测试套件

## 概述

本目录包含 Aura 语言的并发库完整测试套件，覆盖所有同步原语：

| 文件 | 覆盖内容 | 测试数 |
|------|---------|--------|
| `thread_basics.aura` | Thread spawn/join/sleep/id/parallelism | 7 |
| `atomic_ops.aura` | Atomic counter/add/sub/cas/load/store | 7 |
| `mutex_shared.aura` | Mutex 保护共享状态、线程间复杂数据传递 | 7 |
| `rwlock_condvar.aura` | RwLock 多读一写 + Condvar 生产者-消费者 | 8 |
| `barrier_semaphore.aura` | Barrier 屏障同步 + Semaphore 资源限制 | 8 |
| `future_chain.aura` | Future spawn/await/isDone/cancel | 8 |
| `integration.aura` | 全部原语组合的复杂集成测试 | 6 |

## 构建与运行

### 单个测试

```bash
# 构建
aura build tests/concurrent/thread_basics.aura --aot \
  --output build/aot-bin/concurrent/thread_basics.exe

# 运行
build/aot-bin/concurrent/thread_basics.exe
```

### 全部测试

```bash
for test in thread_basics atomic_ops mutex_shared rwlock_condvar barrier_semaphore future_chain integration; do
  echo "=== $test ==="
  aura build tests/concurrent/$test.aura --aot \
    --output build/aot-bin/concurrent/$test.exe
  build/aot-bin/concurrent/$test.exe
  echo ""
done
```

## 测试约定

- `fun main(): Int` 返回 `0` 表示全部通过，非零表示失败
- 每个测试用例使用 `do { ... } while (false)` 包装，便于独立验证
- 输出末尾打印 `concurrent.<name> ok` 或 `concurrent.<name> FAIL`

## 并发原语参考

```
aura.lang.concurrent.Thread      # 线程管理
  .spawn(fn, arg) → Int          # 创建线程（AOT: 函数名→索引自动解析）
  .join(thread_id) → Int         # 阻塞等待并返回结果
  .sleep(ms)                     # 休眠
  .id() → Int                    # 当前线程 ID
  .parallelism() → Int           # 并行度
  .availableCores() → Int        # 可用核心数

aura.lang.concurrent.Atomic      # 原子操作
  .new(initial) → Int            # 创建原子变量
  .load(id) → Int                # 读取
  .store(id, value)              # 写入
  .add(id, delta) → Int          # 原子加
  .sub(id, delta) → Int          # 原子减
  .cas(id, exp, des) → Boolean   # 比较交换
  .destroy(id)                   # 销毁

aura.lang.concurrent.Mutex       # 互斥锁
  .new() → Int
  .lock(id)
  .unlock(id)
  .tryLock(id) → Boolean
  .destroy(id)

aura.lang.concurrent.RwLock      # 读写锁
  .new() → Int
  .readLock(id) / .readUnlock(id)
  .writeLock(id) / .writeUnlock(id)
  .destroy(id)

aura.lang.concurrent.Condvar     # 条件变量
  .new() → Int
  .wait(cv_id, mutex_id)
  .signal(cv_id)
  .broadcast(cv_id)
  .destroy(cv_id)

aura.lang.concurrent.Barrier     # 屏障同步
  .new(count) → Int
  .wait(barrier_id) → Int
  .destroy(id)

aura.lang.concurrent.Future      # 异步 Future
  .spawn(fn_id, arg) → Int
  .await(future_id) → Int
  .isDone(future_id) → Boolean
  .cancel(future_id)

aura.lang.concurrent.Semaphore   # 信号量
  .new(permits) → Int
  .acquire(id)
  .tryAcquire(id) → Boolean
  .release(id)
  .count(id) → Int
  .destroy(id)
```

## 线程间复杂数据传递

由于 `Thread.spawn(fn, arg)` 仅接受单个整数参数，线程间传递复杂数据采用以下模式：

1. **共享全局变量 + Mutex 保护** — 最通用的方式，任意数据结构通过全局变量 + 锁保护
2. **Atomic 无锁计数器** — 适合计数、累加等简单场景
3. **RwLock 多读一写** — 读多写少的共享配置/状态
4. **Condvar 条件变量** — 实现缓冲区、任务队列等需要等待的场景
5. **Semaphore 资源限制** — 控制并发线程数，保护有限资源

## 底层床架线程重构验证

本测试套件专门验证 Aura AOT 后端底层床架线程重构后的正确性：

- `Thread.spawn` 通过 trampoline 分派表实现函数引用 → 函数索引的编译时解析
- 每个非 native 函数生成 `i64 fn(i64)` 的 trampoline，适配 C 运行时 `thread_dispatch`
- 函数指针表 `__aura_fn_table` + 计数 `__aura_fn_count` 供运行时查表
- 原子操作通过 C 原子指令实现，无锁并发安全
- 所有同步原语通过 C FFI 调用底层实现，保证跨平台一致性
