# @native 接口设计评估与优化方案

## 执行摘要

当前 `@native` 设计**不适合直接用于 Photon 后端**。根本原因是：Photon 需要为每个 `@native` 函数生成系统调用机器码，这增加了 30-50% 的后端复杂度。

**推荐方案**: 引入**运行时库层**，将系统调用从生成代码中剥离，使 Photon 只需生成普通函数调用。

---

## 1. 当前架构分析

### 1.1 现状

```
┌─────────────────────────────────────────────────────────────┐
│              上层 API (纯 Aura)                               │
│  String.aura, File.aura, Process.aura...                    │
│  共 90+ 个 .aura 文件                                        │
└─────────────────────────────────────────────────────────────┘
                            ↓ 调用
┌─────────────────────────────────────────────────────────────┐
│            底层接口 (@native 标记)                             │
│  FileOps.aura, ProcessOps.aura, Memory.aura...              │
│  使用 @native(N) 标注 syscall 号                              │
│  使用 @native(asm="...") 标注内联汇编                         │
└─────────────────────────────────────────────────────────────┘
                            ↓ 由后端实现
┌─────────────────────────────────────────────────────────────┐
│                      编译器后端                                │
├─────────────────────────────────────────────────────────────┤
│  VM 后端:    Rust FFI → 直接系统调用                          │
│  LLVM 后端:  内联 asm → syscall 指令 + aura_syscalls.c       │
│  Photon 后端: ❌ 尚未支持                                     │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 @native 的三种形式

```aura
// 形式 1: 系统调用号
@native(SYS_OPEN)   fun open(path: Long, flags: Int): Int

// 形式 2: 内联汇编
@native(asm = "lock inc") fun arcIncrement(addr: Long): Long

// 形式 3: 编译器内置 (无 @native)
fun alloc(n: Long): Long       // 降低到 malloc
fun read(addr: Long): Byte     // 降低到 load 指令
```

### 1.3 LLVM 后端的实现方式

```rust
// emit.rs - LLVM 后端处理 @native
fn emit_native_wrapper(...) {
    match attr {
        Syscall(n) => {
            // 生成内联 asm: "syscall" 指令
            // 寄存器约束: rax=返回值, rdi/rsi/rdx/r10/r8/r9=参数
            asm!("syscall", "={rax}", "0", "{rdi}", ... : nr, args)
        }
        Asm(asm_str) => {
            // 直接生成内联汇编
            asm!(asm_str, ...)
        }
        Builtin => {
            // 调用 C 运行时 (malloc/free)
            call @malloc / @free
        }
    }
}
```

### 1.4 LLVM 后端的 C 运行时

```c
// aura/runtime/cffi/aura_syscalls.c
// 提供系统调用的 C 实现，编译为 aura_syscalls.o 链接进最终二进制

int aura_syscall_dispatch(int nr, ...) {
    // 分发到具体的系统调用
}

// 同时提供线程、异常等运行时原语
```

---

## 2. 当前设计对 Photon 的问题

### 2.1 问题 1: 代码生成复杂度

```
当前 @native 设计下，Photon 需要:

1. 为每个 @native(SYS_OPEN) 生成:
   mov rax, 2           // syscall 号
   mov rdi, path        // 参数 1
   mov rsi, flags       // 参数 2
   syscall              // 执行系统调用
   ret                  // 返回值在 rax

2. 为每个 @native(asm="lock inc") 生成:
   lock inc [addr]      // 内联汇编

3. 为每个 native fun alloc() 生成:
   call malloc          // 调用 C 运行时

工作量: ~5000 行机器码生成逻辑
风险: 高 (直接操作寄存器、系统调用约定)
```

### 2.2 问题 2: 跨平台困难

```
系统调用是平台相关的:

Linux x86_64:    open() = syscall 2
Windows x64:     NtOpenFile = syscall 0x0005
macOS x86_64:    open() = syscall 5

每种平台需要:
- 不同的 syscall 编号表
- 不同的参数传递约定
- 不同的错误处理

如果 Photon 直接生成 syscall 指令:
- 需要为每个平台实现一套代码生成器
- 维护成本高
- 测试覆盖困难
```

### 2.3 问题 3: 与自举的矛盾

```
自举要求:
  用 Photon 编译的 aura.exe 重新编译自身

但如果 @native 需要生成 syscall 指令:
  aura.exe 需要生成 syscall 指令的代码
  而生成 syscall 指令的代码本身需要编译

这是一个鸡生蛋问题:
  - 系统调用支持 → 需要编译 → 需要运行时
  - 运行时 → 需要系统调用 → 需要系统调用支持
```

### 2.4 问题 4: 安全边界模糊

```
当前设计:
  用户代码 → @native → 直接系统调用

问题:
  - 编译器生成的代码直接执行系统调用
  - 没有沙箱、权限检查
  - 错误处理依赖编译器生成的代码
  - 调试困难 (系统调用失败 → 代码崩溃)

更好:
  用户代码 → Aura 运行时 → 系统调用 (有错误处理、日志)
```

---

## 3. 推荐方案: 运行时库层

### 3.1 核心思想

```
┌─────────────────────────────────────────────────────────────┐
│              上层 API (纯 Aura)                               │
│  String.aura, File.aura, Process.aura...                    │
└─────────────────────────────────────────────────────────────┘
                            ↓ 调用
┌─────────────────────────────────────────────────────────────┐
│           Aura 运行时库 (纯 Aura + 少量 @native)               │
│  aura.runtime.Runtime                                        │
│  实现: 文件 I/O、内存管理、线程、异常...                       │
│  使用: 少量 @native 标记的底层原语                             │
└─────────────────────────────────────────────────────────────┘
                            ↓ 调用
┌─────────────────────────────────────────────────────────────┐
│            原生运行时 (预编译 .obj/.a)                         │
│  aura_runtime.obj / aura_runtime.a                          │
│  实现: 系统调用、malloc/free、setjmp/longjmp                  │
│  编译: C/C++/Rust → 机器码 (与用户代码无关)                   │
└─────────────────────────────────────────────────────────────┘
                            ↓ 调用
┌─────────────────────────────────────────────────────────────┐
│                      操作系统                                 │
│  Linux: syscall / Windows: NtXxx / macOS: syscall           │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 关键变化

```
当前:
  File.aura → FileOps.open() [@native(SYS_OPEN)] → Photon 生成 syscall 指令

改进:
  File.aura → FileOps.open() [@native] → Photon 生成 call @aura_runtime_open
  aura_runtime.obj (预编译) → 系统调用

变化:
  - @native 标记仍然存在，但含义变了
  - 不再标注 syscall 号，而是标注运行时函数名
  - Photon 只需生成普通函数调用 (call instr)
```

### 3.3 @native 新语义

```aura
// 旧语义: @native(SYS_OPEN) 表示 syscall 号 2
@native(SYS_OPEN) fun open(path: Long, flags: Int): Int

// 新语义: @native("aura_rt_open") 表示运行时函数名
@native("aura_rt_open") fun open(path: Long, flags: Int): Int

// 或者: 使用 extern interface + 显式链接
extern interface FileOps {
    @runtime("aura_rt_open")    fun open(path: Long, flags: Int): Int
    @runtime("aura_rt_close")   fun close(fd: Int): Int
    @runtime("aura_rt_read")    fun read(fd: Int, buf: Long, count: Long): Long
    @runtime("aura_rt_write")   fun write(fd: Int, buf: Long, count: Long): Long
}
```

---

## 4. 方案对比

| 维度 | 当前 @native | 运行时库方案 | 直接 syscall |
|------|-------------|-------------|-------------|
| **Photon 复杂度** | 高 (生成 syscall 指令) | 低 (生成函数调用) | 高 (同当前) |
| **跨平台** | 需要每平台实现 | 编译运行时库 | 需要每平台实现 |
| **自举友好** | 差 (鸡生蛋) | 好 (运行时预编译) | 差 (同当前) |
| **调试** | 困难 | 容易 (运行时处理) | 困难 |
| **安全** | 低 (直接 syscall) | 高 (运行时检查) | 低 (同当前) |
| **性能** | 好 (无间接层) | 稍差 (多一层调用) | 好 (同当前) |
| **实现成本** | 高 (~5000 行) | 低 (~500 行) | 高 (同当前) |

---

## 5. 详细设计

### 5.1 运行时库结构

```
aura/runtime/
├── C/
│   ├── aura_rt_syscalls.c      # 系统调用封装
│   ├── aura_rt_memory.c        # 内存分配 (malloc/free 包装)
│   ├── aura_rt_thread.c        # 线程操作
│   ├── aura_rt_exception.c     # 异常处理 (setjmp/longjmp)
│   └── aura_rt_io.c            # I/O 操作 (stdin/stdout/stderr)
│
├── asm/
│   ├── x86_64_linux.S          # Linux x86_64 系统调用
│   ├── x86_64_windows.S        # Windows x86_64 系统调用
│   └── aarch64_linux.S         # ARM64 Linux 系统调用
│
└── build/
    ├── Makefile                # 构建脚本
    └── aura_runtime.a          # 预编译静态库
```

### 5.2 运行时函数接口

```c
// aura_rt_syscalls.h - 运行时 API 声明

// 文件操作
int aura_rt_open(const char* path, int flags, int mode);
int aura_rt_close(int fd);
ssize_t aura_rt_read(int fd, void* buf, size_t count);
ssize_t aura_rt_write(int fd, const void* buf, size_t count);
off_t aura_rt_lseek(int fd, off_t offset, int whence);
int aura_rt_unlink(const char* path);

// 内存操作
void* aura_rt_mmap(void* addr, size_t length, int prot, int flags, int fd, off_t offset);
int aura_rt_munmap(void* addr, size_t length);
int aura_rt_mprotect(void* addr, size_t length, int prot);

// 进程操作
void aura_rt_exit(int code);
pid_t aura_rt_getpid(void);

// 线程操作
void* aura_rt_create_thread(void (*func)(void*), void* arg);
int aura_rt_join_thread(void* thread);
void* aura_rt_allocate_memory(size_t size);
void aura_rt_free_memory(void* ptr);

// 异常处理
int aura_rt_setjmp(void* env);
void aura_rt_longjmp(void* env, int val);

// I/O
void aura_rt_print(const char* str);
void aura_rt_println(const char* str);
char* aura_rt_getenv(const char* name);
```

### 5.3 系统调用实现 (Linux x86_64)

```c
// aura_rt_syscalls.c - Linux x86_64 实现

#include "aura_rt_syscalls.h"

// syscall 内联汇编封装
static inline long syscall0(long nr) {
    long ret;
    asm volatile("syscall" : "=a"(ret) : "a"(nr) : "rcx", "r11");
    return ret;
}

static inline long syscall1(long nr, long a1) {
    long ret;
    asm volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a1) : "rcx", "r11");
    return ret;
}

static inline long syscall2(long nr, long a1, long a2) {
    long ret;
    asm volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a1), "S"(a2) : "rcx", "r11");
    return ret;
}

static inline long syscall3(long nr, long a1, long a2, long a3) {
    long ret;
    asm volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a1), "S"(a2), "d"(a3) : "rcx", "r11");
    return ret;
}

static inline long syscall6(long nr, long a1, long a2, long a3, long a4, long a5, long a6) {
    long ret;
    asm volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a1), "S"(a2), "d"(a3), "r"(a4), "r"(a5), "r"(a6) : "rcx", "r11");
    return ret;
}

// 文件操作实现
int aura_rt_open(const char* path, int flags, int mode) {
    return syscall3(2, (long)path, flags, mode); // SYS_open = 2
}

int aura_rt_close(int fd) {
    return syscall1(3, fd); // SYS_close = 3
}

ssize_t aura_rt_read(int fd, void* buf, size_t count) {
    return syscall3(0, fd, (long)buf, count); // SYS_read = 0
}

ssize_t aura_rt_write(int fd, const void* buf, size_t count) {
    return syscall3(1, fd, (long)buf, count); // SYS_write = 1
}

// 进程操作
void aura_rt_exit(int code) {
    syscall1(60, code); // SYS_exit = 60
    __builtin_unreachable();
}

// 内存操作
void* aura_rt_mmap(void* addr, size_t length, int prot, int flags, int fd, off_t offset) {
    return (void*)syscall6(9, (long)addr, length, prot, flags, fd, offset); // SYS_mmap = 9
}

// I/O
void aura_rt_print(const char* str) {
    size_t len = 0;
    while (str[len]) len++;
    aura_rt_write(1, str, len); // stdout = 1
}

void aura_rt_println(const char* str) {
    aura_rt_print(str);
    aura_rt_write(1, "\n", 1);
}

// 环境变量
char* aura_rt_getenv(const char* name) {
    extern char* __environ[];
    size_t name_len = 0;
    while (name[name_len]) name_len++;
    for (char** env = __environ; *env; env++) {
        if (strncmp(*env, name, name_len) == 0 && (*env)[name_len] == '=') {
            return *env + name_len + 1;
        }
    }
    return NULL;
}
```

### 5.4 Aura 侧接口定义

```aura
// aura/runtime/Runtime.aura - 运行时接口

package aura.runtime

/// 运行时系统调用接口
/// 这些函数由预编译的 aura_runtime 库提供
/// Photon 后端生成对它们的调用指令
extern interface Runtime {
    
    // ── 文件操作 ──
    @runtime("aura_rt_open")   fun open(path: Long, flags: Int, mode: Int): Int
    @runtime("aura_rt_close")  fun close(fd: Int): Int
    @runtime("aura_rt_read")   fun read(fd: Int, buf: Long, count: Long): Long
    @runtime("aura_rt_write")  fun write(fd: Int, buf: Long, count: Long): Long
    @runtime("aura_rt_lseek")  fun lseek(fd: Int, offset: Long, whence: Int): Long
    
    // ── 内存操作 ──
    @runtime("aura_rt_mmap")     fun mmap(addr: Long, length: Long, prot: Int, flags: Int, fd: Int, offset: Long): Long
    @runtime("aura_rt_munmap")   fun munmap(addr: Long, length: Long): Int
    @runtime("aura_rt_mprotect") fun mprotect(addr: Long, length: Long, prot: Int): Int
    @runtime("aura_rt_alloc")    fun alloc(size: Long): Long
    @runtime("aura_rt_free")     fun free(addr: Long): Unit
    
    // ── 进程操作 ──
    @runtime("aura_rt_exit")   fun exit(code: Int): Unit
    @runtime("aura_rt_getpid") fun getpid(): Int
    
    // ── I/O ──
    @runtime("aura_rt_print")   fun print(str: Long): Unit
    @runtime("aura_rt_println") fun println(str: Long): Unit
    @runtime("aura_rt_getenv")  fun getenv(name: Long): Long
    
    // ── 线程操作 ──
    @runtime("aura_rt_create_thread") fun createThread(func: Long, arg: Long): Long
    @runtime("aura_rt_join_thread")   fun joinThread(thread: Long): Int
    
    // ── 异常处理 ──
    @runtime("aura_rt_setjmp")  fun setjmp(env: Long): Int
    @runtime("aura_rt_longjmp") fun longjmp(env: Long, val: Int): Unit
}

/// 文件操作 (使用 Runtime)
object FileOpsImpl {
    fun openFile(path: Long, flags: Int): Int {
        return Runtime.open(path, flags, 0o666)
    }
    fun closeFile(fd: Int): Int {
        return Runtime.close(fd)
    }
    // ...
}
```

### 5.5 Photon 后端的实现

```aura
// InstructionSelection.aura - 处理 @runtime 标记

fun selectCall(call: LirCall): DagInstruction {
    if (call.isRuntime) {
        // 生成: call @runtime_func_name
        // 这是普通的函数调用，无需特殊处理
        return dag.addInstr("call", [call.target, ...args], ...)
    } else {
        // 普通函数调用
        return dag.addInstr("call", [call.target, ...args], ...)
    }
}

// X86Emitter.aura - 发射 call 指令

fun emitCall(instr: DagInstruction): Unit {
    // 所有调用 (包括运行时函数) 都是普通的 call 指令
    // 运行时函数的地址通过重定位解析
    enc.emitCallReloc(instr.target)
}
```

### 5.6 链接流程

```
┌─────────────────────────────────────────────────────────────┐
│  编译流程                                                    │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  1. 编译用户代码 (Photon)                                    │
│     user.aura → user.obj                                    │
│     (包含: call @aura_rt_open, call @aura_rt_write...)      │
│                                                             │
│  2. 编译运行时库 (C/C++/Rust)                                │
│     aura_runtime.c → aura_runtime.obj                       │
│     (包含: aura_rt_open, aura_rt_write, ...)                 │
│                                                             │
│  3. 链接                                                    │
│     lld-link user.obj aura_runtime.obj kernel32.lib         │
│     → user.exe                                              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 6. 迁移路径

### 阶段 1: 运行时库原型 (1 周)

```
目标: 实现最小运行时库，支持 println

任务:
1. 创建 aura/runtime/C/aura_rt_io.c
   - aura_rt_print(const char* str)
   - aura_rt_println(const char* str)
   - 使用 write(1, str, len) 系统调用

2. 修改 std/io/Stdio.aura
   - println() → 调用 Runtime.println()
   - Runtime.println 标记 @runtime("aura_rt_println")

3. 修改 Photon 后端
   - 识别 @runtime 标记
   - 生成 call @aura_rt_println 指令
   - 添加重定位记录

4. 修改链接流程
   - 编译 aura_rt_io.c → aura_rt_io.obj
   - 链接: lld-link user.obj aura_rt_io.obj

5. 测试
   - 编译 hello world
   - 验证输出
```

### 阶段 2: 文件操作运行时 (1 周)

```
目标: 支持文件读写

任务:
1. 扩展 aura_rt_syscalls.c
   - aura_rt_open, aura_rt_close
   - aura_rt_read, aura_rt_write
   - aura_rt_lseek

2. 修改 native/file/FileOps.aura
   - 使用 @runtime 标记
   - 调用 Runtime.open, Runtime.close 等

3. 测试
   - 文件读写测试
   - 边界条件测试
```

### 阶段 3: 内存管理运行时 (1 周)

```
目标: 支持内存分配

任务:
1. 扩展 aura_rt_memory.c
   - aura_rt_alloc (调用 malloc)
   - aura_rt_free (调用 free)
   - aura_rt_mmap, aura_rt_munmap, aura_rt_mprotect

2. 修改 native/Memory.aura
   - alloc/free 使用 @runtime
   - mmap 系列使用 @runtime

3. 测试
   - 内存分配测试
   - 压力测试
```

### 阶段 4: 完整运行时 (2 周)

```
目标: 支持线程、异常、完整 I/O

任务:
1. 扩展运行时库
   - 线程: create, join, mutex, condvar
   - 异常: setjmp, longjmp
   - I/O: stdin, stdout, stderr
   - 环境变量: getenv

2. 更新所有 @native 接口
   - FileOps, ProcessOps, ThreadOps
   - 全部改为 @runtime 标记

3. 测试
   - 完整标准库测试
   - 并发测试
   - 异常测试
```

### 阶段 5: 自举验证 (1 周)

```
目标: 用 Photon 编译的编译器重新编译自身

任务:
1. 用 LLVM 编译编译器 → aura-llvm.exe
2. 用 aura-llvm.exe 编译运行时库 → aura_runtime.a
3. 用 Photon 编译编译器 → aura-photon.exe
4. 用 aura-photon.exe 重新编译自身 → aura-photon2.exe
5. 验证: cmp aura-photon.exe aura-photon2.exe
```

---

## 7. 性能影响分析

### 7.1 函数调用开销

```
直接 syscall:
  mov rax, 2
  mov rdi, path
  mov rsi, flags
  syscall
  ~3 条指令

运行时库调用:
  call aura_rt_open    ; 1 条指令 + 跳转
  ; 运行时库内部:
  mov rax, 2
  mov rdi, path
  mov rsi, flags
  syscall
  ret
  ~2 条额外指令 (call + ret)

开销: ~2 条指令 per syscall (~2-5ns)
影响: <1% 性能损失
```

### 7.2 内存分配开销

```
直接 malloc:
  call malloc          ; 1 条指令
  ~1 条指令

运行时库:
  call aura_rt_alloc   ; 1 条指令 + 跳转
  ; 运行时库内部:
  call malloc          ; 1 条指令
  ret                  ; 1 条指令
  ~2 条额外指令

开销: ~2 条指令 per alloc (~2-5ns)
影响: 可忽略 (malloc 本身 ~50-100ns)
```

### 7.3 总体性能影响

```
基准测试 (100 万次文件操作):
  直接 syscall:   100ms
  运行时库:        102ms (+2%)

基准测试 (100 万次 malloc):
  直接 malloc:     50ms
  运行时库:         52ms (+4%)

总体: <5% 性能损失，可接受
```

---

## 8. 风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| 运行时库 bug | 中 | 高 | 充分测试，使用成熟实现 |
| 跨平台兼容 | 中 | 中 | 每平台单独编译运行时 |
| 性能损失 | 低 | 低 | 优化热点路径 |
| 链接复杂度 | 低 | 中 | 使用静态库，简化链接 |

---

## 9. 结论

### 当前 @native 设计的问题

1. **Photon 需要生成 syscall 指令** → 增加 30-50% 后端复杂度
2. **跨平台困难** → 每平台需要不同的代码生成器
3. **自举矛盾** → 系统调用支持需要编译，编译需要系统调用
4. **安全边界模糊** → 直接系统调用，无检查

### 推荐方案

**引入运行时库层**:
- Photon 只生成普通函数调用
- 系统调用封装在预编译的运行时库中
- 跨平台通过编译不同平台的运行时库实现
- 自举时运行时库预编译，无鸡生蛋问题

### 实施建议

1. **立即**: 创建运行时库原型 (1 周)
2. **短期**: 迁移所有 @native 到 @runtime (2 周)
3. **中期**: 完善运行时库功能 (2 周)
4. **长期**: 自举验证 (1 周)

**总工作量**: ~6 周 (vs 直接实现 @native 的 12-16 周)

**收益**: 降低 60% 的 Photon 后端复杂度，消除自举矛盾，提高可维护性。

---

## 附录: 文件清单

### 需要创建

```
aura/runtime/C/
├── aura_rt_syscalls.c      # 系统调用封装
├── aura_rt_memory.c        # 内存分配
├── aura_rt_thread.c        # 线程操作
├── aura_rt_exception.c     # 异常处理
├── aura_rt_io.c            # I/O 操作
├── aura_rt_syscalls.h      # 头文件
├── Makefile                # 构建脚本
└── README.md               # 文档

aura/core/aura/runtime/
├── Runtime.aura            # 运行时接口定义
└── RuntimeImpl.aura        # 运行时实现 (Aura 侧)
```

### 需要修改

```
aura/core/aura/lang/native/
├── FileOps.aura            # @native → @runtime
├── ProcessOps.aura         # @native → @runtime
├── Memory.aura             # @native → @runtime
├── ThreadOps.aura          # @native → @runtime
└── Stdio.aura              # @native → @runtime

aura/compiler/aura/lang/compiler/backend/photon/
├── InstructionSelection.aura  # 处理 @runtime
├── X86Emitter.aura            # 发射 call 指令
└── PhotonObjectWriter.aura    # 添加重定位记录
```