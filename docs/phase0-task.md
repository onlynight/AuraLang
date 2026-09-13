# Phase 0: 编译器 @native 纯 LLVM IR 生成

> **版本**: 1.0  
> **日期**: 2026-09-14  
> **状态**: 进行中  
> **目标**: 让 `@native(N)` 直接生成纯 LLVM syscall IR，不依赖 C 分发函数  
> **前置文档**: `docs/完全Aura化技术方案-v3.0.md`

---

## 〇、阶段目标

### 0.0 当前状态（错误方向）

```
当前 @native(N) 生成路径：
  @native(1) fun write(fd, buf, count) → NativeAttr::Syscall(1)
  → emit.rs: call i64 @aura_syscall_dispatch(i64 1, i64 %fd, i64 %buf, i64 %count, i64 0, i64 0, i64 0)
  → 需要链接 aura_syscalls.c 中的 aura_syscall_dispatch 函数
  → 运行时依赖 C 代码
```

### 0.0.1 目标状态（正确方向）

```
目标 @native(N) 生成路径：
  @native(1) fun write(fd, buf, count) → NativeAttr::Syscall(1)
  → emit.rs: 生成内联 syscall 指令 IR
    %rax = i64 1
    call void asm sideeffect "syscall", "={rax},{rdi},{rsi},{rdx},~{rcx},~{r11}"(
      i64 %rax, i64 %fd, i64 %buf, i64 %count)
    ret i64 %rax
  → 不需要任何 C 代码
  → 纯 LLVM 产物
```

### 0.0.2 验收标准

| 标准 | 判定 |
|------|------|
| `@native(N)` 生成纯 LLVM IR | 生成的 .ll 文件中无 `@aura_syscall_dispatch` |
| `@native(asm="...")` 生成 LLVM inline asm | 已有，不变 |
| `native fun` 生成纯 LLVM IR | `ret 0` 空壳不再出现 |
| Syscalls.aura 编译通过 | `aura build aura/core/aura/lang/native/arch/x86_64_linux/Syscalls.aura --aot` |
| 不需要链接 C .o 文件 | `llc → clang` 不需要 aura_syscalls.o |

---

## 一、任务清单

### 1.1 Syscall 分支改为内联 syscall 指令

**位置**：`compiler/src/codegen/aot/emit.rs`（`emit_native_wrapper` 函数的 `Syscall` 分支）

**当前代码**（错误）：
```rust
crate::ast::NativeAttr::Syscall(nr) => {
    let call_str = format!("call i64 @aura_syscall_dispatch({})", call_args.join(", "));
    format!("{} = {}", ret_str, call_str)
}
```

**修正代码**：
```rust
crate::ast::NativeAttr::Syscall(nr) => {
    // 生成内联 syscall 指令 IR
    let nr_str = nr.to_string();
    
    // 构建 asm 调用：syscall 指令 + 约束字符串
    // Linux x86_64: %rax = nr, %rdi = arg0, %rsi = arg1, %rdx = arg2
    let asm_body = "syscall";
    let constraints = "={rax},{rdi},{rsi},{rdx},~{rcx},~{r11}";
    
    // 参数列表
    let mut arg_names: Vec<String> = Vec::new();
    arg_names.push(format!("i64 {}", nr_str));  // %rax
    arg_names.push(format!("i64 %arg.0"));       // %rdi (arg0)
    if func.params.len() > 1 {
        arg_names.push("i64 %arg.1".to_string());  // %rsi (arg1)
    } else {
        arg_names.push("i64 0".to_string());
    }
    if func.params.len() > 2 {
        arg_names.push("i64 %arg.2".to_string());  // %rdx (arg2)
    } else {
        arg_names.push("i64 0".to_string());
    }
    
    let call = format!(
        "call {} asm sideeffect \"{}\", \"{}\"({})",
        ret_str, asm_body, constraints,
        arg_names.join(", ")
    );
    if ret_str == "void" { call } else { format!("%result = {}", call) }
}
```

### 1.2 Builtin 分支扩展

**位置**：`compiler/src/codegen/aot/emit.rs`（`emit_native_wrapper` 函数的 `Builtin` 分支）

**当前代码**（不完整）：
```rust
crate::ast::NativeAttr::Builtin => {
    let sym_lower = sym.to_lowercase();
    if sym_lower.contains("alloc") {
        "call i64 @aura_memory_alloc(i64 %arg.0)".to_string()
    } else if sym_lower.contains("free") {
        "call void @aura_memory_free(i64 %arg.0)".to_string()
    }
    // ... 只有 4 个匹配
}
```

**修正**：
- `alloc(n)` → 生成 `brk` syscall IR（获取当前 brk → 扩展 brk → 返回起始地址）
- `free(addr)` → 生成 `mmap`/`munmap` syscall IR
- `read(addr)` → `inttoptr i64 %addr to i8*` + `load i8, i8* %ptr`（已有）
- `write(addr, val)` → `inttoptr` + `store i8 %val, i8* %ptr`（已有）

### 1.3 去除 C 依赖声明

**位置**：`compiler/src/codegen/aot/emit.rs`（`emit_native_wrappers` 函数开头）

**当前代码**：
```rust
s.push_str("declare i64 @aura_syscall_dispatch(i64 %arg.0, i64 %arg.1, ...)\n");
s.push_str("declare i64 @aura_memory_alloc(i64 %arg.0)\n");
s.push_str("declare void @aura_memory_free(i64 %arg.0)\n");
s.push_str("declare i64 @aura_cpu_rdtsc()\n");
// ...
```

**修正**：
- 保留 `@aura_cpu_*` 声明（CPU 指令仍走 C 封装，性能更优）
- 移除 `@aura_syscall_dispatch` 声明
- 移除 `@aura_memory_alloc` / `@aura_memory_free` 声明

### 1.4 添加 syscall 号常量表

**位置**：`compiler/src/ast.rs`（新增 `syscall_number` 函数）

**新增**：
```rust
pub fn lookup_syscall_const(name: &str) -> Option<i64> {
    match name {
        "SYS_READ" => Some(0),
        "SYS_WRITE" => Some(1),
        "SYS_OPEN" => Some(2),
        "SYS_CLOSE" => Some(3),
        "SYS_STAT" => Some(4),
        "SYS_FORK" => Some(57),
        "SYS_EXECVE" => Some(59),
        "SYS_WAIT4" => Some(61),
        "SYS_KILL" => Some(62),
        "SYS_EXIT" => Some(60),
        "SYS_CLOCK_GETTIME" => Some(228),
        "SYS_MMAP" => Some(9),
        "SYS_MUNMAP" => Some(11),
        "SYS_BRK" => Some(12),
        "SYS_GETPID" => Some(39),
        "SYS_GETPPID" => Some(64),
        "SYS_GETUID" => Some(102),
        "SYS_GETEUID" => Some(50),
        "SYS_ACCESS" => Some(21),
        "SYS_RENAME" => Some(82),
        "SYS_MKDIR" => Some(83),
        "SYS_RMDIR" => Some(84),
        "SYS_READDIR" => Some(80),
        "SYS_INOTIFY_INIT" => Some(254),
        "SYS_INOTIFY_ADD_WATCH" => Some(255),
        "SYS_INOTIFY_RM_WATCH" => Some(256),
        "SYS_NANOSLEEP" => Some(35),
        "SYS_SOCKET" => Some(41),
        "SYS_CONNECT" => Some(42),
        "SYS_BIND" => Some(49),
        "SYS_LISTEN" => Some(50),
        "SYS_ACCEPT" => Some(43),
        "SYS_SENDTO" => Some(44),
        "SYS_RECVFROM" => Some(45),
        "SYS_SELECT" => Some(23),
        "SYS_PSELECT6" => Some(273),
        "SYS_EPOLL_CREATE1" => Some(291),
        "SYS_EPOLL_CTL" => Some(290),
        "SYS_EPOLL_PWAIT" => Some(281),
        "SYS_PIPE" => Some(22),
        "SYS_PIPE2" => Some(257),
        "SYS_DUP" => Some(32),
        "SYS_DUP2" => Some(33),
        "SYS_IOCTL" => Some(16),
        "SYS_GETENV" => Some(-1), // 特殊：不是 syscall
        "SYS_GETENV_PATH" => Some(-2), // 特殊：读取 /proc/self/environ
        _ => None,
    }
}
```

### 1.5 添加 Windows Nt* 服务号表

**位置**：`compiler/src/ast.rs`（新增 `lookup_nt_service` 函数）

**新增**：
```rust
pub fn lookup_nt_service(name: &str) -> Option<i64> {
    match name {
        "NtReadFile" => Some(0x01),
        "NtWriteFile" => Some(0x02),
        "NtCreateFile" => Some(0x05),
        "NtClose" => Some(0x08),
        "NtOpenFile" => Some(0x0C),
        "NtCreateProcess" => Some(0x16),
        "NtWaitForSingleObject" => Some(0x17),
        "NtTerminateProcess" => Some(0x31),
        "NtExitProcess" => Some(0x1A),
        "NtQuerySystemInformation" => Some(0x2E),
        "NtAllocateVirtualMemory" => Some(0x30),
        "NtFreeVirtualMemory" => Some(0x31),
        "NtMapViewOfSection" => Some(0x36),
        "NtUnmapViewOfSection" => Some(0x3A),
        "NtResumeThread" => Some(0x27),
        "NtSuspendThread" => Some(0x28),
        "NtGetCpuTime" => Some(0x1F),
        "NtQueryInformationProcess" => Some(0x19),
        "NtCreateThread" => Some(0x26),
        "NtCreateMutant" => Some(0x13),
        "NtReleaseMutant" => Some(0x14),
        "NtCreateEvent" => Some(0x1B),
        "NtSetEvent" => Some(0x1C),
        "NtPulseEvent" => Some(0x1D),
        "NtCreateSemaphore" => Some(0x22),
        "NtReleaseSemaphore" => Some(0x23),
        "NtWaitForSingleObjectEx" => Some(0x18),
        "NtCreatePipe" => Some(0x15),
        "NtDuplicateObject" => Some(0x1E),
        "NtCreateDirectoryObject" => Some(0x1A),
        "NtCreateFileObject" => Some(0x1B),
        "NtCreateSection" => Some(0x2A),
        "NtMapViewOfSection2" => Some(0x3B),
        "NtCreateSymbolicLinkObject" => Some(0x4B),
        "NtCreateUserProcess" => Some(0x53),
        _ => None,
    }
}
```

### 1.6 验证

**测试文件**：`tests/pure_aura/native_c2_syscall_tests.aura`

```aura
import aura.lang.native.arch.x86_64_linux.Syscalls
import aura.lang.native.Memory

fun main(): Int {
    // 测试 1: write syscall
    val msg: Long = Memory.alloc(6)
    // ... 写入 "hello\n" 到 msg
    val ret: Int = Syscalls.write(1, msg, 6)
    if (ret == 6) {
        println("TEST 1: write syscall PASS")
    } else {
        println("TEST 1: write syscall FAIL: " + ret)
    }
    
    // 测试 2: getpid syscall
    val pid: Int = Syscalls.getpid()
    if (pid > 0) {
        println("TEST 2: getpid syscall PASS: pid=" + pid)
    } else {
        println("TEST 2: getpid syscall FAIL")
    }
    
    // 测试 3: clock_gettime syscall
    val time: Int = Syscalls.clock_gettime()
    if (time > 0) {
        println("TEST 3: clock_gettime syscall PASS")
    } else {
        println("TEST 3: clock_gettime syscall FAIL")
    }
    
    Memory.free(msg)
    return 0
}
```

---

## 二、交付物

| 文件 | 改动类型 | 行数 |
|------|---------|------|
| `compiler/src/codegen/aot/emit.rs` | 修改 Syscall/Builtin 分支 | ~80 行改动 |
| `compiler/src/ast.rs` | 新增 syscall 号常量表 | ~60 行新增 |
| `compiler/src/codegen/aot/emit.rs` | 移除 C 依赖声明 | ~10 行删除 |
| `tests/pure_aura/native_c2_syscall_tests.aura` | 新增测试 | ~50 行 |

---

## 三、风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| Linux syscall 号跨内核版本不一致 | 低 | 使用标准 syscall 号（Linux 5.0+ 稳定） |
| x86_64 寄存器约定不同平台 | 中 | 按 target_triple 分支生成 |
| Windows Nt* 服务号不稳定 | 高 | 先只支持 Linux，Windows 后续补 |

---

*本文档为 Phase 0 任务说明，配套 `docs/完全Aura化技术方案-v3.0.md` 阅读。*
