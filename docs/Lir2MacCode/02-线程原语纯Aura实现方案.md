# 线程原语纯 Aura 实现方案

> **日期**: 2026-07-12
> **对象**: `ThreadOps`（`aura/core/aura/lang/native/thread/ThreadOps.aura`）
> **目标**: 消除对 `aura_syscalls.c`（pthread/Win32）的依赖，仅使用 syscall + inline asm
> **结论**: 可以完全实现，仅 `create()` 需要新的 `@native` 注解类型（trampoline 模式）

---

## 一、现状与问题

### 1.1 当前架构

```
ThreadOps (extern interface, Aura 侧声明)
  └─ create/join/sleepMs/currentId/cores
       │
       ▼
Emit.aura emitNativeWrapper
  └─ call @aura_thread_create(i64, i64)
       │
       ▼
aura_syscalls.c  ← 外部 C 依赖（1055 行）
  └─ pthread_create() → glibc → clone() syscall
```

### 1.2 问题

- `aura_syscalls.c` 是不可自举的外部 C 依赖
- 用户明确要求：不想再有外部依赖，自己解决线程问题
- 不使用 LLVM IR `declare`，不使用 libc/pthread

### 1.3 现有基础设施

编译器已经具备的能力：

| 能力 | 来源 | 用于线程？ |
|------|------|-----------|
| inline asm `syscall` | `@native(N)` | ✅ 所有 syscall 操作 |
| inline asm 指令 | `@native(asm = "...")` | ✅ TLS 设置、分支跳转 |
| 内存分配 | `Memory.alloc/mmap`（编译器内置） | ✅ 线程栈、TLS 描述符 |
| 原子操作 | `atomicrmw`（LLVM 指令） | ✅ 锁操作（已在用） |
| 函数分派表 | `__aura_fn_table[]`（编译器生成） | ✅ 线程入口查找 |
| 异常处理 | handler 栈（非 setjmp） | ✅ 已在用 |

---

## 二、各线程原语替代方案

### 2.1 ThreadOps.join(thread_id) — ✅ 直接 syscall

**现有**: `call @aura_thread_join(i64)` → C: `pthread_join()`

**替代**: 直接使用 `wait4()` syscall（Linux 61 / x86_64）

**x86_64 Linux**:
```
wait4(pid, status, options, rusage)  →  syscall 61
```

**aarch64 Linux**:
```
wait4(pid, status, options, rusage)  →  syscall 61
```

**实现**:
```aura
// 新增 syscall 声明（Syscalls.aura）
const val SYS_WAIT4: Int = 61  // 已存在

// ThreadOps.aura — extern interface 内新增
@native(SYS_WAIT4) fun wait4Syscall(pid: Int, status: Long, options: Int, rusage: Long): Int

// ThreadOps.aura — object 内实现 join
fun join(thread_id: Int): Int {
    return wait4Syscall(thread_id, 0, 0, 0)
}
```

**Windows 差异**: Windows 没有 `wait4` 等价 syscall。需使用 `NtWaitForSingleObject`（Nt syscall 20）。但 Windows 上的 `ThreadOps.create` 使用 `CreateThread`，返回句柄。`WaitForSingleObject` 也是 Nt syscall 20。

### 2.2 ThreadOps.sleepMs(ms) — ✅ 直接 syscall

**现有**: `call @aura_thread_sleep(i64)` → C: `nanosleep()`

**替代**: 直接使用 `nanosleep()` syscall（Linux 35 / x86_64）

**实现**:
```aura
// Syscalls.aura 新增
const val SYS_NANOSLEEP: Int = 35

// ThreadOps.aura — extern interface 内新增
@native(SYS_NANOSLEEP) fun nanosleepSyscall(req: Long, rem: Long): Long

// ThreadOps.aura — object 内实现 sleepMs
fun sleepMs(ms: Int): Unit {
    if (ms <= 0) { return }
    val tsAddr: Long = Memory.alloc(16)
    Memory.write64(tsAddr, (ms as Long) / 1000L)                // tv_sec
    Memory.write64(tsAddr + 8, (ms as Long) % 1000L * 1000000L) // tv_nsec
    nanosleepSyscall(tsAddr, 0)
    Memory.free(tsAddr)
}
```

**Windows 差异**: Windows 使用 `NtDelayExecution`（Nt syscall 8）。需传入 ` LARGE_INTEGER` 负值（相对时间）。

### 2.3 ThreadOps.currentId() — ✅ 直接 syscall

**现有**: `call @aura_thread_id()` → C: `pthread_self()`

**替代**: 直接使用 `gettid()` syscall（Linux 186 / x86_64）

**实现**:
```aura
// Syscalls.aura 新增
const val SYS_GETTID: Int = 186

// ThreadOps.aura — extern interface 内新增
@native(SYS_GETTID) fun gettidSyscall(): Int

// ThreadOps.aura — object 内实现 currentId
fun currentId(): Int {
    return gettidSyscall()
}
```

**Windows 差异**: Windows 使用 `GetCurrentThreadId()`（CRT 函数，非 syscall）。但可以通过 `NtCurrentTeb`（`gs:[0x18]` on x86_64）读取线程 ID。这是 inline asm：

```asm
mov rax, gs:[0x18]
mov rax, [rax]
ret
```

或 `mov rax, [gs:0x18]; ret`

### 2.4 ThreadOps.cores() — ✅ 直接 syscall

**现有**: `call @aura_thread_available_parallelism()` → C: `sysconf(_SC_NPROCESSORS_ONLN)`

**替代**: 使用 `sched_getaffinity()` syscall（Linux 204 / x86_64）+ 位计数

**实现**:
```aura
// Syscalls.aura 新增
const val SYS_SCHED_GETAFFINITY: Int = 204

// ThreadOps.aura — extern interface 内新增
@native(SYS_SCHED_GETAFFINITY) fun schedGetaffinity(pid: Int, cpusetsize: Long, mask: Long): Int

// ThreadOps.aura — object 内实现 cores
fun cores(): Int {
    val maskAddr: Long = Memory.alloc(128)  // 最多 1024 CPU
    val ret: Int = schedGetaffinity(0, 128, maskAddr)
    if (ret != 0) { Memory.free(maskAddr); return 1 }
    // 统计 mask 中置位的位数
    var count: Int = 0
    var i: Int = 0
    while (i < 128) {
        val b: Byte = Memory.read(maskAddr + i)
        // 统计 8 bit 中的 1 位数
        var tmp: Int = b as Int
        while (tmp != 0) {
            count = count + (tmp and 1)
            tmp = tmp shr 1
        }
        i = i + 1
    }
    Memory.free(maskAddr)
    return count
}
```

**Windows 差异**: Windows 使用 `GetSystemInfo`（CRT 函数）。可通过 `NtQuerySystemInformation`（Nt syscall 30）获取。

### 2.5 ThreadOps.create(fn_id, arg) — ⚠️ 需要 trampoline 模式

**这是唯一需要特殊处理的函数。**

#### 2.5.1 核心挑战

`clone()` syscall 创建的新线程**从父线程的返回地址继续执行**，不是从指定函数开始。

```
clone() 返回:
  - 父线程: 子线程的 TID（非零）
  - 子线程: 0
```

**但父线程的 IP 是 clone 调用之后的下一条指令**，子线程的 IP 也是同样的位置。所以子线程会继续执行父线程的代码！

我们需要在 clone 之后做分支：

```
clone() 调用
test rax, rax
jz .Lchild_trampoline    // 子线程（rax=0）
ret                      // 父线程返回 TID

.Lchild_trampoline:
; 子线程代码：
; 1. 设置 TLS
; 2. 从栈上读取 fn_id, arg
; 3. 从 __aura_fn_table 查找函数指针
; 4. 调用函数
; 5. exit_group
```

#### 2.5.2 所需 syscall

| syscall | 编号 (x86_64 Linux) | 用途 |
|---------|-------------------|------|
| `clone` | 56 | 创建新线程 |
| `mmap` | 9 | 分配线程栈（已在用） |
| `arch_prctl` | 158 | 设置子线程 TLS |
| `exit_group` | 231 | 子线程退出 |
| `set_robust_list` | 273 | 设置 robust futex 列表（可选） |

#### 2.5.3 x86_64 Linux 实现方案

**Step 1: 定义新的 `@native` 注解类型**

建议在编译器中增加 `@native(clone_trampoline)` 注解，指示编译器生成 clone+trampoline 内联汇编块。

或者，使用 `@native(asm = "...")` 手动编写整个序列。

**Step 2: 声明 clone syscall**

```aura
// Syscalls.aura 新增
const val SYS_CLONE: Int = 56

// clone flags（x86_64 Linux）
const val CLONE_VM: Int = 0x100
const val CLONE_FS: Int = 0x200
const val CLONE_FILES: Int = 0x400
const val CLONE_SIGHAND: Int = 0x00800000
const val CLONE_THREAD: Int = 0x00010000
const val CLONE_SETTLS: Int = 0x00080000
const val CLONE_PARENT_SETTID: Int = 0x1000
const val CLONE_CHILD_CLEARTID: Int = 0x200000

// Syscalls.aura 常量定义（仅 Syscalls.aura 有，arch 文件没有）
```

**Step 3: 编译器后端实现**

`@native(clone_trampoline)` 在 `Emit.aura` 中的处理逻辑：

```
emitNativeWrapper 分支:
  @native(clone_trampoline) → 生成 clone+branch+trampoline 内联汇编块
```

生成的 LLVM IR（x86_64 Linux）:

```llvm
define i64 @ThreadOps_create(i64 %fn_id, i64 %arg) {
  entry:
    ; === 1. 分配线程栈 ===
    %stack_addr = call i64 @mmap(i64 0, i64 1048576, i64 3, i64 0x22, i64 -1, i64 0)
    ; 1MB 栈，MAP_PRIVATE|MAP_ANONYMOUS

    ; === 2. 分配线程描述符 ===
    %desc = call i64 @malloc(i64 64)
    ; desc[0] = fn_id, desc[8] = arg, desc[16] = stack_top, desc[24] = tls_ptr

    ; === 3. 填充描述符 ===
    store i64 %fn_id, i64* (i64*) %desc
    store i64 %arg, i64* (i64* (i64*) %desc + 1)
    ; stack_top = stack_addr + 1048576 - 16（向下增长栈，16字节对齐）
    ; tls_ptr = desc（用作 TLS 描述符）

    ; === 4. clone + trampoline（内联汇编）===
    %result = call i64 asm sideeffect "
      .intel_syntax noprefix
      ; 准备 clone 参数
      mov rax, 56               ; clone syscall
      mov rdi, [clone_flags]    ; CLONE_VM|CLONE_FS|...|CLONE_SETTLS|...
      mov rsi, [stack_top]      ; 子线程栈顶
      mov rdx, [parent_tidptr]  ; 父线程写 TID 的位置
      mov r10, 0                ; child_tidptr = NULL
      mov r8, [tls_ptr]         ; TLS 指针
      syscall
      test rax, rax
      jz .Lchild_trampoline
      ret
      .Lchild_trampoline:
      ; === 子线程开始执行 ===
      ; 4a. 设置 TLS (arch_prctl ARCH_SET_FS)
      mov rax, 158              ; arch_prctl
      mov rdi, 0x1002           ; ARCH_SET_FS
      mov rsi, [tls_ptr]        ; TLS 描述符地址
      syscall
      ; 4b. 从描述符读取 fn_id 和 arg
      mov rdi, [desc]           ; fn_id
      mov rsi, [desc+8]         ; arg
      ; 4c. 查找函数指针并调用
      lea r8, [rip + fn_table_offset]
      mov rax, [r8 + rdi*8]     ; fn_ptr = __aura_fn_table[fn_id]
      mov rdx, rsi              ; 移动 arg 到 rdx（线程函数签名: fn(arg)）
      call rax
      ; 4d. 线程函数返回后退出
      mov rax, 231              ; exit_group
      mov rdi, 0
      syscall
    ", "=r,{rdi},{rsi},{rdx},{r10},{r8},{r11}"(...)

    ret i64 %result
}
```

#### 2.5.4 实现复杂度评估

| 子任务 | 复杂度 | 说明 |
|--------|--------|------|
| mmap 分配栈 | 🟢 低 | 已有 `Memory.mmap` |
| 描述符分配与填充 | 🟢 低 | `Memory.alloc` + `Memory.write64` |
| clone + branch | 🟡 中 | 需要新的 `@native` 注解类型 |
| TLS 设置（arch_prctl） | 🟢 低 | inline asm，一个 syscall |
| 函数分派表查找 | 🟡 中 | 需要 `__aura_fn_table` 地址 |
| 子线程退出（exit_group） | 🟢 低 | 已有 `@native(SYS_EXIT_GROUP)` |
| 跨平台适配 | 🟡 中 | Windows 需 Nt* 系列 syscall |

**总计**: 中等复杂度。主要工作集中在 `Emit.aura` 中新增 `clone_trampoline` 注解的 IR 发射逻辑。

#### 2.5.5 Windows 替代方案

Windows 没有 `clone` syscall。替代路径：

**方案 A: 使用 NtCreateThreadEx（Nt syscall 276）**

```
NtCreateThreadEx(thread, access, obj_attr, process, thread_start,
                  arg, create_flags, zero_bits, stack_size, max_stack_size)
```

这是一个 Nt syscall，可以直接通过 `@native(N)` 调用。但参数较多（11 个），且需要设置 `CLIENT_ID` 和 `KAPC_STATE` 等 Windows 内核结构。

**方案 B: 使用 NtCreateThread（Nt syscall 267）**

较简单的 NtCreateThread 变体，参数较少。

**方案 C: 保留 CRT 调用（CreateThread）**

如果决定 Windows 上保留 CRT 依赖，可以在 `Emit.aura` 的 `winCrtWrapper` 中增加 `CreateThread` 映射。

**推荐**: 方案 A（NtCreateThreadEx），与 Linux 的 clone 对称，完全 syscall 化。

---

## 三、完整实现架构

### 3.1 新增 syscall 常量

```
Syscalls.aura 新增（x86_64 Linux）:

SYS_CLONE              = 56     // clone
SYS_ARCH_PRCTL         = 158    // arch_prctl (ARCH_SET_FS)
SYS_SCHED_GETAFFINITY  = 204    // sched_getaffinity
SYS_GETTID             = 186    // gettid
SYS_NANOSLEEP          = 35     // nanosleep

clone flags:
CLONE_VM               = 0x100
CLONE_FS               = 0x200
CLONE_FILES            = 0x400
CLONE_SIGHAND          = 0x800000
CLONE_THREAD           = 0x10000
CLONE_SETTLS           = 0x80000
CLONE_PARENT_SETTID    = 0x1000
CLONE_CHILD_CLEARTID   = 0x200000
```

### 3.2 ThreadOps.aura 改造

```
┌───────────────────────────────────────────────────────────────────┐
│ ThreadOps.aura                                                     │
│                                                                   │
│ extern interface ThreadOps {                                       │
│   @native(clone_trampoline) fun create(fn_id: Int, arg: Int): Int │
│   @native(SYS_WAIT4) fun wait4Syscall(pid: Int, ...): Int        │
│   @native(SYS_NANOSLEEP) fun nanosleepSyscall(req: Long, ...): Long│
│   @native(SYS_GETTID) fun gettidSyscall(): Int                     │
│   @native(SYS_SCHED_GETAFFINITY) fun schedGetaffinity(...): Int   │
│ }                                                                  │
│                                                                   │
│ object ThreadOpsImpl {                                             │
│   fun create(fn_id: Int, arg: Int): Int                           │
│     → call @native(clone_trampoline)                              │
│                                                                   │
│   fun join(thread_id: Int): Int                                   │
│     → call @native(SYS_WAIT4)                                     │
│                                                                   │
│   fun sleepMs(ms: Int): Unit                                      │
│     → Memory.alloc + @native(SYS_NANOSLEEP) + Memory.free        │
│                                                                   │
│   fun currentId(): Int                                            │
│     → call @native(SYS_GETTID)                                    │
│                                                                   │
│   fun cores(): Int                                                │
│     → Memory.alloc + @native(SYS_SCHED_GETAFFINITY) + bit count  │
│ }                                                                  │
└───────────────────────────────────────────────────────────────────┘
```

### 3.3 编译器后端改造（Emit.aura）

新增 `@native(clone_trampoline)` 的 IR 发射逻辑：

```
emitNativeWrapper 分支扩展:

  if meta.startsWith("native|clone_trampoline") {
    // 生成 clone+trampoline 内联汇编块
    // 1. mmap 分配栈
    // 2. 分配描述符
    // 3. 填充描述符
    // 4. clone + branch + child trampoline
    // 5. parent returns TID
  }
```

### 3.4 函数分派表

编译器已经生成 `__aura_fn_table`（全局函数指针数组）。在 clone trampoline 中需要引用此数组。

```
编译器生成的 IR:
  @__aura_fn_table = global [N x i8*] [i8* @fn0, i8* @fn1, ...]
  @__aura_fn_count = global i32 N

clone trampoline 中:
  lea rax, [__aura_fn_table]     ; 加载函数表基址
  mov rax, [rax + fn_id*8]       ; 查找目标函数
  call rax                       ; 调用线程函数
```

---

## 四、跨平台适配矩阵

| 原语 | Linux x86_64 | Linux aarch64 | Windows x86_64 | macOS x86_64 | macOS aarch64 |
|------|-------------|---------------|---------------|-------------|--------------|
| create | clone(56) + arch_prctl(158) | clone(56) + 直接写 TLS | NtCreateThreadEx(276) | clone(308) + arch_prctl | clone(308) |
| join | wait4(61) | wait4(61) | NtWaitForSingleObject(20) | wait4(61) | wait4(61) |
| sleepMs | nanosleep(35) | nanosleep(63) | NtDelayExecution(8) | nanosleep(230) | nanosleep(63) |
| currentId | gettid(186) | gettid(178) | inline asm (gs:[0x18]) | gettid(202) | gettid(178) |
| cores | sched_getaffinity(204) | sched_getaffinity(204) | NtQuerySystemInformation(30) | sched_getaffinity(204) | sched_getaffinity(204) |

---

## 五、实施计划

### Phase 1: 简单 syscall 替代（1-2 天）

**目标**: 替换 join/sleepMs/currentId/cores，不涉及 clone。

| 任务 | 涉及文件 | 改动量 |
|------|---------|--------|
| 新增 syscall 常量（WAIT4/NANOSLEEP/GETTID/SCHED_GETAFFINITY） | `Syscalls.aura` | ~5 行 |
| 新增 @native 声明 | `ThreadOps.aura` | ~5 行 |
| 实现 join/sleepMs/currentId/cores | `ThreadOps.aura` | ~40 行 |
| Emit.aura 移除 `aura_thread_*` 调用 | `Emit.aura` | ~20 行删除 |

**验证**:
- `aura build Main.aura --aot` 编译通过
- `Thread.sleep(100)` + `Thread.id()` 运行正常
- 并发测试通过

### Phase 2: clone trampoline（1-2 周）

**目标**: 实现 `@native(clone_trampoline)` 注解 + create 函数。

| 任务 | 涉及文件 | 改动量 |
|------|---------|--------|
| 新增 `@native(clone_trampoline)` 语法 | `Parser.aura` / `Ast.aura` | ~30 行 |
| clone flags 常量 | `Syscalls.aura` | ~15 行 |
| Emit.aura 新增 clone_trampoline 分支 | `Emit.aura` | ~100-150 行 |
| ThreadOps.aura 实现 create | `ThreadOps.aura` | ~10 行 |
| Windows NtCreateThreadEx 适配 | `Emit.aura`（win 分支） | ~50 行 |

**验证**:
- 并发测试通过（`tests/pure_aura/native_c2_aot_tests.aura`）
- 多进程/多线程混合测试
- 压力测试（100 并发线程）

### Phase 3: 清理 aura_syscalls.c（1 天）

**目标**: 删除已替代的 C 代码。

| 任务 | 涉及文件 | 改动量 |
|------|---------|--------|
| 删除组 A（syscall 分发器，187 行） | `aura_syscalls.c` | -187 行 |
| 删除组 B（Memory alloc/free，23 行） | `aura_syscalls.c` | -23 行 |
| 删除组 D（线程原语，126 行） | `aura_syscalls.c` | -126 行 |
| 删除组 E（同步原语，~300 行） | `aura_syscalls.c` | -300 行 |
| 删除组 F（原子操作，~50 行） | `aura_syscalls.c` | -50 行 |

**结果**: `aura_syscalls.c` 从 1055 行降至 ~370 行（仅保留 SHA256 + 异常桥 + CPU 内联汇编）。

---

## 六、风险与注意事项

### 6.1 clone trampoline 中的内存可见性

clone 创建的新线程与父线程共享地址空间，但**不共享**栈。父线程分配的栈空间和描述符需要：
1. 在 clone 之前完成分配（否则新线程看不到）
2. 完成数据写入（否则新线程读到垃圾数据）

**保证**: `Memory.alloc` + `Memory.write64` 在 `syscall` 之前执行，编译器生成的 IR 保证了顺序。但需要 `fence seq_cst` 确保写入可见性。

**措施**: 在 clone syscall 前插入 `fence seq_cst`：
```asm
fence seq_cst
syscall  ; clone
```

### 6.2 TLS 设置

子线程需要设置 TLS（Thread-Local Storage）：

- **Linux x86_64**: `arch_prctl(ARCH_SET_FS, tls_desc)` — 设置 %fs 基址
- **Linux aarch64**: TLS 通过 `clone` 的 TLS 参数自动设置（内核管理）
- **Windows**: NtCreateThreadEx 内部设置 TLS

**注意**: `CLONE_SETTLS` 标志要求 `tls` 参数指向一个 `struct user_desc`，而非 TLS 值本身。`struct user_desc` 的布局：

```c
struct user_desc {
    unsigned long entry_number;  // TLS 索引
    unsigned long base_addr;     // TLS 基址
    unsigned long limit;         // 限制
    unsigned int flags;          // 标志
};
```

编译器的 clone trampoline 需要构造此结构。

### 6.3 函数分派表地址

`__aura_fn_table` 是编译时生成的全局数组。在 clone trampoline 的 inline asm 中，需要通过 RIP 相对寻址引用：

```asm
lea rax, [rip + offset_to_fn_table]
mov rax, [rax + rdi*8]  ; rdi = fn_id
call rax
```

**编译器需要**:
1. 在 IR 中生成 `@__aura_fn_table` 全局变量
2. 在 trampoline 中计算偏移量
3. 使用 `getelementptr` 生成正确的地址

### 6.4 Windows 特殊性

Windows 上没有 `clone`，但有 `NtCreateThreadEx`（Nt syscall 276）。参数较多：

```
NtCreateThreadEx(
    thread,             // out: HANDLE
    access,             // THREADEX_ALL_ACCESS = 0x1FFFFF
    obj_attr,           // NULL
    process,            // NtCurrentProcess = -1
    thread_start,       // 线程入口函数指针
    arg,                // 线程参数
    create_flags,       // 0
    zero_bits,          // 0
    stack_size,         // 栈大小
    max_stack_size      // 最大栈大小
)
```

**关键差异**: `NtCreateThreadEx` 需要 `thread_start` 作为参数，而 `clone` 没有这个参数。所以 Windows 上的 trampoline 模式不同——不需要 clone 后的分支，而是直接在 `NtCreateThreadEx` 调用中指定线程入口。

### 6.5 线程安全

clone trampoline 中，子线程和父线程共享地址空间。以下操作必须保证线程安全：

1. **`__aura_fn_table` 读取**: 只读，天然线程安全
2. **描述符读取**: 只读，天然线程安全
3. **内存分配**: 不使用 malloc（避免跨线程竞争），使用 mmap（无锁）

---

## 七、最终结论

| 原语 | 替代方案 | 难度 | 是否可完全消除外部依赖 |
|------|---------|------|---------------------|
| `create` | `@native(clone_trampoline)` → clone+arch_prctl+exit_group | 🟡 中 | ✅ 完全 syscall 化 |
| `join` | `@native(SYS_WAIT4)` | 🟢 低 | ✅ 直接 syscall |
| `sleepMs` | `@native(SYS_NANOSLEEP)` + Memory | 🟢 低 | ✅ 直接 syscall |
| `currentId` | `@native(SYS_GETTID)` | 🟢 低 | ✅ 直接 syscall |
| `cores` | `@native(SYS_SCHED_GETAFFINITY)` + bit count | 🟢 低 | ✅ 直接 syscall |

> **结论**: 线程原语可以完全通过 syscall + inline asm 实现，不需要任何外部 C 库依赖。
>
> 唯一需要编译器后端新增支持的是 `@native(clone_trampoline)` 注解（用于 `create`），其余四个原语均可通过已有的 `@native(SYS_*)` 机制直接替代。
>
> 实施路径：Phase 1（简单 syscall 替代，1-2 天）→ Phase 2（clone trampoline，1-2 周）→ Phase 3（清理 C 代码，1 天）。
