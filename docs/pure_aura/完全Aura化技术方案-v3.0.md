# 完全 Aura 化技术方�?v3.0 �?�?LLVM 运行�?
> **版本**: 3.0  
> **日期**: 2026-09-14  
> **状�?*: 待实�? 
> **核心目标**: 完全脱离 Rust �?C 代码编写，仅使用 LLVM 工具链生成全部运行时代码  
> **设计原则**: `@native` = 编译�?LLVM IR 生成指令，不�?C FFI 桥接  
> **运行时策�?*: LLVM 工具链自�?C 运行时（ucrt/msvcrt），编译器生�?IR 直接调用，无需自行编写 C 代码

---

## 〇、结论摘�?
### 核心修正

v2.0 方案存在方向性错误：`@native` 被当作「C 函数桩」使用，导致所有工具链 .aura 文件变成无效空壳�?v3.0 修正为：`@native` �?*编译�?LLVM IR 生成指令**，三种注解形式全部由编译器直接生成纯 LLVM IR�?
### 运行时策略修�?
**关键发现**：LLVM 工具链（`D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc`）自带完整的 C 运行时库和链接器。编译器生成 LLVM IR 后，通过 `llc` 编译为机器码，再�?`clang`/`lld-link` 链接�?C 运行时（ucrt.lib、msvcrt.lib），**无需自行编写任何 C 代码**�?
### 最终架�?
```
┌─────────────────────────────────────────────────────────────────────�?�? Layer 0-A: 最小引导（Rust，编译时工具�?                               �?�? ┌──────────────────────────────────────────────────────────────�?  �?�? �? compiler/src/bootstrap/                                       �?  �?�? �? ├── vm_core.rs     最小编译器 + VM（鸡生蛋必需�?                �?  �?�? �? ├── aot_core.rs    LLVM IR 生成入口                           �?  �?�? �? ├── memory.rs      最小内存管�?                              �?  �?�? �? ├── runtime.rs     协程 + GC 引导                             �?  �?�? �? ├── any_core.rs    toString/equals/hashCode                   �?  �?�? �? └── type_core.rs   typeOf/isOfType/cast                       �?  �?�? └──────────────────────────────────────────────────────────────�?  �?�? �?仅用于「第一次编�?Aura 编译器」，运行时不加载 Rust                    �?�?                                                                     �?�? Layer 0-B: LLVM 运行时（LLVM 工具�?+ C 运行时）                     �?�? ┌──────────────────────────────────────────────────────────────�?  �?�? �? @native(N) �?编译器生成内�?syscall 指令 IR                    �?  �?�? �? @native(asm="...") �?编译器生�?LLVM inline asm               �?  �?�? �? native fun �?编译器生成已知模式的 IR                           �?  �?�? �?                                                                 �?  �?�? �? 运行时依�?LLVM 工具链提供：                                     �?  �?�? �? ├── llc.exe           LLVM IR �?机器�?                        �?  �?�? �? ├── clang.exe / lld.exe  链接器（链接 C 运行时）                 �?  �?�? �? ├── opt.exe           LLVM IR 优化                             �?  �?�? �? ├── ucrt.lib / msvcrt.lib  C 运行时（printf, malloc, file I/O�?�?  �?�? �? ├── @llvm.memcpy / @llvm.memset  LLVM 内建函数                 �?  �?�? �? └── libm / libpthread  数学�?+ 线程�?                        �?  �?�? └──────────────────────────────────────────────────────────────�?  �?�? �?编译器不生成 C 代码，不生成 ASM 代码                                �?�? �?链接时自动链�?LLVM 工具链自带的 C 运行�?                           �?�?                                                                     �?�? Layer 1+: 全部 Aura 编译（编译器 + 标准�?+ 工具链）                   �?�? ┌──────────────────────────────────────────────────────────────�?  �?�? �? aura/compiler/aura/lang/compiler/   完整编译�?                �?  �?�? �? aura/core/aura/lang/std/            标准�?                   �?  �?�? �? aura/toolchain/aura/lang/           CLI/LSP/Debugger/Loom     �?  �?�? └──────────────────────────────────────────────────────────────�?  �?�? �?全部编译�?LLVM IR �?llc �?机器�?�?clang/lld �?可执行文�?        �?�?                                                                     �?�? 外部工具链（仅构建时）：                                               �?�?   �?LLVM 23.1.0 (llc/clang/lld/opt)   AOT 机器码生�?+ 链接         �?�?   �?(可�? Git                         包管�?                       �?�?                                                                     �?└─────────────────────────────────────────────────────────────────────�?```

### 运行时模�?
```
编译时：
  Aura 源码 �?Aura 编译�?�?LLVM IR �?opt �?llc �?机器�?.o

链接时：
  机器�?.o + ucrt.lib + msvcrt.lib + libm �?clang/lld-link �?可执行文�?
运行时：
  可执行文�?�?操作系统加载 �?执行（零 Rust，零自写 C 代码�?```

### @native 三种注解的正确语�?
| 注解形式 | NativeAttr | 编译器生�?| 运行时依�?|
|---------|-----------|----------|-----------|
| `@native(0)` | `Syscall(nr)` | **内联 `syscall` 指令 IR**（Linux�? **Nt* IR**（Windows�?| 无（直接 OS�?|
| `@native(asm="...")` | `Asm(code)` | **LLVM inline asm** `call asm sideeffect "..."` | 无（CPU 指令�?|
| `native fun` | `Builtin` | **已知模式 IR**（alloc→brk, read→load, free→munmap�?| �?|

### �?v2.0 的差�?
| 方面 | v2.0（错误） | v3.0（正确） |
|------|------------|------------|
| @native 定位 | C 函数�?| LLVM IR 生成指令 |
| Syscall 实现 | C 函数 `aura_syscall_dispatch` | 编译器生成内�?`syscall` |
| 内存管理 | C 函数 `aura_memory_alloc` | 编译器生�?brk/mmap IR |
| C 运行�?| 自行编写 aura_std_cffi.c | LLVM 工具链自带（ucrt/msvcrt�?|
| 工具�?.aura | �?@native（无效空壳） | 正确 @native(N) / @native(asm=) |
| C FFI �?| 2,840 �?C 代码运行时依�?| 可选（仅性能优化�?|
| 运行时依�?| Rust VM + C FFI + LLVM | LLVM 工具�?+ C 运行�?|

---

## 一、`@native` 编译�?IR 生成设计

### 1.1 Syscall（`@native(N)`�?
**目标**：编译器直接生成 `syscall` 指令�?LLVM IR，不依赖任何 C 函数�?
**Linux x86_64 生成**�?```llvm
define i64 @write(i64 %fd, i64 %buf, i64 %count) {
  entry:
    %rax = i64 1          ; SYS_write
    %rdi = %fd            ; arg0
    %rsi = %buf           ; arg1
    %rdx = %count         ; arg2
    call void asm sideeffect "syscall", "={rax},{rdi},{rsi},{rdx},{rdx},~{rcx},~{r11}"(
      i64 %rax, i64 %rdi, i64 %rsi, i64 %rdx)
    ret i64 %rax
}
```

**Windows x86_64 生成**�?```llvm
define i64 @write(i64 %handle, i64 %buf, i64 %count) {
  entry:
    %rax = i64 1          ; NtWriteFile service number
    %rdx = %handle
    call void asm sideeffect "syscall", "=r,0,0,{rdx},0,0,~{rcx},~{r11}"(
      i64 %rax, i64 0, i64 0, i64 %rdx, i64 0, i64 0)
    ret i64 %rax
}
```

### 1.2 Asm（`@native(asm="...")`�?
**目标**：已实现。编译器生成 LLVM inline asm，x86 使用 Intel 语法�?
```llvm
define i64 @rdtsc() {
  entry:
    %result = call i64 asm sideeffect "rdtsc", "={eax},={edx}"()
    ret i64 %result
}
```

### 1.3 Builtin（`native fun`�?
**目标**：按函数名匹配生�?LLVM IR 模式�?
| 函数名模�?| 生成�?IR |
|-----------|----------|
| `Memory.alloc(n)` | `call i64 asm sideeffect "brk", "=r"(i64 %old_brk)` �?返回�?brk 地址 |
| `Memory.free(addr)` | `call i64 asm sideeffect "mmap", "=r"(i64 0, i64 %addr, ...)` |
| `Memory.read(addr)` | `inttoptr i64 %addr to i8*` + `load i8, i8* %ptr` |
| `Memory.write(addr, val)` | `inttoptr` + `store i8 %val, i8* %ptr` |
| `Memory.alloc` (无参) | 内联 bump allocator |

### 1.4 内存分配器设�?
编译器生成内�?bump allocator�?```
内存模型�?  ┌─────────────────────�?  �? brk (当前断点)       �?  �? �?已分配区�?        �?  �? ══════════════════�?�?  �? 空闲区域            �?  └─────────────────────�?
分配�?  old_brk = syscall(brk, 0)  ; 获取当前 brk
  syscall(brk, old_brk + n)  ; 扩展 brk
  return old_brk              ; 返回起始地址

释放�?  使用 freelist �?mmap/munmap
```

---

## 二、完整关键字与注解方�?
### 2.1 注解体系总览

v3.0 定义了完整的注解体系，每个注解对应编译器不同�?LLVM IR 生成策略�?
```
┌──────────────────────────────────────────────────────────────────────�?�? 注解形式                      AST 节点           编译�?IR 生成         �?�?                                                                       �?�? @native(N)                   NativeAttr::Syscall  内联 syscall IR     �?�? @native(asm = "...")         NativeAttr::Asm      LLVM inline asm    �?�? @native / native fun         NativeAttr::Builtin  已知模式 IR          �?�? @aot fun                     AotMethod            AOT 库函�?IR      �?�? export fun                   ExportMethod         导出符号 IR         �?�? extern "C" { ... }           ExternDecl           C ABI 声明          �?�? extern object Name { ... }   ExternInterfaceDecl  FFI 接口�?         �?�? default fun                  DefaultModifier      默认实现（loadLibrary）│
�?                                                                       �?�? 所有注解由编译器在 AOT 阶段转换�?LLVM IR，运行时�?C/Rust 依赖          �?└──────────────────────────────────────────────────────────────────────�?```

### 2.2 @native 注解（Syscall / Asm / Builtin�?
已在 §一 详细设计。三种形式：

| 形式 | AST | IR 生成 | 示例 |
|------|-----|---------|------|
| `@native(0)` | `Syscall(nr)` | 内联 `syscall` 指令 | `@native(1) fun write(...)` |
| `@native(asm="...")` | `Asm(code)` | LLVM inline asm | `@native(asm="rdtsc") fun rdtsc()` |
| `@native` / `native fun` | `Builtin` | 已知模式 IR | `native fun alloc(n: Long)` |

### 2.3 @aot 注解（AOT 库函数）

**语法**�?```aura
extern object Math {
    default fun loadLibrary(): String = "libm.so"
    
    @aot fun sqrt(value: Float): Float { }
    @aot fun pow(base: Float, exp: Float): Float { }
}
```

**AST**：`AotMethod`（含 `AotInfo` metadata�?
**IR 生成策略**�?- `@aot` 函数声明�?`define <ret> @<name>(<params>)` �?LLVM IR
- 函数体为空（由链接时解析到动态库�?- 调用点生�?`call <ret> @<name>(<args>)`
- 编译时记录库路径，链接时传入 `-L <dir> -l <lib>`

**�?@native 的区�?*�?| | @native | @aot |
|---|---------|------|
| 实现位置 | 编译器生�?IR | 外部库提�?|
| 运行时依�?| 零（�?LLVM�?| 需要动态库 |
| 用�?| 系统调用、CPU 指令 | 数学库、第三方�?|

**Phase 0 处理**：@aot 声明暂保�?`declare`（不生成函数体），链接时解析。Phase 1 后逐步�?@native 替换�?
### 2.4 export 注解（导出符号）

**语法**�?```aura
export fun addToCart(item: String): Int {
    // 此函数对 C/Rust 可见
    return 0
}
```

**AST**：`ExportMethod`（含导出信息�?
**IR 生成策略**�?- `export fun` 生成�?LLVM IR 使用 `define dso_local` 修饰�?- 函数名不修饰（保持原样，供外部链接）
- 导出函数可被 C/Rust 代码通过符号名调�?
**�?@native 的区�?*�?| | @native | export |
|---|---------|--------|
| 方向 | Aura �?系统 | 系统 �?Aura |
| 实现 | 编译器生�?| 用户实现 |
| 用�?| 调用 syscall | 被外部调�?|

**Phase 0 处理**：export 暂生成普�?`define`，不添加 `dso_local` 修饰符。Phase 1 完善导出符号�?
### 2.5 extern "C" 声明（C ABI�?
**语法**�?```aura
extern "C" {
    fun malloc(size: Long): Long
    fun printf(fmt: String, ...): Int
    fun dlopen(path: String): Long
}
```

**AST**：`ExternDecl`（含 `lib_path`、`functions`、`constants`�?
**IR 生成策略**�?- 每个 `extern "C"` 函数生成 LLVM `declare` 语句
- 函数体为空，链接时解析到 C �?- 调用点生�?`call <ret> @<name>(<args>)`

**�?@native 的区�?*�?| | extern "C" | @native |
|---|-----------|---------|
| 实现 | C 库提�?| 编译器生�?|
| 运行时依�?| 需�?C �?| �?|
| 用�?| FFI 调用 C 函数 | 直接调用系统 |

**Phase 0 处理**：extern "C" 声明暂保留（用于 FFI �?libc 函数�?`malloc`/`dlopen`）。Phase 1 后逐步�?@native 替换�?
### 2.6 extern object 声明（FFI 接口块）

**语法**�?```aura
extern object Network {
    default fun loadLibrary(): String = "libnet.so"
    
    @aot fun connect(host: String, port: Int): Int { }
    @aot fun send(sock: Int, data: String, len: Int): Int { }
    
    const TIMEOUT_MS: Int = 5000
}
```

**AST**：`ExternInterfaceDecl`（含 `name`、`lib_path`、`functions`、`constants`�?
**IR 生成策略**�?- `default fun loadLibrary()` �?提取库路径，用于链接
- `@aot fun` �?生成 `declare` 语句（库函数�?- `@native fun` �?生成 `define` 语句（编译器 IR�?- `const` �?生成 LLVM 全局常量
- 调用点生�?`call` 语句

**Phase 0 处理**：extern object 中的 @aot 函数保留 `declare`，@native 函数生成 IR�?
### 2.7 完整注解优先�?
当同一函数有多种注解时，优先级从高到低�?
```
1. @native(N)        �?Syscall IR（最高优先级�?2. @native(asm="..") �?Asm IR
3. native fun        �?Builtin IR
4. @aot              �?AOT 库声�?5. export            �?导出修饰�?6. extern "C"        �?C ABI 声明（最低优先级�?```

---

## 三、LLVM 工具链运行时分析

### 3.1 LLVM 工具链能�?
LLVM 23.1.0 工具链（`D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc`）提供完整的运行时基础设施�?
| 工具 | 用�?| Aura 使用场景 |
|------|------|-------------|
| `llc.exe` | LLVM IR �?机器�?| 编译 .aura �?.o |
| `clang.exe` | C/C++ 编译 + 链接 | 链接 .o + C 运行�?�?可执行文�?|
| `lld.exe` / `lld-link.exe` | LLVM 链接�?| 替代系统链接�?|
| `opt.exe` | LLVM IR 优化 | IR 优化�?O2/-O3�?|
| `lli.exe` | JIT 执行 | 快速原�?|

### 3.2 C 运行时库

LLVM 工具链自�?Windows C 运行时库，编译时自动链接�?
| �?| 提供功能 | 替代当前 C FFI |
|----|---------|---------------|
| `ucrt.lib` | printf, malloc, free, strlen, strcpy, strcat, fopen/fread/fwrite | `aura_println`, `aura_malloc`, `aura_free`, `aura_string_*` |
| `msvcrt.lib` | 兼容�?| 同上 |
| `libm.lib` | sin, cos, tan, sqrt, pow, log, exp | `aura_math_*` |
| `kernel32.lib` | CreateFile, ReadFile, WriteFile, GetCurrentProcessId | `aura_fs_*`, `aura_process_*` |
| `ntdll.lib` | Nt* 系统调用 | `@native(N)` 直接调用 |

### 3.3 LLVM 内建函数

LLVM 提供一组内建函数（intrinsics），编译器可直接生成调用，无需任何 C 代码�?
| LLVM 内建函数 | 用�?| 替代 |
|-------------|------|------|
| `@llvm.memcpy` | 内存复制 | `memcpy` / `strcpy` |
| `@llvm.memset` | 内存填充 | `memset` / `memset` |
| `@llvm.memcmp` | 内存比较 | `memcmp` / `strcmp` |
| `@llvm.lifetime.start` | 生命周期标记 | RAII 析构 |
| `@llvm.lifetime.end` | 生命周期标记 | RAII 析构 |
| `@llvm.assume` | 断言优化 | 条件优化 |
| `@llvm.expect` | 分支预测 | 热点路径 |
| `@llvm.trap` | 触发断点 | 调试断点 |
| `@llvm.frameaddress` | 帧地址 | 栈回�?|
| `@llvm.returnaddress` | 返回地址 | 调用�?|
| `@llvm.getfunctionaddress` | 函数地址 | 函数指针 |
| `@llvm.stacksave` | 栈保�?| 栈切�?|
| `@llvm.stackrestore` | 栈恢�?| 栈切�?|

### 3.4 当前 C FFI �?LLVM 库的映射

当前 `aura_std_cffi.h`�?78 行声明）+ `aura_std_cffi.c`�?,847 行实现）+ `aura_syscalls.c`�?15 行实现）的功能可�?LLVM 工具链替代：

| 当前 C FFI 函数 | LLVM 替代方案 | 处理方式 |
|----------------|-------------|---------|
| `aura_println(s)` | `printf` + `write` syscall | 链接 ucrt.lib |
| `aura_print(s)` | `printf` | 链接 ucrt.lib |
| `aura_malloc(n)` | `malloc` | 链接 ucrt.lib |
| `aura_free(p)` | `free` | 链接 ucrt.lib |
| `aura_string_concat` | `@llvm.memcpy` + `malloc` | LLVM 内建 + ucrt |
| `aura_string_length` | `strlen` | 链接 ucrt.lib |
| `aura_math_sin` | `sin` | 链接 libm.lib |
| `aura_math_sqrt` | `sqrt` | 链接 libm.lib |
| `aura_fs_readText` | `fopen` + `fread` + `fclose` | 链接 ucrt.lib |
| `aura_fs_writeText` | `fopen` + `fwrite` + `fclose` | 链接 ucrt.lib |
| `aura_syscall_dispatch` | 编译�?@native(N) IR | 编译器生�?|
| `aura_memory_alloc` | `@llvm.memcpy` + `malloc` | LLVM 内建 + ucrt |
| `aura_memory_free` | `free` | 链接 ucrt.lib |
| `aura_cpu_rdtsc` | `@native(asm="rdtsc")` | 编译器生�?|
| `aura_cpu_mem_fence` | `@native(asm="lfence")` | 编译器生�?|

### 3.5 运行时模�?
```
编译时：
  Aura 源码
    �?Aura 编译器（Rust bootstrap�?    �?LLVM IR 文本
    �?opt -O2（IR 优化�?    �?llc -filetype=obj（机器码 .o�?
链接时：
  机器�?.o
    + ucrt.lib        （C 运行时：printf, malloc, file I/O�?    + libm.lib        （数学库�?    + kernel32.lib    （Windows API�?    + ntdll.lib       （Nt* 系统调用�?    �?clang / lld-link �?可执行文�?
运行时：
  可执行文�?    �?操作系统加载�?    �?执行（零 Rust，零自写 C 代码�?    �?C 运行时由操作系统提供
```

### 3.6 与当�?C FFI 的关�?
**当前状�?*：`aura_std_cffi.c` 提供 ~2,840 �?C 代码，在编译时编译为 .o，链接到可执行文件�?
**v3.0 策略**�?1. **短期（Phase 0-2�?*：AOT 编译器路径逐步脱离 C FFI，VM 模式保留 C FFI 作为 fallback
2. **中期（Phase 3-4�?*：标准库�?LLVM 化，AOT 路径完全独立�?C FFI
3. **长期（Phase 5+�?*：C FFI 标记为遗留代码，�?VM 模式使用，编译器不再调用

**关键区别**�?- v2.0：编译器生成 IR 调用 `@aura_syscall_dispatch` �?需要链�?`aura_syscalls.c`
- v3.0：编译器生成内联 `syscall` IR �?不需要任�?C 代码，直接调�?OS
- C FFI 代码保留但仅 VM 字节码模式使用，AOT/JIT 路径完全独立

### 3.7 LLVM 内建函数在编译器中的使用

编译器在生成 LLVM IR 时，可直接插�?LLVM 内建函数调用�?
```llvm
; 字符串拼接示�?define { i8*, i64 } @concat(i8* %a, i64 %alen, i8* %b, i64 %blen) {
  entry:
    %total = add i64 %alen, %blen
    %buf = call i8* @malloc(i64 %total)
    call void @llvm.memcpy(i8* %buf, i8* %a, i64 %alen, i1 false)
    call void @llvm.memcpy(i8* %buf_end, i8* %b, i64 %blen, i1 false)
    %result = insertvalue { i8*, i64 } undef, i8* %buf, 0
    %result = insertvalue { i8*, i64 } %result, i64 %total, 1
    ret { i8*, i64 } %result
}
```

---

## 四、C FFI 层的处理

### 4.1 当前 C FFI �?
| 文件 | 行数 | 用�?| v3.0 处理 |
|------|------|------|----------|
| `aura_syscalls.c` | 515 | syscall 分发、内存管理、CPU 指令 | **逐步替换**：每�?syscall 转为 @native(N) IR |
| `aura_std_cffi.c` | 1,847 | 标准�?C ABI | **分模块处�?*：纯逻辑→Aura，syscall→@native(N) |
| `aura_std_cffi.h` | 478 | 声明 | **最终删�?* |

### 4.2 替换策略

**P0 �?必须替换（阻塞完�?Aura 化）**�?| C 函数 | @native 替换方式 |
|--------|----------------|
| `aura_syscall_dispatch(nr, ...)` | `@native(N)` �?编译器生�?syscall IR |
| `aura_memory_alloc(n)` | `@native(SYS_BRK)` �?`native fun Memory.alloc` |
| `aura_memory_free(addr)` | `@native(SYS_MUNMAP)` �?`native fun Memory.free` |
| `aura_cpu_rdtsc()` | `@native(asm="rdtsc")` |
| `aura_cpu_mem_fence()` | `@native(asm="lfence")` |
| `aura_cpu_atomic_add(addr, val)` | `@native(asm="lock xaddq (rax)")` |
| `aura_println(s)` | `@native(SYS_WRITE)` + 字符串操�?|

**P1 �?可选替换（性能敏感�?*�?| C 函数 | 替代方式 |
|--------|---------|
| `aura_string_*` | �?Aura 字符串操作（已有 String.aura�?|
| `aura_math_*` | �?Aura 实现（已�?Math.aura�?|
| `aura_collections_*` | �?Aura 实现（已�?Collections.aura�?|

**P2 �?保留 C（性能敏感或不可替换）**�?| C 函数 | 理由 |
|--------|------|
| `dlopen/dlsym` | 动态链接是内核 ABI，不可纯 LLVM |
| `zstd_compress` | 性能敏感，保�?C + FFI |
| `sha256`（C 版） | 已有 Aura 实现，但 C 版更�?|

---

## 五、工具链 .aura 文件修正（Phase E�?
### 5.1 Loom.aura

**当前问题**�? 个裸 `@native` 声明，函数名不在匹配表，生成空壳�?
**修正**：不调用 Rust，不调用 `@native`，使用纯 Aura + `aura.core.native`�?```aura
// 错误方式 1（旧）：通过 Rust 实现
@native fun loomShellExec(cmd: String): Int { }

// 错误方式 2（旧）：在工具链中自行声�?@native
@native(SYS_FORK) fun loomShellExec(cmd: String): Int { }

// 正确方式（Phase E）：使用 aura.core.native �?import aura.lang.native.Syscalls
import aura.lang.native.file.FileOps
import aura.lang.native.process.ProcessOps
import aura.lang.native.console.Console

// �?Aura 实现，通过 aura.core.native 包调�?fun loomShellExec(cmd: String): Int {
    // 使用 FileOps 进行文件操作
    val fd: Int = FileOps.open(cmdAddr, O_WRONLY)
    FileOps.write(fd, outputBuf, outputLen)
    FileOps.close(fd)
    return 0
}
```

### 5.2 AuraCli.aura

**当前问题**�?0 个裸 `@native` 声明，参数类型为 Int（丢失参数信息）�?
**修正**：纯 Aura 实现，调用编译器 API + `aura.core.native` 包：
```aura
import aura.lang.compiler.Main
import aura.lang.native.file.FileOps
import aura.lang.native.console.Console
import aura.lang.native.memory.Allocator

// 编译器函数由 Aura 编译器自身实�?fun codegenCompile(src: String): String {
    val ir: String = Main.compile(src)
    return ir
}

// 文件操作使用 aura.core.native
fun readSourceFile(path: String): String {
    val fd: Int = FileOps.open(pathAddr, O_RDONLY)
    val buf: Long = Allocator.malloc(4096)
    val bytesRead: Long = FileOps.read(fd, buf, 4096)
    FileOps.close(fd)
    return stringFromBuffer(buf, bytesRead)
}
```

### 5.3 AuraLsp.aura

**当前问题**�?1 个裸 `@native` 声明，JSON-RPC 用字符串搜索模拟�?
**修正**：纯 Aura JSON 解析 + 编译�?API + `aura.core.native`�?```aura
import aura.lang.compiler.Main
import aura.lang.native.console.Console
import aura.lang.native.file.FileOps

// �?Aura JSON 解析�?fun lspParseRequest(data: String): String {
    val parsed: List<Any> = Json.parse(data)
    return Json.stringify(parsed)
}

// LSP 请求分派：调用编译器 API
fun lspCompletion(file: String, pos: Int): String {
    val content: String = readViaFileOps(file)
    val ast = Main.parse(content)
    val symbol = Main.findSymbolAt(ast, pos)
    return Json.stringify(["label": symbol.name, "kind": symbol.kind])
}
```

### 5.4 AuraDebugger.aura

**当前问题**�?3 个裸 `@native` 声明，无 VM 交互�?
**修正**：纯 Aura + 编译�?API + `aura.core.native`�?```aura
import aura.lang.compiler.Main
import aura.lang.native.console.Console
import aura.lang.native.file.FileOps

// 编译器插入断点检�?fun debugRun(): Int {
    return Main.debugStart()
}

fun debugStepOver(): Int {
    return Main.debugStepOver()
}

fun debugVariables(): String {
    return Main.debugCurrentVariables()
}
```

---

## 六、开发阶段计�?
### Phase 0：编译器 @native IR 生成�? 周）

**目标**：让 `@native(N)` 直接生成�?LLVM syscall IR，不依赖 C 分发函数�?
| # | 任务 | 文件 | 工作�?|
|---|------|------|--------|
| 0.1 | Syscall 分支改为内联 `syscall` 指令 | `emit.rs` | 2d |
| 0.2 | Builtin 分支扩展 alloc/free/memory | `emit.rs` | 2d |
| 0.3 | 去除 `declare @aura_syscall_dispatch` | `emit.rs` | 0.5d |
| 0.4 | 添加 Linux x86_64 syscall 号表 | `ast.rs` / `emit.rs` | 0.5d |
| 0.5 | 添加 Windows Nt* 服务号表 | `ast.rs` / `emit.rs` | 1d |
| 0.6 | 创建 Linux x86_64 Syscalls.aura | `Syscalls.aura` | 1d |
| 0.7 | 验证：Syscalls.aura 编译通过 | 测试 | 1d |

### Phase 1：LLVM 内建函数 + C 运行时链接（1 周）

**目标**：使�?LLVM 内建函数�?C 运行时库替代手写 C FFI�?
| # | 任务 | 文件 | 工作�?|
|---|------|------|--------|
| 1.1 | LLVM 内建函数集成（memcpy/memset/memcmp�?| `emit.rs` | 2d |
| 1.2 | C 运行时链接（ucrt.lib/msvcrt.lib�?| 构建脚本 | 1d |
| 1.3 | Memory.read/write �?load/store IR | `emit.rs` | 1d |
| 1.4 | 去除 `declare @aura_memory_alloc/free` | `emit.rs` | 0.5d |
| 1.5 | 验证：Memory.aura 编译通过 | 测试 | 1d |

### Phase 2：Syscalls.aura 全面覆盖�? 周）

| # | 任务 | 文件 | 工作�?|
|---|------|------|--------|
| 2.1 | Linux Syscalls.aura 完整 syscall �?| `Syscalls.aura` | 2d |
| 2.2 | Windows Nt* Syscalls.aura | `Syscalls.aura` | 2d |
| 2.3 | FileOps.aura �?syscall 调用 | `FileOps.aura` | 2d |
| 2.4 | ProcessOps.aura �?syscall 调用 | `ProcessOps.aura` | 1d |
| 2.5 | 验证：文�?进程操作通过 | 测试 | 1d |

### Phase 3：标准库�?LLVM 化（2 周）

**目标**：标准库使用 LLVM 工具�?C 运行时，不再依赖 aura_std_cffi.c�?
| # | 任务 | 文件 | 工作�?|
|---|------|------|--------|
| 3.1 | IO.aura �?C 运行�?printf/write | `IO.aura` | 2d |
| 3.2 | FileSystem.aura �?C 运行�?fopen/fread | `FileSystem.aura` | 3d |
| 3.3 | String.aura �?C 运行�?strlen/strcpy + LLVM 内建 | `String.aura` | 2d |
| 3.4 | Math.aura �?libm.lib 链接 | `Math.aura` | 1d |
| 3.5 | 去除 aura_std_cffi.c 依赖 | C 清理 | 2d |
| 3.6 | 验证：全部标准库通过 | 测试 | 2d |

### Phase E：工具链全部�?Aura 重写�? 周）

**目标**：工具链（Loom/AuraCli/AuraLsp/AuraDebugger）完全用 Aura 实现�?*每个工具独立编译�?exe**，不再依�?Rust 编译�?`aura` CLI�?
**核心原则**�?- **�?Rust 调用**：工具链 .aura 文件不包含任�?`@native` 调用 Rust 实现的代码，所有逻辑用纯 Aura 编写
- **�?Aura 实现**：编译器代码已存在于 `aura/compiler/aura/lang/compiler/`�?0+ 文件，完整编译管线），工具链直接调用 Aura 编译�?API
- **Native 接口唯一来源**：需要系统级操作时，**仅调�?`aura.core.native` �?*下的接口（Syscalls / FileOps / ProcessOps / Console / Memory / Allocator / StrOps / MathCore 等），不自行编写 `@native` 声明
- 每个工具独立编译�?exe，零 Rust 依赖
- 仅依�?LLVM 工具链（clang/llc/opt�? `aura.core.native` �?
**`aura.core.native` 包提供的 Native 接口**�?| 模块 | 路径 | 提供的能�?|
|------|------|-----------|
| Syscalls | `aura.lang.native.Syscalls` | 系统调用号常量（SYS_READ / SYS_WRITE / SYS_OPEN 等） |
| FileOps | `aura.lang.native.file.FileOps` | 文件读写/打开/关闭/seek/stat/delete/exists |
| ProcessOps | `aura.lang.native.process.ProcessOps` | 进程退�?wait/exec/pid |
| Console | `aura.lang.native.console.Console` | stdout 输出（print/println/printInt/printlnInt�?|
| Memory | `aura.lang.native.Memory` | 内存 read/write/copy/set/alloc/free |
| Allocator | `aura.lang.native.memory.Allocator` | 堆内�?malloc/free/realloc（bump allocator�?|
| StrOps | `aura.lang.native.string.StrOps` | 字符�?strlen/strcmp/strcpy/strncpy/hash |
| Cpu | `aura.lang.native.Cpu` | rdtsc/memFence/cpuid/atomicAdd（内联汇编） |
| MathCore | `aura.lang.native.math.MathCore` | abs/max/min/sqrt/sin/cos/tan/pow/log/exp |
| Clock | `aura.lang.native.time.Clock` | clock_gettime 高精度时�?|

**架构设计**�?```
┌─────────────────────────────────────────────────────────�?�?                   工具链层 (独立 exe)                      �?├─────────────────────────────────────────────────────────�?�? AuraCli.exe  ──�?                                       �?�? Loom.exe      ──┤→ 直接调用 Aura 编译�?API              �?�? AuraLsp.exe   ──�?  (Main.compile / aotBuildExeSource)  �?�? Debugger.exe  ──�?  �?Rust 调用，纯 Aura 实现            �?├─────────────────────────────────────────────────────────�?�?             aura.core.native �?(Native 接口�?            �?�? ┌──────────┬──────────┬──────────┬───────────────�?    �?�? �?Syscalls �?FileOps  �?ProcessOps�?Console       �?    �?�? ├──────────┼──────────┼──────────┼───────────────�?    �?�? �?Memory   �?Allocator�?StrOps   �?Cpu           �?    �?�? ├──────────┼──────────┼──────────┼───────────────�?    �?�? �?MathCore �?Clock    �?         �?              �?    �?�? └──────────┴──────────┴──────────┴───────────────�?    �?�? �?所�?@native 声明集中�?native 包，工具链不自行声明       �?├─────────────────────────────────────────────────────────�?�?                   编译器层 (Aura 实现)                     �?�? aura/compiler/aura/lang/compiler/Main.aura              �?�? ├── lexer/  ├── parser/  ├── sema/  ├── hir/            �?�? ├── mir/    ├── codegen/ ├── aot/   ├── vm/             �?�? └── jit/    └── gc/                                    �?├─────────────────────────────────────────────────────────�?�?                   LLVM 工具�?                             �?�? clang.exe / llc.exe / opt.exe                           �?└─────────────────────────────────────────────────────────�?```

**独立编译目标**�?| 工具 | 输入文件 | 输出 exe | 功能 |
|------|----------|----------|------|
| AuraCli | `aura/toolchain/cli/aura/lang/cli/Main.aura` | `aura.exe` | 编译/运行/检�?格式�?|
| Loom | `aura/toolchain/loom/aura/lang/loom/Main.aura` | `loom.exe` | 构建系统/任务编排 |
| AuraLsp | `aura/toolchain/lsp/aura/lang/lsp/Main.aura` | `aura-lsp.exe` | LSP 服务�?|
| Debugger | `aura/toolchain/debugger/aura/lang/debugger/Main.aura` | `aura-debug.exe` | 调试�?|

**编译命令**�?```bash
# 编译 AuraCli.exe
aura build aura/toolchain/cli/aura/lang/cli/Main.aura --aot --llvm-home <LLVM> --output build/aura.exe

# 编译 Loom.exe
aura build aura/toolchain/loom/aura/lang/loom/Main.aura --aot --llvm-home <LLVM> --output build/loom.exe

# 编译 AuraLsp.exe
aura build aura/toolchain/lsp/aura/lang/lsp/Main.aura --aot --llvm-home <LLVM> --output build/aura-lsp.exe

# 编译 Debugger.exe
aura build aura/toolchain/debugger/aura/lang/debugger/Main.aura --aot --llvm-home <LLVM> --output build/aura-debug.exe
```

| # | 任务 | 文件 | 工作�?|
|---|------|------|--------|
| E.1 | 验证 aura/compiler/ 编译管线可用 | `aura/compiler/**` | 2d |
| E.2 | AuraCli.aura �?独立 CLI（纯 Aura，调用编译器 API + aura.core.native�?| `AuraCli.aura` | 3d |
| E.3 | Loom.aura �?独立构建系统（纯 Aura，调用编译器 API + aura.core.native�?| `Loom.aura` | 3d |
| E.4 | AuraLsp.aura �?独立 LSP 服务器（�?Aura，调用编译器 API + aura.core.native�?| `AuraLsp.aura` | 2d |
| E.5 | AuraDebugger.aura �?独立调试器（�?Aura，调用编译器 API + aura.core.native�?| `AuraDebugger.aura` | 2d |
| E.6 | 验证�? 个工具独立编译为 exe | 测试 | 1d |
| E.7 | 验证：零 Rust 调用、零自写 @native 声明 | 测试 | 1d |

**关键区别（vs Phase 4 旧方案）**�?| 旧方�?| 新方案（Phase E�?|
|--------|-------------------|
| 工具链调�?`system("aura build ...")` | 工具链直接调�?`Main.aotBuildExeSource()` |
| 任务命令是字符串 "aura build ..." | 任务直接调用 Aura API |
| 依赖 Rust 编译�?`aura` CLI | 每个工具独立编译�?exe |
| shell 调用 aura.exe | 进程内函数调�?|
| 工具链内自行声明 `@native` | 仅调�?`aura.core.native` �?|
| 工具链调�?Rust 实现（vmRun 等） | �?Aura 实现，无 Rust 调用 |

**编译器模块结�?*（`aura/compiler/aura/lang/compiler/`）：
```
Main.aura          �?主入口，编译管线
lexer/             �?词法分析 (Lexer, Token, Span)
parser/            �?语法分析 (Parser)
ast/               �?AST 定义
sema/              �?语义分析 (TypeChecker, Type, TypeInfo, SymbolTable)
hir/               �?HIR (Desugar, Hir, Mono, Inline, Fold)
mir/               �?MIR (Mir, MirLower, MirOpt)
codegen/           �?代码生成 (Codegen)
aot/               �?AOT 后端 (Aot, Emit, Target, Linker, Runtime, Ffi...)
vm/                �?虚拟�?(Vm, VmRunner, Frames, Opcodes...)
jit/               �?JIT 编译�?(JitCore, JitLower, JitOpt...)
gc/                �?垃圾回收 (Gc, MarkSweep, Concurrent...)
```

**工具链调用关�?*（进程内直接调用，无 shell，无 Rust）：
```
AuraCli.exe        �?Main.compile() / Main.aotBuildExeSource()   + FileOps (文件读写) + Console (输出)
Loom.exe           �?Main.compile() / Main.aotBuildExeSource()   + FileOps (文件读写) + Allocator (内存)
AuraLsp.exe        �?Main.compile() (诊断) / lexer/parser (补全) + Console (stdio I/O)
AuraDebugger.exe   �?Main.compile() (求�? / vm (调试)           + Console (交互式输�?
```

**禁止事项**�?| 禁止 | 正确做法 |
|------|---------|
| 工具链中声明 `@native` 函数 | 调用 `aura.core.native` 包已有的 native 函数 |
| 工具链调�?`system("aura build ...")` | 直接调用 `Main.aotBuildExeSource()` |
| 工具链调�?`Process.args()` 来自 Rust | 使用 `ProcessOps.getPid()` �?native 接口 |
| 自行实现文件 I/O | 使用 `FileOps.open/read/write/close` |
| 自行实现控制台输�?| 使用 `Console.println/print` �?`IO.println` |
| 自行实现内存分配 | 使用 `Allocator.malloc` �?`Memory.alloc` |

**总工�?*�? 周（14 天）

### Phase 5：C FFI 层标记为遗留代码�? 周）

**目标**：C FFI 代码保留在仓库中但编译器不再调用，作�?VM 模式 fallback �?legacy 兼容层�?
| # | 任务 | 文件 | 工作�?|
|---|------|------|--------|
| 5.1 | 标记 aura_syscalls.c 为遗留代�?| C 注释 + docs | 0.5d |
| 5.2 | 标记 aura_std_cffi.c 中已替换部分为遗�?| C 注释 + docs | 1d |
| 5.3 | 验证：AOT 路径�?C FFI 引用 | 测试 | 1d |
| 5.4 | 验证：VM 模式仍可调用 C FFI（兼容） | 测试 | 0.5d |
| 5.5 | 最终文档更�?| docs | 0.5d |

**关键区别**�?- Phase 0-4：逐步将编译器路径�?C FFI 迁移�?LLVM IR + C 运行�?- Phase 5：确认编译器已不调用 C FFI，C 代码仅作�?VM 模式 fallback 保留
- C 代码不删除（VM 模式需要），但 AOT 编译路径完全独立�?C FFI

**编译器路�?vs VM 路径**�?| 路径 | C FFI 依赖 | 说明 |
|------|-----------|------|
| AOT 编译路径 | �?不依�?| @native(N) �?内联 syscall IR；Builtin �?LLVM 内建 |
| VM 字节码路�?| �?依赖 | 运行时调�?C FFI 函数（性能敏感场景�?|
| JIT 编译路径 | �?不依�?| �?AOT 路径相同，LLVM IR �?JIT |

**总工�?*�? �?
---

## 七、保�?C 的例外（唯一合法依赖�?
| 例外 | 理由 | 来源 | AOT 路径 | VM 路径 |
|------|------|------|---------|---------|
| LLVM 工具�?C 运行时（ucrt/msvcrt/libm�?| 提供 printf, malloc, file I/O, 数学函数 | LLVM 安装目录自带 | �?链接使用 | �?链接使用 |
| LLVM 内建函数（@llvm.memcopy/@llvm.memset�?| 内存操作，编译器直接生成调用 | LLVM IR 内建 | �?编译器生�?| N/A |
| 目标平台动态链接（dlopen/dlsym�?| 内核 ABI 是二进制接口，不可纯 LLVM | OS API | �?保留 | �?保留 |
| Zstd 压缩（可选） | 性能敏感，保�?C + FFI | 第三方库 | �?保留 | �?保留 |
| 编译器本身（Rust�?| 仅构建时使用，不进入运行�?| bootstrap | N/A | N/A |
| aura_std_cffi.c（遗留） | VM 模式运行�?fallback | 仓库�?| �?不调�?| �?运行时调�?|
| aura_syscalls.c（遗留） | VM 模式 syscall 分发 fallback | 仓库�?| �?不调�?| �?运行时调�?|

### 7.1 保留 C 运行时的理由

**关键区别**�?- v2.0：自行编�?`aura_std_cffi.c`�?,840 行）�?编译�?.o �?链接到可执行文件 �?编译器调�?- v3.0：链�?LLVM 工具链自带的 C 运行时（ucrt.lib/msvcrt.lib）→ 不编写任�?C 代码
- C FFI 代码保留但仅 VM 模式使用，AOT/JIT 编译路径完全独立

**保留 C 运行时不影响"�?LLVM"目标**，因为：
1. 不编写任�?C 代码
2. 不生成任�?ASM 代码
3. C 运行时由 LLVM 工具链（clang）自动提供和链接
4. 编译器仅生成 LLVM IR，运行时行为�?OS + C 运行时保�?
**C FFI 遗留代码保留理由**�?1. VM 字节码模式需�?C FFI 作为运行�?fallback
2. 性能敏感场景（zstd, sha256）仍使用 C 实现
3. 渐进迁移：先确认 AOT 路径不依赖，再决定是否删�?4. 向后兼容：旧�?.auc 文件可在 VM 模式下运�?
---

## 八、风险矩�?
| # | 风险 | 概率 | 影响 | 缓解 |
|---|------|------|------|------|
| R1 | Syscall IR 跨平台兼容�?| �?| �?| 先支�?Linux x86_64，再�?Windows |
| R2 | Bump allocator 性能 | �?| �?| 可切换到 LLVM 工具链自�?malloc/free |
| R3 | 内联汇编平台差异 | �?| �?| �?target_triple 分支生成 |
| R4 | 工具�?.aura 重写工作�?| �?| �?| 可分阶段，先�?Loom �?CLI |
| R5 | 标准库字符串性能回退 | �?| �?| 使用 LLVM 工具链自�?C 运行时（strlen, strcpy�?|
| R6 | LLVM 工具链版本兼容�?| �?| �?| 固定 LLVM 版本，CI 中验�?|
| R7 | C 运行时跨平台差异 | �?| �?| Windows: ucrt.lib, Linux: glibc/musl |
| R8 | LLVM 内建函数不可优化 | �?| �?| opt.exe 已支持内建函数优�?|
| R9 | 链接器符号解析失�?| �?| �?| clang 自动处理符号，减少手动链�?|

---

*本文档修正了 v2.0 的方向性错误：`@native` 是编译器 LLVM IR 生成指令，不�?C FFI 桥接�?  
*同时修正了运行时模型：依�?LLVM 工具链自带的 C 运行时，而非自行编写 C 代码�?
