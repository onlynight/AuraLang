# AOT 编译模式去除 Rust/C-C++ 依赖方案

> **文档性质**：技术分析 + 实施方案
> **核心结论**：Aura 标准库和 native 层**已经是目标架构**，问题出在 AOT 发射器层面
> **约束**：本文仅提供分析与方案，不修改任何代码
> **分析日期**：2026-09-16

---

## 目录

1. [核心发现：基础设施已就位](#1-核心发现基础设施已就位)
2. [方案架构：三层分离](#2-方案架构三层分离)
3. [详细改造计划](#3-详细改造计划)
   - [Phase A：改造 emitNativeWrapper](#phase-a改造-emitnativewrapper从-c-包装器到-llvm-ir)
   - [Phase B：移除 preludeTable C 映射](#phase-b移除-preludetable-c-映射让-std-函数走-aura-实现)
   - [Phase C：移除 AOT 构建中的 C 编译步骤](#phase-c移除-aot-构建中的-c-编译步骤)
   - [Phase D：异常处理替代](#phase-d异常处理setjmplongjmp-替代)
   - [Phase E：冻结引导二进制](#phase-e冻结引导二进制--构建流程)
4. [C 运行库函数到 Aura 实现的映射表](#4-c-运行库函数到-aura-实现的映射)
5. [风险与工作量估算](#5-风险与工作量)
6. [附录：当前代码依赖全景](#附录当前代码依赖全景)

---

## 1. 核心发现：基础设施已就位

### 1.1 已有基础设施（无需新建）

| 层次 | 目录 | 状态 | 说明 |
|------|------|------|------|
| **Aura 逻辑层** | `aura/core/aura/lang/std/` | ✅ 已实现 | String/FileSystem/Math/Process/Process 等，注释明确写"全部以 Aura 编写" |
| **Aura native 层** | `aura/core/aura/lang/native/` | ✅ 已实现 | `@native(N)` syscall + `@native(asm=...)` 内联汇编，5 个平台架构 |
| **数学纯实现** | `native/math/MathOps.aura` | ✅ 已实现 | Taylor 级数/牛顿法，不依赖 libm |
| **AOT 发射器** | `Emit.aura:emitNativeWrapper` | ⚠️ 需改造 | 当前路由到 C 包装器，需改为直接生成 LLVM IR |
| **C 运行库映射** | `Runtime.aura:preludeTable()` | ❌ 需移除 | 强制 std 函数走 C 实现，是 C 依赖的根因 |

**关键证据**：

- `FileSystem.aura` 注释："全部以 Aura 编写，不使用 extern \"C\" 或 Rust cffi"
- `Math.aura` 注释："全部以 Aura 编写，不使用 extern \"C\" 或 Rust cffi"
- `MathOps.aura` 注释："Taylor 级数 / 牛顿法实现，不依赖 libm"
- `Syscalls.aura`（5 个平台）：`@native(0) fun read(...)`, `@native(1) fun write(...)` 等 syscall 号声明
- `Cpu.aura`：`@native(asm = "rdtsc") fun rdtsc(): Long`, `@native(asm = "mfence") fun memFence()`

### 1.2 问题根因

```
当前流程（错误路径）：
  s.length → AOT 发射器查 preludeTable()
           → 生成 declare @aura_lang_std_String_length(i8*)
           → 调用 C 运行库实现（aura_std_cffi.c）

目标流程（正确路径）：
  s.length → ModuleLink 包含 String.aura 模块
           → AOT 发射器编译 Aura 实现
           → Aura 实现内部调用 @native(SYS_READ)
           → AOT 发射器生成内联 syscall 指令
           → 系统链接器解析（无需 C 代码）
```

**一句话**：`preludeTable()` 把 `s.length` 硬映射到 C 符号 `aura_lang_std_String_length`，而 Aura 标准库中 `String.length` 的纯 Aura 实现被完全绕过了。

### 1.3 Native 层完整清单

```
aura/core/aura/lang/native/
├── Syscalls.aura                  ← 平台分叉入口
├── arch/
│   ├── x86_64_linux/Syscalls.aura    ← @native(0) read, @native(1) write, ...
│   ├── x86_64_windows/Syscalls.aura  ← @native(asm="call qword [rip + WriteFile]")
│   ├── x86_64_darwin/Syscalls.aura   ← macOS POSIX syscall
│   ├── aarch64_linux/Syscalls.aura   ← ARM64 Linux syscall 号
│   └── aarch64_darwin/Syscalls.aura  ← ARM64 macOS syscall
├── Cpu.aura                       ← @native(asm="rdtsc"), @native(asm="mfence"), @native(asm="lock xaddq")
├── Memory.aura                    ← @native alloc/free
├── memory/Allocator.aura          ← 内存分配器
├── file/FileOps.aura              ← @native(SYS_OPEN), @native(SYS_READ), ...
├── io/Stdio.aura                  ← I/O 操作
├── math/MathOps.aura              ← 纯 Aura 数学实现（Taylor 级数/牛顿法）
├── process/Process.aura           ← @native(SYS_EXIT_GROUP), @native(SYS_FORK), ...
├── thread/ThreadOps.aura          ← 线程操作
├── network/NetworkOps.aura        ← @native(SYS_SOCKET), @native(SYS_CONNECT), ...
├── time/Clock.aura                ← @native(SYS_CLOCK_GETTIME)
├── console/Console.aura           ← @native(SYS_WRITE)
├── env/                           ← 环境变量
├── boxed/                         ← 装箱操作
```

---

## 2. 方案架构：三层分离

```
┌─────────────────────────────────────────────────────────────────────────┐
│  用户程序 .aura                                                          │
│    │                                                                    │
│    ▼                                                                    │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  Layer 3：Aura 标准库（逻辑层）                                    │   │
│  │  aura/core/aura/lang/std/                                        │   │
│  │  String.aura / FileSystem.aura / Math.aura / Process.aura ...   │   │
│  │  • 纯 Aura 实现，不含 @native                                      │   │
│  │  • 调用 Layer 2 的 native 接口                                    │   │
│  └──────────────────────┬──────────────────────────────────────────┘   │
│                         │                                               │
│                         ▼                                               │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  Layer 2：Aura native 声明（接口层）                               │   │
│  │  aura/core/aura/lang/native/                                     │   │
│  │  • @native(SYS_READ)  → syscall 号                               │   │
│  │  • @native(asm="...") → 内联汇编                                  │   │
│  │  • 平台分叉：x86_64_linux / x86_64_windows / aarch64 / darwin    │   │
│  └──────────────────────┬──────────────────────────────────────────┘   │
│                         │                                               │
│                         ▼                                               │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  Layer 1：LLVM IR（指令层）                                        │   │
│  │  • syscall 指令：mov rax, N; syscall                             │   │
│  │  • inline asm：LLVM asm 语法                                      │   │
│  │  • fence / atomicrmw：LLVM 内建指令                               │   │
│  │  • declare malloc/free：系统链接器解析                             │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘

最终产物：仅 LLVM IR → llc → .obj → clang 链接 → .exe
         无 C 源码、无 C 编译、无 Rust 依赖
```

---

## 3. 详细改造计划

### Phase A：改造 emitNativeWrapper——从 C 包装器到 LLVM IR

#### A.1 `@native(N)` 系统调用：生成内联 syscall 指令

**当前实现**（`Emit.aura:1738-1753`）：
```aura
// @native(SYS_READ)：调用 C 分发入口，参数不足 6 个补 0
body = "call i64 @aura_syscall_dispatch(" + callArgs + ")"
```

**目标实现**：根据目标三元组生成平台特定的内联 syscall 指令。

```aura
/// 按目标三元组生成内联 syscall 汇编。
fun emitSyscallAsm(target: String, nr: String): String {
    if (target.contains("x86_64") && target.contains("linux")) {
        // Linux x86_64: rdi=arg0, rsi=arg1, rdx=arg2, r10=arg3, r8=arg4, r9=arg5
        return "mov rax, " + nr + "; syscall"
    } else if (target.contains("x86_64") && target.contains("darwin")) {
        // macOS x86_64: 同 Linux x86_64 syscall 机制
        return "mov rax, " + nr + "; syscall"
    } else if (target.contains("aarch64") && target.contains("linux")) {
        // Linux aarch64: x0-x5=args, x16=syscall_nr
        return "mov x16, " + nr + "; svc #0"
    } else if (target.contains("aarch64") && target.contains("darwin")) {
        // macOS aarch64: 同 Linux aarch64
        return "mov x16, " + nr + "; svc #0"
    }
    // Windows: 已在 Syscalls.aura 中用 @native(asm="call qword [rip + X]") 标注
    //          走通用 inline asm 路径（A.2），无需特殊处理
    return ""
}
```

**改造要点**：
- `emitNativeWrapper` 中 `isSyscall` 分支不再调用 `aura_syscall_dispatch`
- 改为构造 LLVM `call asm sideeffect` 指令
- 寄存器映射按目标平台生成（x86_64: rdi/rsi/rdx/r10/r8/r9; aarch64: x0-x5）

#### A.2 `@native(asm = "rdtsc")`：直接 LLVM inline asm

**当前实现**（`Emit.aura:1758-1761`）：
```aura
// 已知指令：走 C 封装
body = "call i64 @aura_cpu_rdtsc()"
```

**目标实现**：移除 rdtsc 的特殊分支，统一走通用 inline asm 路径（已有代码，1775-1819 行）。

```aura
// 移除 rdtsc/mfence/atomic 的三个特殊分支
// 统一走通用 inline asm 路径：
//   call i64 asm sideeffect "rdtsc", "=r"()
```

#### A.3 `@native(asm = "mfence")`：LLVM `fence` 指令

**当前实现**（`Emit.aura:1762-1765`）：
```aura
// 走 C 封装
body = "call void @aura_cpu_mem_fence()"
```

**目标实现**：生成 LLVM `fence` 指令。

```aura
// LLVM IR: fence seq_cst
body = "fence seq_cst"
// 注：fence 指令无返回值，直接作为语句发射到 body
```

#### A.4 `@native(asm = "atomic...")`：LLVM `atomicrmw` 指令

**当前实现**（`Emit.aura:1766-1774`）：
```aura
// 走 C 封装
body = "call i64 @aura_cpu_atomic_add(i64 %arg.0, i64 %arg.1)"
```

**目标实现**：生成 LLVM `atomicrmw` 指令。

```aura
// LLVM IR:
//   %result = atomicrmw add i64* %arg.0, i64 %arg.1 seq_cst
// 注意：atomicrmw 需要指针类型操作数，需先 inttoptr
val ptr: String = freshVar()
// %ptr.0 = inttoptr i64 %arg.0 to i64*
body = ptr + " = inttoptr i64 %arg.0 to i64*"
// %result = atomicrmw add i64* %ptr.0, i64 %arg.1 seq_cst
body = body + " " + freshVar() + " = atomicrmw add i64* " + ptr + ", i64 %arg.1 seq_cst"
```

#### A.5 `@native` 内置（alloc/free）：`declare` malloc/free

**当前实现**（`Emit.aura:1824+`）：
```aura
// 走 C 封装
body = "call i64 @aura_memory_alloc(i64 %arg.0)"
body = "call void @aura_memory_free(i64 %arg.0)"
```

**目标实现**：`declare` 系统 malloc/free，由链接器解析。

```aura
// LLVM IR:
//   declare ptr @malloc(i64)     ← 系统 libc 解析
//   declare void @free(i64)      ← 系统 libc 解析
// 注意：LLVM 不透明指针模式下 malloc 返回 ptr 类型
body = "call i64 @malloc(i64 %arg.0)"   // declare malloc
body = "call void @free(i64 %arg.0)"    // declare free
```

#### A.6 声明替换

**`Runtime.aura:runtimeDeclarations()` 修改**：

移除旧 C 包装器声明：
```
declare i64 @aura_syscall_dispatch(i64, i64, i64, i64, i64, i64, i64)
declare i64 @aura_memory_alloc(i64)
declare void @aura_memory_free(i64)
declare i64 @aura_cpu_rdtsc()
declare void @aura_cpu_mem_fence()
declare i64 @aura_cpu_atomic_add(i64, i64)
declare void @aura_args_set(i32, i8**)
```

新增系统库声明：
```
declare i64 @malloc(i64)           ← 系统 libc
declare void @free(i64)            ← 系统 libc
declare i64 @strlen(i8*)           ← 系统 libc（如需要）
declare void @exit(i32)            ← 系统 libc
declare i32 @getpid()              ← 系统 libc（如需要）
```

**注意**：Windows 平台的函数名可能需要 `@__malloc` 或 `@malloc`（取决于 CRT），需按目标三元组适配。

---

### Phase B：移除 preludeTable C 映射——让 std 函数走 Aura 实现

#### B.1 移除 preludeTable 中的 std 函数映射

**当前** `Runtime.aura:preludeTable()` 包含 40+ 个 `aura_lang_std_*` 符号映射：

```
aura_lang_std_String_toInt|i64|i8*
aura_lang_std_String_toFloat|double|i8*
aura_lang_std_String_length|i64|i8*
aura_lang_std_String_equals|i32|i8*,i8*
aura_lang_std_String_contains|i32|i8*,i8*
aura_lang_std_String_startsWith|i32|i8*,i8*
aura_lang_std_String_endsWith|i32|i8*,i8*
aura_lang_std_String_indexOf|i64|i8*,i8*
aura_lang_std_String_countChar|i64|i8*,i8*
aura_lang_std_String_substring|i8*|i8*,i64,i64
aura_lang_std_String_charAt|i8*|i8*,i64
aura_lang_std_String_charCodeAt|i64|i8*,i64
aura_lang_std_String_toUpperCase|i8*|i8*
aura_lang_std_String_toLowerCase|i8*|i8*
aura_lang_std_String_trim|i8*|i8*
aura_lang_std_Collections_emptyList|i8*|
aura_lang_std_Collections_listAppend|i8*|i8*,i8*
aura_lang_std_Collections_count|i64|i8*
aura_lang_std_Collections_getAt|i8*|i8*,i64
aura_lang_std_Collections_listSet|void|i8*,i64,i8*
aura_lang_std_Collections_filter|i8*|i8*,i8*
aura_lang_std_Collections_map|i8*|i8*,i8*
aura_lang_std_Collections_take|i8*|i8*,i64
aura_lang_std_Math_sin|double|double
aura_lang_std_Math_cos|double|double
aura_lang_std_Math_sqrt|double|double
aura_lang_std_Math_pow|double|double,double
```

**目标**：移除这些映射。这些函数将由 Aura 源码编译，不再需要 `declare`。

**注意**：`preludeTable()` 中还有非 std 的函数（`aura_println`, `aura_print`, `aura_to_str` 等），这些可以保留或按同样方式 Aura 化。

#### B.2 AOT 发射器改造：std 调用走 Aura 编译

**改造点**（`Emit.aura`）：

1. **移除 std 函数名到 C 符号的映射**：`preludeTable()` 不再包含 `aura_lang_std_*` 条目
2. **通过 `funcSignatureOf` 查找 Aura 编译产物**：std 函数作为普通用户函数编译
3. **仅对真正的外部函数生成 `declare`**：`@native` 函数 + 系统 libc 函数

**调用路径变化**：
```
当前：s.length → emitCall → preludeTable() → declare @aura_lang_std_String_length → C 实现
目标：s.length → emitCall → funcSignatureOf → Aura 编译的 String.length 函数 → 直接调用
```

#### B.3 ModuleLink 改造：自动包含 std 模块

**当前**：ModuleLink 仅包含用户源码的 import 依赖。

**目标**：当用户代码使用 `String`/`FileSystem`/`Process` 等 std 类时，自动包含对应的 std 模块。

```
用户代码 import aura.lang.std.String
  → ModuleLink 解析 → 包含 aura/core/aura/lang/std/String.aura
  → String.aura import aura.lang.native.file.FileOps
  → ModuleLink 解析 → 包含 aura/core/aura/lang/native/file/FileOps.aura
  → FileOps.aura 含 @native(SYS_OPEN) 等声明
  → AOT 发射器为 @native 生成内联 syscall/asm IR
```

**改造点**（`ModuleLink.aura`）：
- 确保 std 模块的 import 路径解析正确（`aura/core/aura/lang/std/*.aura`）
- 确保 native 模块的平台分叉正确（按目标三元组选择 `arch/<platform>/Syscalls.aura`）

---

### Phase C：移除 AOT 构建中的 C 编译步骤

#### C.1 `Aot.aura:aotBuildExeFromHir` 改造

**移除步骤**：
```
// 移除第 4 步：clang -c aura_std_cffi.c
// 移除第 4.5 步：clang -c aura_syscalls.c
```

**保留步骤**：
```
1. llc -verify-each .ll           ← IR 验证
2. llc -mtriple ... -filetype=obj ← IR → 目标文件
3. clang .obj -o exe               ← 链接（仅链接，无 C 编译）
```

#### C.2 链接命令简化

**当前**：
```
clang user.obj aura_std_cffi.obj aura_syscalls.obj -o user.exe
```

**目标**：
```
clang user.obj -o user.exe
```

`user.obj` 已包含所有代码（用户代码 + Aura std 库 + native 内联汇编），链接器自动解析系统 libc 函数（malloc/free/printf 等）。

#### C.3 `AotExeResult` 结构变更

移除字段：
```
cffiExit: Int          ← C FFI 编译退出码
syscallsExit: Int      ← syscalls 编译退出码
```

`ok` 判定简化：
```
// 当前
r.ok = (r.llcExit == 0) && (r.cffiExit == 0) && (r.syscallsExit == 0) && (r.clangExit == 0)

// 目标
r.ok = (r.llcExit == 0) && (r.clangExit == 0)
```

#### C.4 构建脚本简化

**`build-aura-compiler.ps1` 变更**：
- 移除 `clang -c aura_std_cffi.c` 步骤
- 移除 `clang -c aura_syscalls.c` 步骤
- 移除 `-I compiler/src/std/cffi` 包含路径

**`self-bootstrap.ps1` 变更**：
- 无需修改（自举流程不变，仅底层实现变化）

---

### Phase D：异常处理（setjmp/longjmp）替代

#### D.1 当前方案

```
aura_syscalls.c:
  void *aura_exception_value = NULL;
  int aura_setjmp(void *buf) { return setjmp(buf); }
  void aura_longjmp(void *buf, int val) { longjmp(buf, val); }
```

`Emit.aura` 中 try/catch 生成：
```
call void @aura_setjmp(i8* %buf)       ← 保存跳转上下文
call void @aura_longjmp(i8* %buf, i32)  ← 非局部跳转
```

#### D.2 目标方案

使用 LLVM 内建异常处理机制（invoke + landing pad）替代 setjmp/longjmp。

**LLVM IR 结构**：
```llvm
; try 块
try:
  %result = invoke i32 @mayThrow(...)
              to label %normal unwind label %catch

; 正常路径
normal:
  ...
  ret void

; 异常路径（landing pad）
catch:
  %ex = landingpad { i32, i8* }
            cleanup
  %isnull = extractvalue { i32, i8* } %ex, 0
  %unwind = icmp ne i32 %isnull, 0
  %sel = select i1 %unwind, i8* null, i8* %unwind
  ...
  ret void
```

**改造点**：
1. `Emit.aura:emitTryCatch` 使用 LLVM `invoke` + `landingpad`
2. 移除 `aura_setjmp`/`aura_longjmp` 声明
3. 移除 `aura_exception_value` 全局变量
4. 异常对象通过 `landingpad` 的 `{ i32, i8* }` 结构传递

**风险**：LLVM landing pad 与 setjmp/longjmp 语义不同——setjmp 可多次调用且可跨越函数边界，landing pad 仅限 `invoke` 的异常传播。需仔细对齐语义。

---

### Phase E：冻结引导二进制 + 构建流程

#### E.1 发行包结构（最终形态）

```
build/bin/
├── aura-compiler.exe              ← 冻结引导二进制（含完整 Aura 编译器）
└── SHA256SUMS                     ← 校验和

用户环境需求：
  • aura-compiler.exe    ← 发行包内（无需 Rust）
  • llc                  ← LLVM 工具
  • clang                ← LLVM 工具（仅链接模式，无需 C 编译器）
  • 系统 CRT             ← 操作系统自带（libc/kernel32.dll）
```

#### E.2 构建流程

**开发环境**（需要 Rust + C 编译器，仅一次）：
```
cargo build → aura.exe（Rust 编译器）
aura.exe build Main.aura --aot → aura-compiler-native.exe（新载体）
clang -c aura_std_cffi.c -o aura_std_cffi.obj（运行库，仅开发时需要）
clang -c aura_syscalls.c -o aura_syscalls.obj（运行库，仅开发时需要）
验证行为一致性 → 冻结二进制 + 计算 SHA-256
```

**用户环境**（仅需 LLVM，每次编译）：
```
aura-compiler.exe user.aura -o user.exe
  → ModuleLink: user.aura + std 模块 → 单一 HIR
  → Emit: HIR → LLVM IR（含内联 syscall/asm，无 C 运行库引用）
  → llc -verify-each user.ll → 验证
  → llc -mtriple ... user.ll -o user.obj → 目标文件
  → clang user.obj -o user.exe → 链接（系统 CRT 自动解析）
```

---

## 4. C 运行库函数到 Aura 实现的映射

| C 函数（aura_std_cffi.c / aura_syscalls.c） | 行数 | Aura 实现位置 | 底层能力 | 实现方式 |
|----------------------------------------------|------|--------------|---------|---------|
| `aura_lang_std_String_length` | 3 | `String.length` (Aura) | 字符串内建 | 纯 Aura 逻辑 |
| `aura_lang_std_String_contains` | 5 | `String.contains` (Aura) | 字符串遍历 | 纯 Aura 逻辑 |
| `aura_lang_std_String_substring` | 10 | `String.substring` (Aura) | malloc + 拷贝 | Aura 逻辑 + declare malloc |
| `aura_lang_std_String_charCodeAt` | 3 | `String.charCodeAt` (Aura) | 字符串索引 | 纯 Aura 逻辑 |
| `aura_lang_std_String_toUpperCase` | 20 | `String.toUpperCase` (Aura) | 字符映射 | 纯 Aura 逻辑 |
| `aura_lang_std_Collections_emptyList` | 5 | `Collections.emptyList` (Aura) | 无 | 纯 Aura 逻辑 |
| `aura_lang_std_Collections_listAppend` | 15 | `Collections.listAppend` (Aura) | malloc | declare malloc |
| `aura_lang_std_Collections_count` | 3 | `Collections.count` (Aura) | 无 | 纯 Aura 逻辑 |
| `aura_lang_std_Collections_getAt` | 5 | `Collections.getAt` (Aura) | 数组索引 | 纯 Aura 逻辑 |
| `aura_lang_std_Collections_filter` | 30 | `Collections.filter` (Aura) | malloc + 闭包 | Aura 逻辑 + declare malloc |
| `aura_lang_std_Collections_map` | 30 | `Collections.map` (Aura) | malloc + 闭包 | Aura 逻辑 + declare malloc |
| `aura_lang_std_Math_sin` | 1 | `MathOps.sin` (Aura) | 无（Taylor 级数） | 纯 Aura 逻辑 |
| `aura_lang_std_Math_sqrt` | 1 | `MathOps.sqrt` (Aura) | 无（牛顿法） | 纯 Aura 逻辑 |
| `aura_lang_std_Math_pow` | 1 | `MathOps.pow` (Aura) | 无（二进制幂） | 纯 Aura 逻辑 |
| `aura_lang_std_FileSystem_readText` | 50 | `FileSystem.readText` (Aura) | `@native(SYS_READ)` | LLVM inline asm (syscall) |
| `aura_lang_std_FileSystem_writeText` | 50 | `FileSystem.writeText` (Aura) | `@native(SYS_WRITE)` | LLVM inline asm (syscall) |
| `aura_lang_std_Process_run` | 30 | `Process.run` (Aura) | `@native(SYS_FORK)`+`@native(SYS_EXECVE)` | LLVM inline asm (syscall) |
| `aura_memory_alloc` | 3 | `Allocator.alloc` (Aura) | declare malloc | 系统 libc |
| `aura_memory_free` | 3 | `Allocator.free` (Aura) | declare free | 系统 libc |
| `aura_cpu_rdtsc` | 3 | `Cpu.rdtsc` (Aura) | `@native(asm="rdtsc")` | LLVM inline asm |
| `aura_cpu_mem_fence` | 3 | `Cpu.memFence` (Aura) | `@native(asm="mfence")` | LLVM `fence` 指令 |
| `aura_cpu_atomic_add` | 5 | `Cpu.atomicAdd` (Aura) | `@native(asm="lock xaddq")` | LLVM `atomicrmw` |
| `aura_syscall_dispatch` | 80 | `Syscalls.read/write/...` (Aura) | `@native(N)` | LLVM inline asm (syscall) |
| `aura_setjmp` | 5 | `Exception` (Aura) | LLVM `invoke`/`landingpad` | LLVM 异常处理 |
| `aura_longjmp` | 3 | `Exception` (Aura) | LLVM `resume` | LLVM 异常处理 |
| `aura_println` | 5 | `Console.println` (Aura) | `@native(SYS_WRITE)` | LLVM inline asm (syscall) |
| `aura_to_str` | 20 | `String.toString` (Aura) | malloc | Aura 逻辑 + declare malloc |
| `aura_io_fileRead` | 50 | `Stdio.readFile` (Aura) | `@native(SYS_READ)` | LLVM inline asm (syscall) |
| `aura_lang_std_StringBuilder_create` | 10 | `StringBuilder` (Aura) | malloc | declare malloc |
| `aura_lang_std_Process_argCount` | 3 | `Process.argCount` (Aura) | `@native(SYS_GETPARENT)` | LLVM inline asm |

**统计**：
- 纯 Aura 逻辑（无外部依赖）：~15 个函数
- Aura 逻辑 + declare malloc：~10 个函数
- LLVM inline asm (syscall)：~8 个函数
- LLVM `fence`/`atomicrmw`：~3 个函数
- LLVM 异常处理：~2 个函数
- **总计**：~38 个函数，全部可由 Aura + LLVM IR 实现

---

## 5. 风险与工作量

### 5.1 工作量估算

| 改造项 | 工作量 | 风险 | 说明 |
|--------|--------|------|------|
| A.1 syscall 内联生成 | 3 人日 | 中 | 需按平台生成正确的寄存器序列；已有 `@native(asm=...)` 通用路径可复用 |
| A.2 rdtsc 直接 inline asm | 0.5 人日 | 低 | 移除特殊分支即可，通用路径已有 |
| A.3 fence 指令 | 0.5 人日 | 低 | LLVM `fence` 是标准指令 |
| A.4 atomicrmw 指令 | 1 人日 | 低 | LLVM `atomicrmw` 是标准指令，需 inttoptr |
| A.5 declare malloc/free | 0.5 人日 | 低 | 简单替换声明 |
| A.6 声明替换 | 0.5 人日 | 低 | 删除旧声明 + 添加新声明 |
| B.1 移除 preludeTable | 0.5 人日 | 低 | 删除代码 |
| B.2 发射器改造 | 3 人日 | 中 | 需确保 std 函数作为普通函数编译，调用路径正确 |
| B.3 ModuleLink 自动包含 | 1 人日 | 低 | 已有递归 import 解析，只需确保 std 模块被包含 |
| C.1-C.3 构建流程简化 | 1 人日 | 低 | 删除步骤 |
| D.1-D.2 异常处理 | 3 人日 | 高 | LLVM landing pad 与 setjmp/longjmp 语义不同，需仔细对齐 |
| E.1-E.2 冻结二进制 | 1 人日 | 低 | 已有 65 代自举产物验证 |
| 测试验证 | 5 人日 | 中 | 需覆盖所有 std 函数 + 异常处理 + 并发 |
| **总计** | **~20 人日** | | |

### 5.2 风险矩阵

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| syscall 寄存器映射错误 | 中 | 高 | 按平台分别测试；参考 OS 内核文档；现有 `@native(asm=...)` 路径可复用 |
| std 函数编译后调用路径错误 | 中 | 高 | 保留 `preludeTable()` 作为 fallback（feature flag）；逐函数迁移 |
| 异常处理语义不一致 | 中 | 中 | 先保留 setjmp/longjmp 作为 fallback；LLVM landing pad 路径逐步替换 |
| Windows 平台 syscall 不支持 | 高 | 中 | Windows 已用 `@native(asm="call qword [rip + X]")` 直接调用，不受影响 |
| 交叉编译目标平台缺失 | 低 | 中 | 按目标三元组选择平台特定的 `Syscalls.aura` 模块 |
| malloc/free 在不同平台的符号差异 | 低 | 低 | Windows: `@malloc`; Linux: `@malloc`; 按目标三元组适配 |
| 性能回归（inline asm 比 C 包装器慢） | 低 | 低 | LLVM `fence`/`atomicrmw` 比函数调用更快；syscall inline 无额外调用开销 |

### 5.3 分阶段交付

```
Phase 1（5 人日）：Phase A + Phase C
  • emitNativeWrapper 改造（syscall/asm/fence/atomicrmw/malloc）
  • 移除 C 编译步骤
  • 验证：简单程序（println + Math）可编译运行

Phase 2（5 人日）：Phase B
  • 移除 preludeTable C 映射
  • ModuleLink 自动包含 std 模块
  • 验证：String/FileSystem/Collections 功能完整

Phase 3（5 人日）：Phase D
  • 异常处理改造（setjmp/longjmp → landing pad）
  • 验证：try/catch 语义一致

Phase 4（5 人日）：Phase E + 全面测试
  • 冻结二进制
  • 65 代自举验证
  • 全部 std 函数覆盖
  • 发布发行包
```

---

## 附录：当前代码依赖全景

### A.1 `Aot.aura:aotBuildExeFromHir` 当前流程（第 99-199 行）

```
1. 落盘 LLVM IR                    → FileSystem.writeText
2. llc -verify-each                → Process.run("llc -verify-each ...")     ← LLVM 工具
3. llc -mtriple -filetype=obj      → Process.run("llc ... -filetype=obj")     ← LLVM 工具
4. clang -c aura_std_cffi.c        → Process.run("clang -c ...")              ← C 编译 ⚠️
4.5. clang -c aura_syscalls.c      → Process.run("clang -c ...")              ← C 编译 ⚠️
5. clang .obj ... -o exe           → Process.run("clang ... -o exe")          ← LLVM 链接
```

### A.2 `Emit.aura:emitNativeWrapper` 当前路由（第 1717-1825 行）

```
@native(N)              → call @aura_syscall_dispatch(N, args...)     ← C 包装器
@native(asm="rdtsc")    → call @aura_cpu_rdtsc()                      ← C 包装器
@native(asm="mfence")   → call @aura_cpu_mem_fence()                   ← C 包装器
@native(asm="atomic..") → call @aura_cpu_atomic_add(args)             ← C 包装器
@native(asm="其他")     → call asm sideeffect "..."                   ← LLVM inline asm ✅
@native（内置）         → call @aura_memory_alloc/free                ← C 包装器
```

### A.3 `Runtime.aura:preludeTable()` 当前映射（第 73-113 行）

```
40+ 个 aura_lang_std_* 符号 → C 运行库（aura_std_cffi.c）
```

### A.4 C 运行库规模

```
aura_std_cffi.c:  86KB, 2039 行, ~200 个函数
aura_syscalls.c:  37KB,  832 行, ~100 个函数
aura_std_cffi.h:  25KB,  366 行, 头文件
总计:             148KB, 3237 行
```

### A.5 Aura 已有对应实现规模

```
aura/core/aura/lang/std/      : 21 个 .aura 文件, ~180KB
aura/core/aura/lang/native/   : 15 个 .aura 文件（含 5 个平台分叉）, ~80KB
aura/compiler/aura/lang/compiler/aot/ : 15 个 .aura 文件, ~180KB
```

### A.6 已验证的自举产物

```
build/bin/
  aura-compiler-native.exe      : 681KB（Rust 编译的载体）
  aura-compiler-native2.exe     : 675KB（Aura 自举编译产物）
  aura-compiler-native2.ll      : 4.7MB（LLVM IR）
  aura-compiler-selfhost.exe    : 841KB（第 65 代自举）
  ...
  65 代连续自举产物，证明 Aura AOT 后端可编译编译器自身
```

---

> **文档状态**：方案分析完成，待实施
> **最后更新**：2026-09-16
> **相关文档**：
> - `docs/编译器LLVM交互分析与纯Aura化迁移计划.md` — 总迁移计划
> - `docs/pure_aura/完全Aura化技术方案-v3.0.md` — 完全 Aura 化架构
> - `docs/pure_aura/aot_pure_aura_stdlib.md` — AOT 纯 Aura 标准库设计
> - `docs/绕过LLVM直生成机器码-可行性评估与技术方案.md` — 替代方案对比
