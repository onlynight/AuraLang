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

## 6. 实施进度追踪（2026-09-16）

### 6.1 已完成

| Phase | 改造项 | 状态 | 代码位置 |
|-------|--------|------|---------|
| A.1 | `@native(N)` → 内联 syscall asm（平台特定寄存器映射） | ✅ | `Emit.aura:1737-1767` |
| A.2 | `@native(asm="rdtsc")` → 通用 inline asm 路径（移除特殊分支） | ✅ | `Emit.aura:1792-1836` |
| A.3 | `@native(asm="mfence")` → LLVM `fence seq_cst` | ✅ | `Emit.aura:1772-1777` |
| A.4 | `@native(asm="atomic")` → LLVM `atomicrmw add` | ✅ | `Emit.aura:1779-1791` |
| A.5 | `@native` 内置（alloc/free）→ `@malloc`/`@free` | ✅ | `Emit.aura:1842-1849` |
| A.6 | Runtime 声明替换（系统 libc 声明） | ✅ | `Runtime.aura:127-142` |
| C.1 | 移除 `aura_syscalls.c` 编译步骤 | ✅ | `Aot.aura:165-170` |
| C.3 | 移除 `syscallsExit` 字段 + 简化 `ok` 判定 | ✅ | `Aot.aura:188, 416-421` |
| B.3 | ModuleLink 自动包含 std 模块 | ✅ | `ModuleLink.aura:53-76, 211-232` |
| B.3b | object 方法（静态方法）receiver 修复 | ✅ | `Emit.aura:152-155, 343-345, 1408-1415, 1457-1462` |
| B.2a | 移除 `isStdClassName` 检查（emitCall + inferRetTy） | ✅ | `Emit.aura:4282, 4978` |
| B.3c | HIR Package 声明处理修复 | ✅ | `Hir.aura:538-544` |
| E.1 | 冻结引导二进制脚本（freeze-bootstrap.ps1） | ✅ | `scripts/freeze-bootstrap.ps1` |
| E.2 | SHA256SUMS 生成 + 发行包布局 | ✅ | `scripts/freeze-bootstrap.ps1` |
| D.1a | LLVM 异常处理基础设施声明（personality 函数） | ✅ | `Runtime.aura:143-152` |

### 6.2 进行中

| Phase | 改造项 | 状态 | 说明 |
|-------|--------|------|------|
| B.1 | 移除 `preludeTable()` 中的 String/Math `aura_lang_std_*` 映射 | ✅ 已移除 | `Runtime.aura:preludeTable` 中 String C 映射已注释；StdSigs.aura 保留（发射器仍需） |
| B.2 | 发射器改造：std 调用走 Aura 编译 | ✅ 完成 | **Step 1-3 已完成**：4 处 std 调用路径已添加 HIR 签名表优先检查；Step 4 @init bug 已修复 |
| B.2a | `emitCall` stdClassReceiver 路径 | ✅ | `Emit.aura:4477-4491` 优先查 `funcSignatureOf` |
| B.2b | `emitCall` methodCallSymbol 路径（Collections 重写） | ✅ | `Emit.aura:4425-4452` 优先查 `funcSignatureOf` |
| B.2c | `emitCall` methodCallSymbol 路径（裸方法名） | ✅ | `Emit.aura:4499-4530` 优先查 `funcSignatureOf` |
| B.2d | `inferRetTy` stdClassReceiver 路径 | ✅ | `Emit.aura:5170-5176` 优先查 `funcSignatureOf` |
| B.2e | `inferRetTy` methodCallSymbol 路径 | ✅ | `Emit.aura:5197-5213` 优先查 `funcSignatureOf` |
| C.2 | 链接命令简化（移除 `aura_std_cffi.c`） | ⚠️ 待 Phase B 完成 | 当前仍保留 `aura_std_cffi.c` 编译（Aot.aura:157-163） |
| D.1b | emitTry → LLVM invoke/landingpad/resume | ⚠️ 待实现 | 当前仍用 setjmp/longjmp；LLVM `invoke` 需将 try 体包装为函数调用 |

### 6.3 待开始

| Phase | 改造项 | 状态 |
|-------|--------|------|
| D.2 | 移除 `aura_setjmp`/`aura_longjmp` 声明 | ❌ 待 D.1b 完成 |
| D.3 | 移除 `aura_exception_value` 全局变量 | ❌ 待 D.1b 完成 |
| E.3 | 发行包自动化（CI 集成） | ❌ 待开始 |

### 6.4 关键发现

1. **HIR Package 声明处理**：`lowerDecl` 函数不处理 "Package" 类型，导致含 `package` 声明的文件（如 Math.aura）在 HIR 降级时异常。已修复为跳过 Package 节点（与 Import 相同）。
2. **object 方法 receiver 问题**：object 内的方法（静态方法）被错误地添加 `%struct.<Class>*` receiver 参数。已修复：新增 `fObjectClassNames` 集合，object 方法不再添加 receiver。
3. **Rust 编译器 resolve_method_owner**：当多个类共享方法名（如 `noKids` 在 AstUtils 和 MirUtils 中）且 sema 信息不可用时，`resolve_method_owner` 返回 None，导致 HIR 产生裸名 callee + phantom receiver。已修复：`emit_call` 中检测裸名 callee + phantom receiver 并自动补全类名前缀。
4. **发射器 std 调用路径**：`stdSymbolFor` 函数无条件为 std 类方法生成 C 符号名（如 `aura_lang_std_String_length`），即使该函数已作为 Aura 编译。**已修复**：4 处调用路径（`emitCall` 的 `stdClassReceiver`、`methodCallSymbol`×2；`inferRetTy` 的 `stdClassReceiver`、`methodCallSymbol`）均已添加 HIR 签名表优先检查，std 函数存在 HIR 时走 Aura 路径。
5. **编译器自举限制**：`aura.exe build Main.aura --aot` 可编译（Rust 编译器），但 self-compiled compiler (`aura-compiler-native2.exe`) 编译 Main.aura 时崩溃（访问违例）。需先解决自举问题后才能验证 Phase B 的完整效果。
6. **value class 方法调用**：String 是 value class，非 struct，故 `isStructSym("String")` 返回 false，path 2（静态方法）无法命中。**已修复**：在 `inferType` 中新增 `i8*` receiver 检查，直接查 `funcSignatureOf("String_" + fname)`。
7. **functionSignature 命名不一致**：`functionSignature` 生成 "Class.method"（含 `.`），但 `emitFunction` 的 `sanitizeLlvm` 将 `.` 替换为 `_`，导致签名表查不到函数。**已修复**：`functionSignature` 改用 `_` 分隔（`Class_method`），11 处 `funcSignatureOf` 调用同步更新。
8. **预存 bug：String.toFloat 字符转换**：AOT 编译时 `String.toFloat()` 中 `(c - '0').toFloat()` 的 `inferType` 返回 `i32`（默认值），导致 `%digit.addr = alloca i32` 但存储 `float`。**已修复**：`i8*` receiver 检查 + `methodCallSymbol` 新增 String 转换方法（toFloat/toInt/toLong/toDouble/toBoolean 等）。验证：`%digit.addr = alloca float`（正确），`sign.toFloat()` 无双重 `sitofp` 转换。
9. **预存 bug：@init 调用缺少类前缀**：`Runtime_init1` 函数体内调用 `@init` 而非 `@Runtime_init1`。原因：`init` 方法未在 `methodCallSymbol` 中登记，`emitCall` 按自由函数处理。**已修复**：`ownerClassInChain` 对 `init` 增加 `funcSignatureArity` 后缀匹配；`emitCall` path 3 和 `inferRetTy` 的 2 条路径同步修复；`emitCall` path 1（类实例接收者）也增加 init 后缀处理。注意：Rust 后端 HIR 已将 `init` 改为 `__ctorN`，此 bug 仅在纯 Aura 自举路径触发。

### 6.5 下一步计划

```
Step 1：✅ 改造发射器 std 调用路径（4 处路径均已修复）
  - emitCall/stdClassReceiver (Emit.aura:4477-4491)
  - emitCall/methodCallSymbol-Collections重写 (Emit.aura:4425-4452)
  - emitCall/methodCallSymbol-裸方法名 (Emit.aura:4499-4530)
  - inferRetTy/stdClassReceiver (Emit.aura:5170-5176)
  - inferRetTy/methodCallSymbol (Emit.aura:5197-5213)
Step 2：✅ 修复预存 bug：functionSignature 命名不一致（`.` → `_`）
  - functionSignature 函数生成 "Class_method"（与 emitFunction 的 sanitizeLlvm 一致）
  - 11 处 funcSignatureOf 调用同步更新
  - i8* receiver 检查：String 方法（value class）接收者类型为 i8*，非 %struct.String*
Step 3：✅ 验证 String.toFloat 字符算术类型推断 bug 修复
  - `%digit.addr = alloca float`（原为 i32，现正确）
  - `sign.toFloat()` 无双重 sitofp 转换（原 bug 已修复）
Step 4：✅ 预存 bug：@init 调用缺少类前缀（独立于 Phase B）
  - `ownerClassInChain` 无法解析 `init` 重载（签名表为 `ClassName_init1`/`init2` 等，
    但查找的是 `ClassName_init`）
  - 已修复：`ownerClassInChain` 对 `fname == "init"` 增加 `funcSignatureArity` 后缀匹配
  - `emitCall` path 3（类体内裸方法名）：`init` 时用 `funcSignatureArity` 取签名并拼带后缀符号名
  - `emitCall` path 1（类实例接收者）：同样增加 init 后缀处理
  - `inferRetTy` 的 stdClassReceiver 和 classInstanceReceiver 路径：同步修复
  - 注：Rust 后端 HIR 已将 `init` 改为 `__ctorN`，此 bug 仅在纯 Aura 自举路径触发
Step 5：✅ 移除 preludeTable 中的 String 条目（已验证）
  - `Runtime.aura:preludeTable()` 中 String C 映射已注释移除（B.1 完成）
  - 发射器 4 处 std 调用路径已改造（B.2 完成），String 函数走 Aura 编译
  - 注：Math 条目在 `cffiTable()` 中（非 `preludeTable()`），单独处理
  - 注：Collections/Math 等其他 C FFI 条目的移除需先解决泛型类型实例化问题
    （`HashMap_Int_Int_` 未定义）— 见自举限制 #5
Step 6：Phase B 完成后移除 aura_std_cffi.c 编译步骤（Phase C.2）
  - 依赖：Math/Collections 等 std 函数需全部走 Aura 编译
  - 当前阻塞：自举编译失败（StringBuilder_create 重复定义 + HashMap_Int_Int_ 未定义类型）
Step 7：Phase D.1b：实现 LLVM invoke/landingpad/resume 异常处理
Step 8：Phase D.2-D.3：移除 setjmp/longjmp 声明 + aura_exception_value
Step 9：解决编译器自举问题（StringBuilder_create 重复定义 + HashMap_Int_Int_ 未定义类型）
Step 10：运行 freeze-bootstrap.ps1 验证完整自举流程
Step 11：Phase E.3：CI 集成 + 发行包自动化
```

---

> **文档状态**：Phase A 完成 + Phase C 部分完成 + Phase B 进行中（发射器 4 处 std 调用路径已改造，命名一致性修复，String.toFloat 类型推断修复；@init bug 已修复；Runtime.aura 三张符号表已清空，C FFI 声明已移除）
> **当前阻塞**：自举编译失败（StringBuilder_create 重复定义 + HashMap_Int_Int_ 泛型类型未定义）— 修复后可继续 Phase B/C 完全验证
> **最后更新**：2026-09-16
> **相关文档**：
> - `docs/编译器LLVM交互分析与纯Aura化迁移计划.md` — 总迁移计划
> - `docs/pure_aura/完全Aura化技术方案-v3.0.md` — 完全 Aura 化架构
> - `docs/pure_aura/aot_pure_aura_stdlib.md` — AOT 纯 Aura 标准库设计
> - `docs/绕过LLVM直生成机器码-可行性评估与技术方案.md` — 替代方案对比

---

## 7. 本轮修复记录（2026-09-16 续）

### 7.1 结果

| 验证项 | 结果 |
|--------|------|
| `aura.exe build aura/compiler/.../Main.aura --aot`（Stage-1，Rust → 原生载体） | ✅ 通过，产出 `build/bin/aura-compiler-native.exe` |
| `examples/language-test/` 全部用例（AOT 编译 + 运行） | ✅ 25 passed / 0 failed |
| `tests/hashmap_impl_test.aura`（VM） | ⚠️ 能跑完不再死循环；剩余 3 个断言失败 + T18 接口调用报错（见 7.3） |

### 7.2 已修复问题（按根因）

| # | 问题 | 根因 | 修复位置 |
|---|------|------|---------|
| 1 | AOT/自举报 `use of undefined value '@shr' / '@shl'` | Rust 解析器把保留运算符**单词形式**（`shr`/`shl`/`xor`/`ushr`）当「自定义中缀调用」，`h shr 16` 降级为 `Call("shr")`；且 `ushr` 未登记 | `compiler/src/parser.rs`（`try_parse_infix_call` 排除保留词；补 `ushr` 映射） |
| 2 | Aura 侧（Stage-2）无法解析位运算 | `Parser.aura:binPrec` 缺 `Ampersand`/`Caret`/`Pipe`；`Hir.aura:hirNormBinOp` 不归一化 `&`/`|`/`^`；`Emit.aura` 无位运算分支 | `aura/compiler/.../parser/Parser.aura`、`hir/Hir.aura`、`aot/Emit.aura` |
| 3 | 标准库哈希混合静默算错（`and` 被当逻辑与、`xor`/`shr` 变未定义调用） | 单词形式在两侧后端语义不一致 | `HashMap.aura`/`PlanA.aura`/`Encoding.aura`/`Signing.aura` 改用符号形式 `^`/`>>`/`<<`/`&` |
| 4 | 链接期 `undefined symbol: hashCode / equals` | `Any` 基类方法只有 `declare` 无 `define`（C 运行库亦未实现），且 `Any` 未参与编译 | `Any.aura` 给出**默认实现**（`hashCode`/`equals`/`toString`）；`codegen/mod.rs` 无条件内联 `Any.aura`；`hir.rs` 新增 `any_base_method` 兜底；`Emit.aura` 的 `ModuleLink.essentialStdModules()` 追加 `Any.aura` |
| 5 | `Any` 的 open 方法被降级为虚调用 → 退化成裸符号 `@hashCode` | 接收者是 `Any` / 值类型 / 泛型形参（无 vtable） | `hir.rs`：解析到 `Any` 时**不走** `CallVirtual`，直接调用 `Any.<method>` |
| 6 | `val x: Any = 5` 装箱错误（`toStr` 打出 "2"） | `emit_store_converted` 缺少「整型 → 指针槽」分支，裸整数被当指针存入 | `codegen/aot/emit.rs`（按 Plan A `(v<<1)|1` 装箱） |
| 7 | `fs.add(true)` 生成 `i8* 1` 非法 IR | `coerce_val_to_i8ptr` 只处理 `i64`/`i32`，`i1` 落到兜底 | `codegen/aot/emit.rs`（改用 `is_int_ty` 覆盖全部整型宽度） |
| 8 | 同名 `declare` + `define` 冲突 | prelude 原生名与 Aura 实现同名 | `codegen/aot/emit.rs:emit_ffi` 跳过「模块内已有定义」的 extern 声明 |
| 9 | **HashMap 测试死循环、内存无上限增长（用户报告）** | `HashMap.containsKey` 的 `while (i >= 0)` **缺 `i = i - 1`**，每轮都执行 `valuesEqual`（内部 `toStr` 持续分配） | `HashMap.aura:containsKey` |
| 10 | `containsValue` 把已删除键的旧值算作存在 | 追加式历史链只看 `fs[i]`，未过滤被遮蔽的旧记录 | `HashMap.aura:containsValue`（增加 `isLatest`） |
| 11 | **未解析 callee 静默 `Call(0)`（= 入口 main）→ 自递归、内存吃满** | 字节码发射器 `fn_index.get(..).unwrap_or(0)` | `codegen/emit.rs`：未解析时打印 `error` 并改用 `u16::MAX`（运行期报「未定义函数」，不再递归） |
| 12 | `s.startsWith(p)` 等 String 实例方法在 VM 中未解析 | `std_string.rs` 以裸名注册了 VM 原生，但编译器 `PRELUDE_NAMES` 未同步登记 → MIR 误判为用户函数 | `compiler/src/std/decl.rs`（补 `contains/startsWith/endsWith/indexOf/lastIndexOf/replace/substringBefore/substringAfter/toLowerCase/toUpperCase`） |

### 7.3 已知遗留（下一步）

| # | 现象 | 定位方向 |
|---|------|---------|
| 1 | `var m: Map<K,V> = HashMap(...)` 后经接口变量调用方法（`put`/`isEmpty`/`getSize`）在 VM 侧未解析（现为明确报错，不再爆内存） | 接口类型接收者需要虚调用（`CallVirtual`）与 vtable 槽位支持；`build_class_table` 目前不登记 `Decl::Interface`（试登记后具体类型 `isEmpty` 回归为栈溢出，已回退，需先确认 `virtual_methods`/`METHOD_SLOTS` 的槽位来源） |
| 2 | `tests/hashmap_impl_test.aura` 仍失败：`kv.noDuplicateValues`、`tc.evenGoneOddKept`、`tc.keysCount` | 追加式历史链的 `isLatest`/遮蔽语义（`rehash` 目前只丢弃 `alive=false`，未丢弃被遮蔽记录） |
| 3 | 用户程序（非编译器自身）AOT 时报 `use of undefined type named 'struct.ArrayList'` | `ModuleLink`/Rust 链接器的 std 模块自动包含范围不足（`HashMap` 依赖 `ArrayList` 等未一并纳入） |
| 4 | Stage-2（Aura 侧发射器）仍缺 `aura_to_str_any`/`aura_strlen`/`aura_string_concat`/`aura_lang_std_Collections_*` 等运行期符号的 Aura 实现 | Phase B/C 收尾：这些符号目前仍是 `declare`，需要纯 Aura/native 落地 |

### 7.4 第二轮修复（遗留问题清理）

| # | 问题 | 根因 | 修复位置 |
|---|------|------|---------|
| 13 | 用户程序里 `xs.size` / `xs.getSize()` / `xs.isEmpty()` 取到 `null`（`toStr(xs.size)` 打印 "null"，但参与算术/比较又正常） | 接收者类型查不到时，`.size` 走「类访问器」路径 → 读 Aura 类 `ArrayList._size`，而运行期列表是 `Value::List`（**没有该字段**）。根因是 sema 类型通道对入口文件缺记录（构造器被推断成 `<error>`：`cannot initialize 'ArrayList' with '<error>'`） | `hir.rs`：新增 `is_list_like_type()`（含 `ArrayList`/`MutableList` 等 `starts_with("List")` 覆盖不到的名字）+ `LOCAL_TYPE_SCOPES` 声明类型兜底（变量/形参/for 变量），并把 6 处 list-like 判断统一 |
| 14 | `var m: Map<K,V> = HashMap(...)` 后经接口变量调用方法在 VM 未解析（运行期「未定义函数」） | 接口不参与类表，方法名退化为裸名 | `hir.rs`：Map 接口接收者按名字改派到 `HashMap.<method>`（沿用既有 `HashMap.get` 模式，扩展到 put/remove/keys/values/isEmpty/getSize/toString 等） |
| 15 | 逐出/删除后 `containsKey` 仍返回 true、`keys()` 数量翻倍 | `rehash` 只丢弃 `alive=false` 的墓碑，**保留了被遮蔽的旧存活记录**，墓碑一丢旧记录重新暴露 | `HashMap.aura:rehash`（只搬运 `isLatest` 的记录） |
| 16 | `HashMapUtils.mapOf/mutableMapOf/emptyMap` 返回空表 | vararg 打包只实现在**自由函数**调用路径；object/类方法路径只做默认参数填充，实参未打包 → `pairs` 收到第一个实参（字符串），`pairs.size` 恒 0 | `hir.rs`：在类/object 方法调用路径补 vararg 打包（`listOf(...)`，含 self 偏移） |
| 17 | 用户程序 AOT 报 `use of undefined type named 'struct.ArrayList'` | `HashMap` 的字段类型是 `ArrayList<...>`，但 `HashMap.aura` 未 import `ArrayList` → 类型被引用而定义未参与编译 | `HashMap.aura` 增加 `import aura.lang.collection.ArrayList` |
| 18 | `tests/hashmap_impl_test.aura` 的 `kv.noDuplicateValues` 误报 | **测试用例自身缺陷**：第二轮循环里 `b = a + 1` 引用了上一轮块作用域内声明的 `b`（出块即失效）。编译器把它静默当 `null`，`vs[null]` 取到 `vs[0]` → 误判重复 | 修测试（重新声明 `var b`）；编译器对「赋值给未声明名字」应报错，见 7.5 |

**验证结果**：`tests/hashmap_impl_test.aura`（VM）**157 passed / 0 failed**；语言测试 25/25；Stage-1 AOT 自举编译通过。

### 7.5 仍待解决（含精确定位）

| # | 现象 | 精确定位 |
|---|------|---------|
| A | 原生载体（Aura 侧编译器）编译 `_gen.aura` 时 llc 报 `'%var.N' defined with type 'ptr' but expected 'i32'` | `Emit.aura` 的**字符串拼接 / 值转字符串**路径把同一操作数**转换了两次**：`build/_gen2.ll:2293-2298` 先 `sext i32 → aura_to_str` 得到 `i8* %var.1137`，随后又对 `%var.1137` 发 `sext i32 %var.1137 to i64`（把已转成字符串的值当整数用）→ 寄存器含义与其记录的（i64）类型不一致。需在 `valueToStr` 中复用已转换结果、不再二次转换 |
| B | AOT 编译含 `HashMap<String,Int>` 的程序报 `insertvalue { i32, i1 } … i32 %var` 类型不符 | AOT 未做泛型实例化：`HashMap<K,V>` 的方法返回类型 `V?` 在定义侧落成 `i8*`，调用侧按 sema 的 `Int?`（`{i32,i1}`）构造 → 需要泛型化（mono）或返回类型统一按 `i8*` 处理 |
| C | `val/var x = <未声明名字赋值>`（超作用域/拼写错误）被静默当成 `null` | sema 应报「未解析引用」；当前静默 null 会制造极难排查的假象（本轮 #18 即是此坑） |
| D | Stage-2 运行期符号（`aura_to_str_any` / `aura_strlen` / `aura_string_concat` / `aura_lang_std_Collections_*` …）在 Aura 侧发射的 IR 中仍是 `declare` | Phase B/C 收尾：需由 Aura/native 模块提供定义（当前 Aura 侧 `Aot.aura` 已不再编译 C 运行库，故 Stage-2 目前无法链接） |

> **更新说明**：本轮以「恢复 Stage-1 自举编译 + 消除内存爆涨 + 清理遗留」为目标，并让 `Any` 基类方法按默认实现下沉到 `Any.aura`；HashMap 标准库（VM 路径）已全绿。

### 7.6 第三轮：回退提前整改 + 打通 Aura 侧 AOT 到 llc 通过

**回退的提前整改**（Aura 侧 AOT 因此产出缺符号可执行文件）：

| 文件 | 被删内容 | 处置 |
|------|---------|------|
| `aot/Aot.aura` | 「Phase 1 整改：移除 C FFI 编译步骤」——删掉了 `clang -c aura_std_cffi.c` 编译与链接 | **已回退**（恢复编译 + 链接 + `r.ok` 含 `cffiExit`）；Rust 侧 `codegen/aot/linker.rs::compile_std_cffi` 一直保留该步骤，两侧必须一致 |
| `aot/Runtime.aura` | `runtimeTable()` / `cffiTable()` / `preludeTable()` **三张签名表被清空为 `""`** | **已回退**（`git checkout` 恢复 HEAD：表 + 声明同源） |

**回退后仍须的微调**（表恢复后暴露的冲突）：

1. `runtimeDeclarations()` 里 `declare i32 @getpid()` 与 `native/**/Syscalls.aura` 的
   `@native(SYS_GETPID) fun getpid(): Int`（发射为 `define i32 @getpid()`）冲突 →
   llc `invalid redefinition of function 'getpid'`。**删除该声明**。
2. 补 `declare i8* @aura_lang_std_String_charAt(i8*, i64)` 与
   `declare i32 @aura_lang_std_String_equals(i8*, i8*)`：`preludeTable` 在 Phase B.1 移除了
   `aura_lang_std_String_*` 映射（预期改走 Aura `String_*`），但仍有调用点按 C 符号发射。

**Aura 侧发射器真实 bug（本轮修复）**：

| 现象（llc 报错） | 根因 | 修复 |
|------|------|------|
| `'%var.N' defined with type 'ptr' but expected 'i32'` | `inferType` 不知道 `toString(x)` / `toStr(x)` 返回 `i8*`（`emitCall` 已按字符串发射）→ 拼接时把**已经转好的字符串**当 i32 再 `sext` 转一次。字符串插值（`"$x"` → `toString(x)`，见 `Parser.parseStringInterp`）与手写 `toStr(...)` 都命中 | `Emit.aura::inferTypeUncached` 的 `HirCall` 分支补 `toString`/`toStr` → `i8*` |
| `'%var.N' defined with type 'i64' but expected 'i32'` | `__callClosure(closure, "retTy\|pTys", …)`（`Collections.filter/map` 的闭包调用）返回类型未推断，落到兜底 i32 | `inferTypeUncached` 补 `__callClosure` → sig 的 `retTy` |

**标准库侧解耦**（`extern object` 成员函数体不发射这一设计缺口的规避）：

- `Stdio.println` 不再调用 `Console.getNewlineBuffer()`（该成员带函数体但被当外部符号丢弃，
  且 C 运行库无此符号）→ 改为自建 1 字节 `\n` 缓冲（`nlBuf`），只依赖 `Allocator`/`Memory`
  与 `Console.writeStdout`（`@native` 包装器，可正常发射）。
- `FileSystem` 的 3 处 `FileOps.mkdirFile(...)` → `FileOps.mkdir(...)`：前者是带函数体的包装
  （被丢弃），后者是 `@native(SYS_MKDIR)` 成员（会发射包装器），语义等价。

**当前结果**：载体编译 `build/_tiny.aura`（`println("hi")`）**已通过 llc 的 IR 校验**
（此前卡在 `@aura_args_set` / `@getNewlineBuffer` / `@mkdirFile` / `@getpid` 重定义等一串问题）。

**新的精确阻塞点（下一步）**：llc 汇编阶段报
`<inline asm>:1:2: error: unknown use of instruction mnemonic without a size suffix`。
发射出的 asm 是 **Linux 专用**：
```llvm
call i64 asm sideeffect "mov rax, 1; syscall", "=r,{rdi},{rsi},…"   ; SYS_WRITE(Linux)
call void asm sideeffect ".intel_syntax noprefix; lock inc qword [rdi]", "r"(…)
```
而目标三元组是 `x86_64-pc-windows-msvc` → 需要像 Rust 侧那样做**平台感知的 syscall/ARC 发射**
（Windows 上应走 libc/Win32 或 `Nt*`，而非裸 `syscall`）。

> **仍待解决（不变）**：`extern object` 成员带函数体时一律不发射（应改为「仅无函数体/`@native` 成员按外部符号」）；AOT 泛型实例化 `Int?`；sema 对未声明名赋值应报错。

### 7.7 第四轮：平台感知（Windows 走 CRT 实现）+ 载体端到端打通

**目标**：修掉上一轮的阻塞点 —— 发射的 asm 是 Linux 专用，而目标三元组是 Windows。

**1) `@native(SYS_*)` 平台感知（`Emit.aura::winCrtWrapper`）**

`aura/core/aura/lang/native/Syscalls.aura` 的 `SYS_*` 是 **Linux 编号**，Windows 既不能
用这些编号、也不能用裸 `syscall`（需 Nt ABI + 内核句柄语义）；且 x86 目标默认按
**AT&T** 解析内联汇编，`mov rax, 1; syscall` 直接报
`unknown use of instruction mnemonic without a size suffix`。

现在 `emitNativeWrapper` 的 `isSyscall` 分支按目标分流：
* **Windows** → `winCrtWrapper` 按成员名映射到 CRT（UCRT，clang 默认链接）实现，
  即「平台对应的实现」：
  `_write` / `_read` / `_open` / `_close` / `_lseeki64` / `_fstat` / `_stat64` /
  `_access` / `_unlink` / `_mkdir` / `_rmdir` / `_getpid` / `exit`；
  参数按 C ABI 收敛（句柄/计数 `i32`、路径/缓冲 `i8*`）；无法映射的
  （`fork`/`execve`/`kill`/`clockGettime`/`getrandom`）返回 0 而非发射错误 asm。
* **其它平台** → 保留裸 syscall，但补上 `.intel_syntax noprefix`（否则同样报上述 asm 错）。

CRT 声明补进 `Runtime.aura::runtimeDeclarationsEx`（非 Windows 目标下未被引用，无害）。

**2) ARC 引用计数改用 LLVM 原子指令（平台无关）**

`native/Runtime.aura` 的 `@native(asm = "lock inc qword [rdi]")` 有两个问题：
1. Intel 语法缺 `ptr` 关键字 → llc 报 `Expected 'PTR' or 'ptr' token!`；
2. `=r` 输出约束下 `lock inc` 并不写该寄存器 → 返回值本就是未定义值。
现改为 `atomicrmw add/sub i64*, 1 seq_cst`（返回旧值，inc 取 `old+1`、dec 取 `old-1`），
彻底去掉这两段 asm。

**3) 补齐 C 运行库链接：`aura_syscalls.c`**

`Aot.aura` 之前只编译链接 `aura_std_cffi.c`，但异常桥
（`aura_setjmp` / `aura_longjmp` / `aura_exception_value`，try/catch 依赖）在
`aura_syscalls.c` 里。Rust 侧 `linker.rs::compile_std_cffi` 一直编**两个**文件，
两侧现已一致。

**4) define / declare 冲突过滤**

`runtimeDeclarationsEx(defs)` 新增 `defs` 参数（`Emit.aura::fDefNames`，由 HIR 中
`HirFunction` 的符号名累积）：`preludeTable` 渲染声明时跳过本模块已有 `define` 的
符号。否则编译器自身源码的 `fun toString(x: Any)` 与 `preludeTable` 的 C 符号
`toString` 冲突 → llc `invalid redefinition of function 'toString'`。

**结果（里程碑）**：

```
$ build/bin/aura-compiler-native.exe build/_tiny.aura -o build/_tiny.exe
✓ Executable generated: build/_tiny.exe
$ ./build/_tiny.exe
hello from carrier
```

**Aura 侧 AOT 全链路（Aura 编译器 → llc → clang → 可执行文件 → 运行）首次端到端打通**，
载体（自举编译器）已能独立产出可运行程序。

**Stage-3（载体编译编译器自身）**：失败点从「llc IR 类型错（行 9356）」推进到
`use of undefined type named 'struct.HashMap_Int_Int_'`（行 86）——
即下面的「AOT 泛型实例化」遗留项，成为下一步首要目标。

**回归**：语言测试 25/25 ✓；HashMap(VM) 157/0 ✓；Stage-1 ✓。

### 7.8 第五轮：泛型实例化（`struct.HashMap_Int_Int_`）——解析器根因修复

**现象**：载体编译编译器自身时 llc 报
`use of undefined type named 'struct.HashMap_Int_Int_'`，且发射出的
`%struct.HashMap` 定义里有一个**巨型垃圾字段类型**：

```
%struct.HashMap = type { i8*, %struct.ArrayList_ArrayList_K___________________bucketKeys…
   privatevarbucketValues_ArrayList_…________2__________privatevarcapacity_Int…, … }
```

**根因（解析器）**：`Parser.parseTypeName()` 解析类型实参时**只认单独的 `Gt`**：

```aura
while (!this.prsCheck("Gt") && !this.prsAtEnd()) { name = name + this.prsCurLit(); … }
```

词法器把 `>>` / `>>>` 合并成**单个** token（`GtGt` / `GtGtGt`）。
`ArrayList<ArrayList<K>>` 因此永远等不到闭合 `>` → 循环一路吞掉后续所有 token
（`bucketKeys`、后续字段、方法体…）当类型文本，产出上述垃圾类型名。

**修复**：
1. `Parser.parseTypeName()`：类型实参扫描改为**深度配平**，`GtGt`/`GtGtGt`
   分别折算 2/3 层闭合（与 `parseFunction` 的泛型参数跳过逻辑一致）。
2. `TypeMapperUtils.stripGenericArgs()` + `TypeMapper.map()`：泛型实例**擦除**到
   基类（`HashMap<Int, Int>` → `%struct.HashMap*`、`ArrayList<K>` →
   `%struct.ArrayList*`），与 Rust 侧一致（Rust 侧
   `%struct.HashMap = type {%struct.ArrayList*, …}`，方法接收者直接是 `i8*`）。
   保留实参会产生无 `= type {…}` 定义的合成类型名。

**结果**：该阻塞点消除，Stage-3 失败点从 `native2.ll:86` 推进到 `native2.ll:9526`。

**新阻塞点（下一轮）**：`use of undefined value '@arg'`。

* 位置：`define i8* @Process_arg(%struct.Process* %this, i32 %index)` 体内
  `%v = call i8* @arg(i32 0, i32 %index)`。
* 来源：`aura/core/aura/lang/std/Process.aura`（**兼容包装**）第 41 行
  `return NativeProcess.arg(index)`，其顶部是**别名导入**
  `import aura.lang.native.process.Process as NativeProcess`。
* 两个成因叠加：
  1. **别名未解析**：发射器没有任何 `as` 别名处理（parser 只把 `" as NativeProcess"`
  写进 Import 文本，见 `Parser.aura:411`，全链路无人消费），于是 `NativeProcess.arg`
  落到「未解析符号」兜底 → 裸 `@arg`；
  2. **同名对象冲突**：`native/process/Process.aura` 的 `extern object Process` 与
     `std/Process.aura` 的 `object Process` **同时进入同一模块**（前者来自
     `essentialStdModules`，后者来自编译器自身的 `import`），同名符号相互覆盖：
     extern object 的成员体被跳过，std 的 `Process_arg` 胜出 → 若把别名直接解析到
     `Process_arg` 反而变成**自递归**。
* 建议修法（需成组进行，属「extern object 语义」这一遗留项的收口）：
  a. 发射器支持别名表（`Import` 文本 → 真实点分名），解析 `X.m` 时先做别名替换；
  b. `extern object` **带函数体**的成员改为发射 `<Object>_<member>` **define**
     （只有无函数体/`@native` 成员才当外部符号），并把调用点从「裸成员名」切到
     `<Object>_<member>`；
  c. 消除 `std/*` 兼容包装与 `native/*` 实现的**同名对象**冲突（重命名其中一方，
     或让 `std` 包装指向 `ProcessOps` 这类不重名的 native 对象）。

### 7.9 第六轮：native Process 改名 + Stage-3 打通（自举里程碑）

**1) 按方案改名：native `Process` → `ProcessNative`（对象 + 文件）**

* `aura/core/aura/lang/native/process/Process.aura` → `ProcessNative.aura`（`git mv`），
  `extern object Process` → `extern object ProcessNative`；
* 同步更新引用：`ModuleLink.essentialStdModules()` 路径、
  `std/Process.aura`（**同时去掉 `as NativeProcess` 别名**，直接 import `ProcessNative`）、
  以及 5 个 toolchain 程序（`cli/Args`、`debugger/Main`、`debugger/Repl`、`loom/Main`、`lsp/Main`）。
* 效果：`std/Process`（兼容包装）与 native 实现的符号空间分离
  （`Process_*` / `ProcessNative_*`），消除了同名符号互相覆盖。

**2) `ProcessNative` 由 `extern object` 改为普通 `object`**

`extern object` 的成员一律按外部 C 符号处理（只 `declare`、**不发射函数体**），
而 `ProcessNative` 的高层成员（`arg` / `args` / `argCount` / `spawn` / `run` …）
是纯 Aura 组合实现，必须发射成 `ProcessNative_<member>` 的 `define`。
改为普通 `object` 后这些函数体正常发射，`std/Process.arg` 的
`ProcessNative.arg(index)` 直接解析到 `ProcessNative_arg` ✓（消除 `@arg`）。

**3) 三个文件 syscall 声明移入 `ProcessOps`（extern object）**

`openFile` / `closeFile` / `readSyscall` 只在 `ProcessNative`（现为普通对象）声明时
不再生成 native 包装器 → 调用点悬空为 `@openFile` / `@closeFile`（llc:
`use of undefined value '@closeFile'`）。把这三条 `@native(SYS_*)` 声明
补进 `ProcessOps`（仍是 `extern object`，其 `@native` 成员会发射
「符号名 = 成员名」的包装器），裸名调用即正确绑定 ✓。

**结果（自举里程碑）**：

```
$ build/bin/aura-compiler-native.exe aura/compiler/aura/lang/compiler/Main.aura \
      -o build/bin/aura-compiler-native2.exe
✓ Executable generated: build/bin/aura-compiler-native2.exe      （40+ 模块，31s）

$ build/bin/aura-compiler-native2.exe
Usage: aura-compiler <input.aura> [-o <output.exe>] [--mem-trace]
```

**Aura 侧编译器已能编译自身并产出可运行的原生可执行文件（Stage-3 ✓）**。

**Stage-4 的前置缺口（下一轮）**：Aura 侧产出的可执行文件在 Windows 下**读不到命令行参数** ——
`ProcessNative` 只实现了 Linux 的 `/proc/self/cmdline` 路径
（`ProcessNative.aura:176/201/243`），没有平台分支，于是 `argc` 恒为 0
（`native2.exe` 只打印 usage，无法继续编译下一代）。

好消息：C 运行库**已经提供跨平台访问器**（由生成的 C `main` 调用的
`aura_args_set(argc, argv)` 注入后读取）：

* `int64_t aura_process_argCount(void)`
* `const char *aura_process_arg(int64_t index)`
* `const char *aura_process_args(void)`

Rust 侧就是用它们实现 `Process.arg/argCount` 的。下一步：在 `ProcessNative`
中按 `EnvOps.platform() == "windows"` 分支改走这三个访问器（在
`Runtime.aura::runtimeDeclarations` 里补 `declare`，并以 extern object 的
无函数体成员暴露，使调用点发射同名 C 符号）；非 Windows 保留 `/proc/self/cmdline`。

**回归**：语言测试 25/25 ✓；HashMap(VM) 157/0 ✓；载体最小程序编译+运行 ✓；
Stage-1/Stage-2（`native2`）✓。

### 7.10 第七轮：Stage-4 前置缺口 —— 命令行参数（进展与新的精确阻塞点）

**1) 去掉 Linux 专有的 `/proc/self/cmdline`，改走 C 运行库的跨平台访问器**

* `ProcessOps` 增加无函数体成员声明（extern object，符号名即成员名）：
  `aura_process_argCount()` / `aura_process_args()`（C 侧由 `aura_args_set` 注入的
  宿主 argv 读取，Rust 侧实现 `Process.arg/argCount` 用的就是它们）；
* `Runtime.aura::runtimeDeclarations` 补对应 `declare`；
* `ProcessNative.argCount/arg` 改走它们（`/proc/self/cmdline` 解析代码保留但不再调用）。

**2) `std/Process` 的 `val` → `fun`（关键 bug）**

`object` 的 `val` 在 AOT 下被当作**实例字段**（读出来是默认值、初始化式不会在访问时求值），
于是 `val argCount: Int = ProcessNative.argCount()` 让 `Process.argCount()` 被编译成
**常量 0**（IR: `icmp sge i32 0, 2`）——自举产物因此永远走「无参数」分支。
`args` / `argCount` / `pid` 已全部改为 `fun`（调用点本来就写成 `xxx()`）。
修复后 IR 变为 `call i32 @Process_argCount(...)` → `icmp sge` ✓，实测
`argc=3`（`build/_args.exe AAA BBB`）。

**3) 新的精确阻塞点：extern object 成员调用的「接收者占位」错位 + String 方法接收者丢失**

* **接收者占位**：extern object 的成员调用点会在实参**前**补一个占位
  （IR: `call i8* @f(i64 0, i32 %index)`）。因此逐条访问器
  `aura_process_arg(index)` 的索引落到第 2 个参数槽，而 C 只认第 1 个 →
  `arg(i)` 恒返回 argv[0]。已改为取 `aura_process_args()`（无参、占位无害）
  后在 Aura 侧切分。
* **String 方法接收者丢失（真实发射器 bug）**：`all.split("\n")` 被发射成

  ```llvm
  %all = call i8* @aura_process_args(i32 0)
  %parts = call i8* @String_split(%struct.String* null, i8* %sep)   ; ← 接收者是 null！
  ```

  接收者表达式被求值后**丢弃**，改传 `null`；`String_split` 解引用 null →
  段错误（`0xC0000005`）。这也解释了 Aura 侧产物（`native2.exe`）带参数运行崩溃：
  产物自身的字符串方法调用都会踩到该路径。

**下一步**：修 `emitCall` 中「String（`i8*`）接收者方法调用」的发射 ——
接收者类型应按 `i8*` 传递实际值（`%all`），而不是 `%struct.String* null`；
`String_<method>` 的查找与实参拼接需同时对齐。修好后 `Process.arg(i)`（split 版）
与 `native2` 自身的字符串处理即可正常工作，Stage-4（用自举产物编译下一代）才能继续。

### 7.11 第八轮：接收者类型不匹配引发失控分配（已回退，附正确修法）

**尝试与结果**：按 §7.10 的思路直接「把值接收者作为第 1 个实参传入」后，
编译产物的调用变成：

```llvm
; 定义（Aura 侧编译产物）
define i8* @String_split(%struct.String* %arg.this, i8* %arg.separator)
; 调用（修复后）
%var.287 = call i8* @String_split(%struct.String* %var.285, i8* %var.286)
```

接收者确实传进去了，但**类型错**：Aura 侧 String 的**值表示是 `i8*`（C 字符串）**，
而方法签名表里接收者是 `%struct.String*`。`%var.285`（字符缓冲指针）被当作
`%struct.String*` 传入后，`String_split` 按结构体布局读取长度字段 → 得到垃圾值 →
内部循环失控并**持续分配内存** → 实测把机器内存耗尽（系统崩溃）。

> 对比：回退前传 `null` 是「接收者丢失 → 立即解引用 null 崩溃」，
> 属于**快速失败**；传错类型的指针则是**无界循环 + 无界分配**，
> 在 AOT **没有 GC** 的前提下会把系统拖垮。教训：这类改动必须带
> 内存/时间看门狗验证。

**已回退**：`Emit.aura` 两处「Aura 编译路径」恢复为
`effArgStart = 1` + `staticSelfTy = "%struct.<Cls>*"`（原行为），
并保留 `recvKidIsValue()` 辅助（当前未启用，供下述正确修法使用）。
仓库已恢复到已验证绿色状态：Stage-1 ✓、载体编译并运行最小程序 ✓、
Stage-3（`native2.exe`）✓、语言测试 25/25 ✓。

**正确修法（下一步）**：根因是**value class 的接收者类型与其值表示不一致**。

1. 构造/使用 `functionSignature` 的合成接收者参数时，若 owner 是 value class
   （`String` / `Long` / `Char`…），接收者类型应取其**值表示**
   （String → `i8*`）而不是 `%struct.<Cls>*`；
2. 相应地 `staticSelfTy` 在值接收者场景应为该值类型（String → `i8*`）；
3. 之后即可启用「值接收者作为第 1 个实参」分支
   （`recvKidIsValue()` 已具备判别：局部变量/参数/`this`/字段 → 值接收者；
   类名 → 静态占位），并同步修正 `String_*` 系列签名表。

> 安全提示：后续所有「运行 Aura 侧产物」的验证都应带看门狗
> （超时强杀）与内存上限，避免再次拖垮宿主。

### 7.12 第九轮：三步修法落地（value class 接收者）—— Stage-4 前置缺口收口

**已实现（三步）**：

1. `Emit.aura::selfTyOf(ownerCls)`：方法接收者类型 —— **value class 用其值表示**
   （`String` → `i8*`、`Char` → `i16`…），其余类/object 仍为 `%struct.<Cls>*`；
   `functionSignature` 与 `emitFunction` 统一走它（二者必须一致，否则定义/调用签名错位）；
2. 两处「Aura 编译路径」的 `staticSelfTy` 改用 `selfTyOf(...)`；
3. 启用「值接收者作为第 1 个实参」分支（`recvKidIsValue()`：局部变量/参数/
   `this`/字段/非类名 → 值接收者；类名 → 静态占位），并补充
   「名字不是类/object 符号 ⇒ 值接收者」的兜底。

**验证**：

```llvm
; 定义与调用签名对齐（此前是 %struct.String* + null）
define i8* @String_split(i8* %arg.this, i8* %arg.separator)
%var.287 = call i8* @String_split(i8* %var.285, i8* %var.286)
```

```
$ build/_args.exe AAA BBB        # 全程看门狗，0.2s 返回
argc=3
arg[0]=D:\...\_args.exe
arg[1]=AAA
arg[2]=BBB
done                             ← 索引正确、无失控分配 ✓
```

Stage-1 ✓；Stage-3（载体编译自身 → `native2.exe`，30s）✓。

**Stage-4 的新阻塞点（已定位）**：`native2.exe`（Aura 自举产物）带参数运行仍段错误，
探针（`build/_io.aura`：`argCount` → `arg` → `FileSystem.readText`）显示
**崩在 `FileSystem.readText`**：

* 链路：`FileSystem.readText` → `Stdio.readFile`；
* 根因：`Stdio.readFile` 用 **Linux 的 `struct stat` 布局**取文件大小 ——
  `FileOps.fstat(fd, statBuf)` 后 `Memory.read64(statBuf + 48)`（Linux `st_size` 偏移）。
  Windows 上 `fstat` 已按我们的 CRT 映射落到 `_fstat`（写入 `struct _stat`，布局不同），
  `+48` 读到的是垃圾 → `size` 是垃圾值 → `Allocator.malloc(垃圾)` →
  **段错误 / 潜在无界分配**（与 §7.11 同一类风险，务必带看门狗验证）。

**建议修法（下一步）**：让 `Stdio.readFile` **不依赖 stat 布局** ——
改用「循环 `read` 直到 EOF」（`read` 返回 0 即为文件末尾）动态扩容缓冲区：
跨平台无结构体布局依赖，也顺带消除「size 为垃圾」的风险。
（备选：按平台选用 `_fstat64` 并读其正确偏移，但仍依赖 CRT 结构体布局。）

> ⚠ 在修好之前，**不要运行任何会调用 `FileSystem.readText`/`Stdio.readFile`
> 的 Aura 侧产物**（含 `native2.exe`），以免再次出现失控分配。

### 7.13 第十轮：Stage-4 逐步推进（已修 3 处，剩 1 处）

**已修**：

1. `Stdio.readFile`：改为「按块 `read` 直到 EOF + 按需扩容」，不再依赖 Linux
   `struct stat`/`st_size` 偏移（`fstat` + `read64(+48)` 在 Windows 读到垃圾长度）；
2. `FileSystem.fs_file_size`：改用可移植的 `lseek(fd, 0, SEEK_END)`；
3. **外部对象成员调用的实参错位**（关键）：新增 `fExternObjNames`（`extern object`
   名清单），调用点遇 `X.m(...)` 且 `X` 属该清单时跳过 `kids[0]` —— 对象名不是实参。
   此前对象名被求值成 `0` 占住第 1 个实参位，`FileOps.open(path, flags)` 变成
   `@open(0, path, flags)` → CRT `_open` 收到非法指针 → 无效参数 fail-fast
   （0xC0000409）。

**验证**（Aura 侧产物，全部带看门狗）：

```
build/_io2.exe            → A / B buf=… / C len=0 / done      （退出码 0，不再 fail-fast）
build/bin/aura-compiler-native2.exe build/_tiny.aura -o …
  → Error: input file not found: build/_tiny.aura              （参数读取正确、走到文件检查）
```

**Stage-4 剩余阻塞点（已精确到 IR）**：`Stdio.stringToBuffer` 的写缓冲循环被错发：

```llvm
; 源码：Memory.write(buf, 0)（写 NUL 终止符）
%var.21 = call i64 @write(i64 0, i64 %buf, i64 0)
```

* `Memory.write/read`（`object Memory` 的 `native fun` 内置）没有被识别为内置
  （应发 `store i8 <v>, i8* <addr>` / `load`），而是落到「裸名兜底」；
* 裸名 `write` 恰好与 `FileOps.write`（`@native(SYS_WRITE)`）的包装器**同名** →
  解析成了**文件写系统调用** ✗；
* 接收者 `Memory` 按表达式求值成 `0`，占据第 1 个实参位 → 实参整体错位。

结果：缓冲区内容全错（含 NUL 在内的字节都没写对）→ `_access(path)` 对存在的文件
返回非 0 → `FileSystem.exists` 为 `false` → 自举产物报 "input file not found"。

**修法（下一步）**：在 `emitCall` 增加 `Memory.*` 内置分支（与 `println`/集合内置
同层）：`Memory.read/read16/32/64` → `load <ty>, <ty>* (inttoptr addr)`；
`Memory.write/write16/32/64` → `store <ty> v, <ty>* (inttoptr addr)`；
`Memory.copy/set` → `llvm.memcpy/memset` 或对应循环。同时把该分支置于
「外部对象/裸名兜底」**之前**，避免与 `FileOps.write` 撞名。

### 7.14 第十一轮：`Memory.*` 内置落地 + Stage-4 收敛到最后一处

**已实现并验证**（`Emit.aura`）：

1. **`Memory.*` 内置访存**（置于「外部对象/裸名兜底」之前，避免与
   `FileOps.write` 撞名）：`read/read16/32/64` → `inttoptr` + `load`；
   `write/write16/32/64` → `inttoptr` + `store`；
2. **配套类型推断**：`inferType` 增 `Memory.*` 分支，按方法名返回
   `i8/i16/i32/i64`（否则落到 native 约定 i64，与发射出的 i8/i32 冲突，
   会出现 `'%var.N' defined with type 'i8' but expected 'i64'` 与
   `… i64 but expected 'i32'` 两类报错交替）；
3. 修正一处拼接错误：`coerceValue` 会把转换指令**写进当前函数体**，
   必须先求值再拼进 `store` 行（否则 llc 报 `expected ',' after store operand`）。

**验证**（全部带看门狗，退出码 0，无崩溃/失控）：

* Stage-1 ✓；**Stage-3 ✓**（`native2.exe` 25–31s）；
* Stage-4：`native2 build/_tiny.aura -o …` 已能**正确读取参数**并进入文件检查
  （报 `Error: input file not found: build/_tiny.aura`）；
* 探针 `build/_ex.aura`：`exists1=false / exists2=false / len=0`
  （崩溃已消除，但存在性判定仍错）。

**Stage-4 剩余阻塞点（已收敛到最后一处）**：
`FileSystem.exists("build/_tiny.aura")` 对**存在**的文件返回 `false`
⇒ `Stdio.fileExists` → `FileOps.access(path, 0)` 失败。
IR 已确认调用形式正确：

```llvm
%var.386 = call i64 @access(i64 %pathBuf, i64 0)     ; 实参不错位 ✓
define i64 @access(i64 %arg.0, i64 %arg.1) { … call i32 @_access(i8* …, i32 …) }
```

因此问题在 **`pathBuf` 的内容**：`Stdio.stringToBuffer` 用
`s.charCodeAt(i)` 逐字节写入，而 Aura 侧把 `charCodeAt` 映射到了
**`aura_lang_std_String_charAt`（C 侧返回 `const char*`）**，
声明为 `declare i8* @aura_lang_std_String_charAt(i8*, i64)` —— 返回的是
**指针**而不是字符码，写入缓冲区的是指针低位 → 路径全是垃圾字节 →
`_access` 找不到文件。

**下一步**：为 `String.charCodeAt` 提供**返回整数**的实现/映射
（C 侧新增 `int64_t aura_lang_std_String_charCodeAt(const char*, int64_t)`，
或让 Aura 侧 `charCodeAt` 走内联 `load i8, i8* (gep …)`），并同步修正
`inferType` 中 `charCodeAt` 的返回类型（应为 `i64/i32`，而不是 `i8*`）。

### 7.15 第十二轮：`charCodeAt` 内联（Stage-4 打通文件 I/O）

**实现**（按「Aura 走内联」）：`emitCall` 增 `charCodeAt` 分支 ——
`gep i8, i8* <s>, i64 <i>` + `load i8` + `zext i8 … to i64`，
不再调用返回 `const char*` 的 `aura_lang_std_String_charAt`；
`inferType` 同步返回 `i64`。

**验证**（Aura 侧产物，全部看门狗）：

```
build/_ex.exe →
exists1=true          ← FileSystem.exists("build/_tiny.aura") ✓
exists2=false         ← 不存在的文件 ✓
len=55                ← FileSystem.readText 真实读到 55 字节 ✓
```

Stage-1 ✓；Stage-3 ✓（`native2.exe` 32s）。

**Stage-4 现状**：`native2 build/_tiny.aura -o build/_tiny2.exe` 已能
**读参数 → 读源码文件 → 打印编译横幅**：

```
aura-compiler 0.1.0-phase9 — AOT compilation build/_tiny.aura
```

随后**静默崩溃**（约 6s，stdout/stderr 无错误输出、未产出 `_tiny2.exe`）——
阻塞点从「文件 I/O」推进到「自举产物的编译管线内部」。

**下一步**：定位该崩溃 —— 建议按管线阶段二分（读源码 → HIR 降级 → 发射 →
llc/clang），用「带看门狗 + 阶段日志」的方式逐段验证；也可先让 `native2`
编译一个更小的源（仅 `fun main(): Unit { }`）以缩小范围。

### 7.16 第十三轮：Stage-4 崩溃定位（结论：非输入相关，落在链接/发射阶段）

**二分实验**（native2 编译三个不同规模的源，全程看门狗）：

| 输入 | 内容 | 结果 |
|------|------|------|
| `build/_empty.aura` | `fun main(): Unit { }` | 崩溃（19s），未产出 |
| `build/_print.aura` | `println(123)` | 崩溃（3s），未产出 |
| `build/_tiny.aura` | `println("…")` | 崩溃（23s），未产出 |

* **连空 `main` 都崩溃** ⇒ 与输入无关，是 native2 自身编译管线的**基础路径**问题；
* 三次运行 stdout 都只到编译横幅，**`.ll` 从未落盘** ⇒ 崩溃发生在
  `AotModuleLinker.link()`（含 `essentialStdModules` 自动包含 20+ 模块）
  或 `AotUtils.compileAotIrOnly`（发射）之中，**尚未到 llc/clang**；
* 耗时在 3–23s 间波动 ⇒ 怀疑与「AOT 无 GC 的分配压力」或 `native2` 内部
  某些构造被 carrier 编译错有关。

**下一步（建议顺序）**：

1. ~~用 `Start-Process -Wait` 取精确退出码~~ —— **已测**：
   `EXITCODE=-1073741819` = **0xC0000005 ACCESS_VIOLATION**（空指针/越界解引用），
   稳定可复现（非 OOM、非栈溢出）⇒ 是 carrier 产物中的**指针错误**，
   下一步用阶段打印 + 最小复现收敛到具体构造；
2. 在编译器源码的管线关键点加**临时阶段打印**（`aotMemMark` 风格），
   重建 Stage-1 + Stage-3 后运行，观察最后一个打印点，逐段收敛；
3. 重点怀疑 carrier（Aura 侧 AOT）对**链接器/发射器自身用到的构造**的发射
   （集合、闭包、字符串方法、`HashMap` 等）存在边界 bug —— 可先用最小
   复现程序逐个验证这些构造在 Aura 产物中的行为。



