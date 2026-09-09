# 完全 Aura 化技术方案

> **版本**: 1.0  
> **日期**: 2026-07-04  
> **状态**: 待实施  
> **核心目标**: 标准库完全 Aura 化，三态执行模式（VM/JIT/AOT）全部支持 AOT 直连 FFI

---

## 目录

1. [背景与目标](#1-背景与目标)
2. [当前架构问题](#2-当前架构问题)
3. [最终架构设计](#3-最终架构设计)
4. [三态执行模式与 AOT 直连](#4-三态执行模式与-aot-直连)
5. [开发阶段详细计划](#5-开发阶段详细计划)
6. [风险与缓解](#6-风险与缓解)
7. [里程碑与交付物](#7-里程碑与交付物)

---

## 1. 背景与目标

### 1.1 背景

Aura 语言当前标准库采用混合架构：
- **Layer 1（Rust native）**：Any 核心虚方法、类型内省、内存管理等
- **Layer 2（C FFI）**：syscall 操作（FileSystem/IO/Network）
- **Layer 3（Aura 源码）**：纯逻辑模块（Math/String/Path 等）

**问题**：
- phantom-source 目录仅用于 IDE SourceIndex 生成，不参与编译
- Rust native 实现与 Aura 源码存在双层漂移
- "上移"只是文档名义，未实现真正的代码迁移

### 1.2 目标

| # | 目标 | 度量 |
|---|------|------|
| G1 | 标准库完全 Aura 化 | 纯逻辑模块 100% Aura 实现 |
| G2 | 三态执行模式 | VM/JIT/AOT 全部支持 |
| G3 | AOT 直连 FFI | 所有模式消除函数指针间接调用 |
| G4 | 单一真相源 | phantom-source 是真实源码 |
| G5 | 性能最优 | AOT 直连消除间接调用开销 |

### 1.3 核心原则

1. **AOT 直连优先**：默认 FFI 方式选择 AOT 直连，消除函数指针间接调用
2. **三态一致性**：VM/JIT/AOT 三态模式语义一致
3. **渐进式迁移**：保留 Rust native 作为降级方案
4. **单一真相源**：phantom-source 是标准库唯一源码

---

## 2. 当前架构问题

### 2.1 phantom-source 不参与编译

```
当前架构：
┌─────────────────────────────────────────────────────────────────┐
│ phantom-source/*.aura  ──→  SourceIndex 生成（仅用于 IDE）      │
│                         ✗ 不参与编译                            │
│                         ✗ 不参与运行时                          │
│                                                                  │
│ compiler/src/std/std_*.rs  ──→  Rust native 实现（实际运行时）  │
│                                  ✓ 参与 VM 执行                 │
│                                  ✓ 参与 AOT 编译                │
│                                                                  │
│ 结果："上移"只是文档，不是实际代码迁移                            │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 双层实现漂移

| 层级 | 文件 | 状态 |
|------|------|------|
| 文档层 | `phantom-source/*.aura` | ❌ 不参与编译 |
| 实现层 | `compiler/src/std/std_*.rs` | ✅ 实际运行时实现 |
| FFI 层 | `compiler/src/std/cffi/aura_std_cffi.c` | ✅ AOT 实际调用 |

### 2.3 FFI 调用开销

```
当前 FFI 调用方式：
┌─────────────────────────────────────────────────────────────────┐
│ VM 模式:   Aura 字节码  ──→  VM 查找函数指针  ──→  C 函数调用   │
│            开销：函数指针查找 + 参数转换 + 调用                   │
│                                                                  │
│ JIT 模式:   Aura 字节码  ──→  JIT 生成间接调用  ──→  C 函数调用 │
│            开销：间接调用 + 参数转换                               │
│                                                                  │
│ AOT 模式:   Aura 字节码  ──→  LLVM IR  ──→  间接调用  ──→ C 函数 │
│            开销：间接调用 + 参数转换                               │
│                                                                  │
│ 问题：所有模式都通过函数指针间接调用，有性能开销                    │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. 最终架构设计

### 3.1 三层架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│  最终架构                                                                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Layer 0: Bootstrap（Rust/C，不能上移）                                   │
│  ├── 最小虚拟机（VM Core）                                               │
│  │   ├── 字节码解释器                                                    │
│  │   ├── 栈帧管理                                                        │
│  │   └── 指令分发                                                        │
│  ├── JIT 编译器核心                                                      │
│  │   ├── 热点检测                                                        │
│  │   ├── 代码生成                                                        │
│  │   └── 去优化（deoptimization）                                        │
│  ├── AOT 编译器核心                                                      │
│  │   ├── LLVM IR 生成                                                    │
│  │   ├── 机器代码生成                                                    │
│  │   └── 内联优化                                                        │
│  ├── Any 核心虚方法                                                      │
│  │   ├── toString() → 字符串转换                                         │
│  │   ├── equals() → 值相等比较                                           │
│  │   └── hashCode() → 哈希计算                                           │
│  ├── 类型内省核心                                                        │
│  │   ├── typeOf() → 类型名称查询                                         │
│  │   ├── isOfType() → 类型检查                                           │
│  │   └── cast() → 类型转换                                               │
│  ├── 空值/数值检查                                                       │
│  │   ├── isNull() / isNotNull()                                          │
│  │   ├── isZero() / isPositive() / isNegative()                          │
│  │   └── isNaN() / isInfinite()                                          │
│  ├── 内存管理                                                            │
│  │   ├── malloc() / free()                                               │
│  │   ├── arc_increment() / arc_decrement()                               │
│  │   └── string_new() / string_length() / string_concat()                │
│  └── 运行时函数                                                          │
│      ├── coroutine_yield()                                               │
│      └── gc_collect() / gc_mark() / gc_sweep()                           │
│                                                                         │
│  Layer 1+: 全部 Aura 编译（loom 构建系统）                               │
│  ├── 纯逻辑模块                                                          │
│  │   ├── Math.aura（abs/min/max/sign/clamp）                             │
│  │   ├── String.aura（contains/split/replace/trim）                      │
│  │   ├── Path.aura（join/split/normalize）                               │
│  │   ├── Encoding.aura（base64/base32/hex）                              │
│  │   └── Time.aura（duration/diff）                                      │
│  ├── Builtin 扩展                                                        │
│  │   ├── toInt/toFloat/toBool/toStr                                     │
│  │   ├── clamp/identity                                                  │
│  │   └── typeHierarchy/isSubtype/implementsInterface                     │
│  ├── syscall-邻近模块（C FFI 直连 libc）                                 │
│  │   ├── FileSystem.aura（stat/fopen/fread/fwrite/close）                │
│  │   ├── IO.aura（printf/fgets/fread/fwrite）                            │
│  │   └── Network.aura（socket/bind/connect/send/recv）                   │
│  └── 运行库（AOT 直连）                                                  │
│      ├── Coroutine.aura（通过 coroutine_yield）                          │
│      ├── Actor.aura（通过线程/消息队列）                                  │
│      ├── Channel.aura（通过管道/消息队列）                                │
│      └── Collections.aura（List/Map/Set）                                │
│                                                                         │
│  构建流程                                                                │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ 1. 预编译标准库（bootstrap 阶段）                                 │   │
│  │    phantom-source/*.aura  ──→  .auc 字节码                      │   │
│  │    aura_std_cffi.c       ──→  .a 静态库                          │   │
│  │                                                                  │   │
│  │ 2. 应用编译时链接标准库                                          │   │
│  │    main.aura  ──→  加载 std.auc  ──→  链接符号                  │   │
│  │                                                                  │   │
│  │ 3. 三态执行                                                      │   │
│  │    VM 模式:   .auc  ──→  VM 解释执行  ──→  AOT 直连 C 函数      │   │
│  │    JIT 模式:   .auc  ──→  VM 解释  ──→  JIT 编译  ──→  AOT 直连  │   │
│  │    AOT 模式:   .auc  ──→  LLVM IR  ──→  机器代码  ──→  AOT 直连  │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 3.2 模块分类

| 层级 | 模块 | 实现方式 | 能否上移 |
|------|------|----------|----------|
| Layer 0 | Any 核心 | Rust native | ❌ 不能 |
| Layer 0 | 类型内省 | Rust native | ❌ 不能 |
| Layer 0 | 空值检查 | Rust native | ❌ 不能 |
| Layer 0 | 内存管理 | Rust native | ❌ 不能 |
| Layer 0 | 运行时 | Rust native | ❌ 不能 |
| Layer 1+ | Math | Aura 实现 | ✅ 能 |
| Layer 1+ | String | Aura 实现 | ✅ 能 |
| Layer 1+ | Path | Aura 实现 | ✅ 能 |
| Layer 1+ | Encoding | Aura 实现 | ✅ 能 |
| Layer 1+ | Builtin | Aura 实现 | ✅ 能 |
| Layer 1+ | Time | Aura 实现 | ✅ 能 |
| Layer 1+ | Collections | Aura 实现 | ✅ 能 |
| Layer 1+ | FileSystem | Aura + C FFI | ✅ 能 |
| Layer 1+ | IO | Aura + C FFI | ✅ 能 |
| Layer 1+ | Network | Aura + C FFI | ✅ 能 |
| Layer 1+ | Coroutine | Aura + AOT 直连 | ✅ 能 |
| Layer 1+ | Actor | Aura + AOT 直连 | ✅ 能 |
| Layer 1+ | Channel | Aura + AOT 直连 | ✅ 能 |

---

## 4. 三态执行模式与 AOT 直连

### 4.1 AOT 直连定义

**AOT 直连**是指消除函数指针间接调用，直接调用目标函数的技术。

```
┌─────────────────────────────────────────────────────────────────────────┐
│  FFI 调用方式对比                                                        │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  传统 FFI（间接调用）                                                    │
│  ─────────────────────                                                  │
│  Aura 字节码  ──→  查找函数指针  ──→  间接调用 C 函数                   │
│  开销：函数指针查找 + 间接调用 + 参数转换                                 │
│                                                                         │
│  AOT 直连（直接调用）                                                    │
│  ─────────────────────                                                  │
│  Aura 字节码  ──→  直接调用 C 函数                                      │
│  开销：直接调用 + 参数转换（无间接调用）                                  │
│                                                                         │
│  性能差异：                                                              │
│  • VM 模式: 消除函数指针查找，预加载函数地址                              │
│  • JIT 模式: 生成直接调用指令，而非间接调用                               │
│  • AOT 模式: 生成直接调用指令，LLVM 可进一步优化                          │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.2 VM 模式 AOT 直连

```
┌─────────────────────────────────────────────────────────────────────────┐
│  VM 模式 AOT 直连                                                        │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  启动阶段:                                                               │
│  1. 加载标准库 .auc 文件                                                 │
│  2. 解析 extern "c" 声明，提取 C 函数名称列表                            │
│  3. 预加载所有 C 函数地址到 VM 的函数表                                  │
│  4. 建立 Aura 函数名 → C 函数地址的映射                                  │
│                                                                         │
│  执行阶段:                                                               │
│  1. VM 遇到 FFI 调用指令                                                 │
│  2. 从预加载的函数表直接获取函数地址                                      │
│  3. 直接调用 C 函数（无函数指针查找）                                     │
│  4. 返回结果到 VM 栈                                                     │
│                                                                         │
│  字节码设计:                                                             │
│  CALL_Cffi <function_name>  // 直接调用，无间接寻址                      │
│                                                                         │
│  性能优化:                                                               │
│  • 函数地址预加载：启动时一次性加载所有 C 函数地址                        │
│  • 内联缓存：缓存最近调用的函数地址，加速重复调用                          │
│  • 零开销调用：消除函数指针查找开销                                       │
│                                                                         │
│  代码示例:                                                               │
│  // VM 内部实现                                                          │
│  struct FfiCallSite {                                                   │
│      function_name: String,                                            │
│      function_address: usize,  // 预加载的 C 函数地址                    │
│      call_count: u64,         // 调用计数                               │
│  }                                                                       │
│                                                                         │
│  impl Vm {                                                              │
│      fn call_ffi(&mut self, func_name: &str) -> Value {                │
│          let call_site = self.ffi_registry.get(func_name).unwrap();     │
│          let func_addr = call_site.function_address;                    │
│          // 直接调用，无函数指针查找                                      │
│          unsafe { call_c_function(func_addr, &self.stack) }            │
│      }                                                                   │
│  }                                                                       │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.3 JIT 模式 AOT 直连

```
┌─────────────────────────────────────────────────────────────────────────┐
│  JIT 模式 AOT 直连                                                       │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  编译阶段:                                                               │
│  1. JIT 编译 Aura 字节码为机器代码                                       │
│  2. 遇到 FFI 调用时，生成直接调用指令（call func_name）                  │
│  3. 生成 PLT（Procedure Linkage Table）条目                              │
│  4. 符号解析后，补丁更新为实际函数地址                                    │
│                                                                         │
│  执行阶段:                                                               │
│  1. JIT 生成的机器代码直接调用 C 函数                                    │
│  2. 无间接调用开销                                                       │
│  3. LLVM 可进一步优化（内联、去虚拟化）                                  │
│                                                                         │
│  机器代码示例:                                                           │
│  ; JIT 生成的直接调用                                                    │
│  call fopen       ; 直接调用，非间接调用                                 │
│  add rsp, 8       ; 清理栈                                              │
│                                                                         |
│  vs. 传统间接调用                                                        │
│  mov rax, [rdi]   ; 从函数指针表加载地址                                 │
│  call rax         ; 间接调用                                            │
│  add rsp, 8       ; 清理栈                                              │
│                                                                         │
│  性能优化:                                                               │
│  • 直接调用：生成 call func_name 指令                                    │
│  • 符号延迟解析：启动时生成 PLT 条目，首次调用时解析                       │
│  • 内联缓存：热点 FFI 调用可内联到调用方                                  │
│  • 零开销调用：消除间接调用开销                                           │
│                                                                         │
│  代码示例:                                                               │
│  ; JIT 代码生成器                                                        │
│  pub fn emit_ffi_call(&mut self, func_name: &str) {                   │
│      // 生成直接调用指令                                                  │
│      self.emit_call_direct(func_name);                                  │
│      // 生成 PLT 条目（符号延迟解析）                                     │
│      self.emit_plt_entry(func_name);                                    │
│  }                                                                       │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.4 AOT 模式 AOT 直连

```
┌─────────────────────────────────────────────────────────────────────────┐
│  AOT 模式 AOT 直连                                                       │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  编译阶段:                                                               │
│  1. AOT 编译 Aura 字节码为 LLVM IR                                       │
│  2. 遇到 FFI 调用时，生成直接调用指令（call func_name）                  │
│  3. LLVM 优化器可进一步优化（内联、去虚拟化、死代码消除）                 │
│  4. 生成机器代码，链接 libc 库                                           │
│                                                                         │
│  LLVM IR 示例:                                                          │
│  ; AOT 生成的直接调用                                                    │
│  declare i8* @fopen(i8*, i8*)  ; 声明 libc 函数                          │
│                                                                         │
│  %fd = call i8* @fopen(                                                │
│      i8* %path,                                                         │
│      i8* %mode                                                           │
│  )                                                                       │
│  ; LLVM 可内联、优化这个调用                                             │
│                                                                         │
│  机器代码示例:                                                           │
│  ; AOT 生成的直接调用                                                    │
│  fopen:                                                                  │
│      call fopen@PLT   ; 直接调用（PLT 符号解析）                         │
│  add rsp, 8          ; 清理栈                                           │
│                                                                         │
│  性能优化:                                                               │
│  • 直接调用：生成 call func_name 指令                                    │
│  • LLVM 优化：内联、去虚拟化、死代码消除                                  │
│  • 静态链接：可选静态链接 libc，消除动态链接开销                           │
│  • 零开销调用：消除间接调用开销                                           │
│                                                                         │
│  代码示例:                                                               │
│  ; AOT 编译器                                                            │
│  pub fn emit_ffi_call(&mut self, func_name: &str) {                   │
│      // 生成 LLVM IR 直接调用                                            │
│      self.emit_call_direct(func_name);                                  │
│      // 声明 C 函数原型                                                  │
│      self.emit_c_function_decl(func_name);                              │
│  }                                                                       │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.5 三态模式 AOT 直连对比

| 维度 | VM 模式 | JIT 模式 | AOT 模式 |
|------|---------|----------|----------|
| **调用方式** | 预加载函数地址，直接调用 | 生成直接调用指令 | 生成直接调用指令 |
| **函数解析** | 启动时预加载 | 首次调用时 PLT 解析 | 编译时符号解析 |
| **间接调用** | ❌ 无 | ❌ 无 | ❌ 无 |
| **调用开销** | 低（无查找） | 低（无间接） | 最低（LLVM 优化） |
| **优化潜力** | 内联缓存 | 内联、去虚拟化 | 内联、去虚拟化、死代码消除 |
| **启动开销** | 预加载函数地址 | 无 | 编译时间 |
| **适用场景** | 开发调试 | 桌面应用 | 高性能计算 |

### 4.6 FFI 声明语法

```aura
// C FFI 声明（AOT 直连）
extern "c" "libc" fun fopen(path: String, mode: String): Pointer
extern "c" "libc" fun fread(buf: Pointer, size: Int, count: Int, stream: Pointer): Int
extern "c" "libc" fun fclose(stream: Pointer): Int

// 自定义 C 库
extern "c" "mylib" fun my_function(arg: Int): Int

// Rust FFI 声明（Layer 1 函数）
extern "rust" fun any_toString(value: Any): String
extern "rust" fun any_equals(a: Any, b: Any): Boolean

// 混合声明（同一模块内）
fun readText(path: String): String {
    val fd = fopen(path, "r")  // C FFI，AOT 直连
    if fd == null then return ""
    
    val buf = malloc(1024)      // C FFI，AOT 直连
    val n = fread(buf, 1, 1024, fd)  // C FFI，AOT 直连
    fclose(fd)                  // C FFI，AOT 直连
    
    val result = bufferToString(buf, n)
    free(buf)
    return result
}
```

### 4.7 默认配置

```toml
# aura.toml（默认配置）
[build]
execution-mode = "auto"  # vm | jit | aot | auto（默认自动选择）
ffi-mode = "aot"         # aot | cffi | rustffi（默认 AOT 直连）

[build.ffi.aot]
inline = true            # 允许内联 C 代码
optimize = 3             # 优化级别（0-3）
static-link = false      # 是否静态链接 libc

[build.ffi.cffi]
lib = "aura_std_cffi"    # C FFI 库名称
include = ["compiler/src/std/cffi"]

[build.ffi.rustffi]
modules = [
    "aura.lang.std.Any",
    "aura.lang.std.Type",
    "aura.lang.std.Value"
]

# 三态模式自动选择策略
[build.auto-mode]
vm = "开发调试、交互式应用"
jit = "桌面应用、服务器应用"
aot = "高性能计算、嵌入式"

# 自动降级策略
[build.fallback]
aot-failed = "jit"       # AOT 编译失败降级为 JIT
jit-failed = "vm"        # JIT 编译失败降级为 VM
```

### 4.8 执行流程

```
┌─────────────────────────────────────────────────────────────────────────┐
│  三态模式执行流程                                                        │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  aura run main.aura  ──→  默认 VM 模式                                  │
│    ├── 加载标准库 .auc 文件                                              │
│    ├── 预加载 C 函数地址（AOT 直连）                                    │
│    ├── VM 解释执行                                                       │
│    └── FFI 调用：直接调用 C 函数                                         │
│                                                                         │
│  aura run --jit main.aura  ──→  JIT 模式                               │
│    ├── 加载标准库 .auc 文件                                              │
│    ├── VM 解释执行（启动快）                                             │
│    ├── 热点检测 → JIT 编译                                               │
│    └── FFI 调用：生成直接调用指令（AOT 直连）                            │
│                                                                         │
│  aura run --aot main.aura  ──→  AOT 模式                               │
│    ├── AOT 编译标准库 + 应用源码                                         │
│    ├── 生成 LLVM IR（含直接调用指令）                                    │
│    ├── 编译为机器代码                                                    │
│    └── FFI 调用：直接调用 C 函数（AOT 直连）                            │
│                                                                         │
│  aura build main.aura  ──→  AOT 编译                                    │
│    ├── 编译为标准库 .auz 制品                                            │
│    ├── 含 VM/JIT/AOT 三态制品                                            │
│    └── 可分发给其他用户                                                   │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 5. 开发阶段详细计划

### Phase 1: Bootstrap 最小引导层（第 1-2 周）

**目标**：提取不能上移的最小 Rust 核心，为三态执行提供基础。

**开发内容**：

```
├── compiler/src/bootstrap/
│   ├── vm_core.rs          # 最小虚拟机核心（VM 模式必需）
│   │   ├── 字节码解释器
│   │   ├── 栈帧管理
│   │   ├── 指令分发
│   │   ├── 异常处理
│   │   └── FFI 调用支持（AOT 直连）
│   ├── jit_core.rs         # JIT 编译器核心（JIT 模式必需）
│   │   ├── 热点检测
│   │   ├── 代码生成
│   │   ├── 内联缓存
│   │   └── 去优化（deoptimization）
│   ├── aot_core.rs         # AOT 编译器核心（AOT 模式必需）
│   │   ├── LLVM IR 生成
│   │   ├── 机器代码生成
│   │   ├── 内联优化
│   │   └── 死代码消除
│   ├── any_core.rs         # Any 核心虚方法
│   │   ├── toString()
│   │   ├── equals()
│   │   └── hashCode()
│   ├── type_core.rs        # 类型内省核心
│   │   ├── typeOf()
│   │   ├── isOfType()
│   │   └── cast()
│   ├── value_check.rs      # 空值/数值检查
│   │   ├── isNull() / isNotNull()
│   │   ├── isZero() / isPositive() / isNegative()
│   │   └── isNaN() / isInfinite()
│   ├── memory.rs           # 内存管理
│   │   ├── malloc() / free()
│   │   ├── arc_increment() / arc_decrement()
│   │   └── string_new() / string_length() / string_concat()
│   └── runtime.rs          # 运行时函数
│       ├── coroutine_yield()
│       └── gc_collect() / gc_mark() / gc_sweep()
```

**AOT 直连支持**：
- VM 模式：预加载 C 函数地址，直接调用
- JIT 模式：生成直接调用指令
- AOT 模式：生成 LLVM IR 直接调用

**验收标准**：
- [x] Bootstrap 层代码独立，无外部依赖
- [x] VM 核心可执行简单字节码
- [x] JIT 核心可编译热点方法
- [x] AOT 核心可生成机器代码
- [x] Any 核心虚方法正常工作
- [x] 类型内省核心正常工作
- [x] 内存管理正常工作
- [x] FFI AOT 直连正常工作（三态模式）

**测试用例**：
```rust
// tests/bootstrap_test.rs
#[test]
fn test_vm_core() {
    // 测试 VM 字节码解释器
}

#[test]
fn test_jit_core() {
    // 测试 JIT 即时编译
}

#[test]
fn test_aot_core() {
    // 测试 AOT 提前编译
}

#[test]
fn test_ffi_aot_direct() {
    // 测试 FFI AOT 直连（三态模式）
}

#[test]
fn test_any_core() {
    // 测试 toString/equals/hashCode
}

#[test]
fn test_type_core() {
    // 测试 typeOf/isOfType/cast
}

#[test]
fn test_memory() {
    // 测试 malloc/free/arc
}
```

---

### Phase 2: loom aura-stdlib 插件改造（第 3-4 周）

**目标**：让 loom 构建系统实际编译 phantom-source 文件，支持三态模式 + AOT 直连。

**开发内容**：

```
├── loom/src/plugin/convention.rs
│   └── StdlibPlugin 改造
│       ├── 扫描 phantom-source 目录
│       ├── 编译 .aura 为 .auc（VM/JIT 模式）
│       ├── 编译 .auc 为机器代码（AOT 模式）
│       ├── 编译 aura_std_cffi.c 为 .a
│       ├── 生成标准库索引
│       └── 注册到构建上下文
│
├── loom/src/task/compile_stdlib.rs  # 新增
│   ├── 编译 phantom-source/*.aura
│   ├── 编译 aura_std_cffi.c 为 .a
│   ├── 打包为标准库制品
│   └── 支持三态模式 + AOT 直连
│
└── loom/src/stdlib/mod.rs           # 新增
    ├── 标准库索引生成
    ├── 标准库符号解析
    ├── 标准库链接
    └── 三态模式 + AOT 直连配置
```

**关键代码**：
```rust
// loom/src/plugin/convention.rs
pub struct StdlibPlugin {
    phantom_source_dir: PathBuf,
    output_dir: PathBuf,
    execution_mode: ExecutionMode,  // Vm | Jit | Aot
    ffi_mode: FfiMode,              // Aot | Cffi | Rustffi（默认 Aot）
}

impl BuildPlugin for StdlibPlugin {
    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-stdlib");
        
        // 1. 扫描 phantom-source 目录
        let aura_files = scan_aura_files(&self.phantom_source_dir)?;
        
        // 2. 编译每个 .aura 文件为 .auc（VM/JIT 模式）
        for file in &aura_files {
            let module = compile_source(file)?;
            let auc_path = self.output_dir.join(file.stem() + ".auc");
            serialize_to_file(&module, &auc_path)?;
        }
        
        // 3. 如果是 AOT 模式，编译为机器代码
        if matches!(self.execution_mode, ExecutionMode::Aot) {
            for auc_path in self.output_dir.join("*.auc") {
                aot_compile(&auc_path)?;
            }
        }
        
        // 4. 编译 C FFI 库（AOT 直连需要）
        compile_cffi_library(&self.output_dir)?;
        
        // 5. 生成标准库索引（含 FFI 函数地址映射）
        let stdlib_index = generate_stdlib_index(&aura_files)?;
        let ffi_index = generate_ffi_index(&aura_files)?;
        
        // 6. 注册到构建上下文
        ctx.register_stdlib(&stdlib_index);
        ctx.register_ffi(&ffi_index);
        
        tracing::info!(
            "aura-stdlib: 已编译 {} 个标准库模块 (mode: {:?}, ffi: {:?})",
            aura_files.len(),
            self.execution_mode,
            self.ffi_mode
        );
        Ok(())
    }
}
```

**AOT 直连配置**：
- 默认 FFI 模式：AOT 直连
- C FFI 函数：生成直接调用指令
- Rust FFI 函数：通过 NativeRegistry 注册
- 运行库函数：AOT 直连内联

**验收标准**：
- [x] aura-stdlib 插件能扫描 phantom-source 目录
- [x] 能编译 .aura 文件为 .auc
- [x] 能编译 .auc 为机器代码（AOT 模式）
- [x] 能编译 aura_std_cffi.c 为静态库
- [x] 能生成标准库索引
- [x] 能生成 FFI 函数地址映射
- [x] 能注册到构建上下文
- [x] 支持三态模式 + AOT 直连

**测试用例**：
```rust
// loom/tests/stdlib_plugin_test.rs
#[test]
fn test_stdlib_plugin_compile_vm() {
    // 测试 VM 模式编译
}

#[test]
fn test_stdlib_plugin_compile_jit() {
    // 测试 JIT 模式编译
}

#[test]
fn test_stdlib_plugin_compile_aot() {
    // 测试 AOT 模式编译
}

#[test]
fn test_ffi_index_generation() {
    // 测试 FFI 函数地址映射生成
}

#[test]
fn test_cffi_library_compile() {
    // 测试 C FFI 库编译
}
```

---

### Phase 3: 编译器集成标准库（第 5-7 周）

**目标**：让编译器在编译应用时链接标准库，支持三态模式 + AOT 直连。

**开发内容**：

```
├── compiler/src/codegen/
│   ├── mod.rs              # 修改：支持标准库链接
│   ├── link_stdlib.rs      # 新增：标准库符号链接
│   │   ├── 加载标准库 .auc 文件
│   │   ├── 链接标准库符号
│   │   └── 解析标准库调用
│   ├── resolve_stdlib.rs   # 新增：标准库调用解析
│   │   ├── 解析标准库函数调用
│   │   ├── 解析标准库类型引用
│   │   └── 解析标准库常量引用
│   ├── ffi_aot.rs          # 新增：AOT 直连支持（三态模式）
│   │   ├── VM 模式：预加载函数地址
│   │   ├── JIT 模式：生成直接调用指令
│   │   └── AOT 模式：生成 LLVM IR 直接调用
│   └── execution.rs        # 新增：执行模式选择
│       ├── VM 模式配置
│       ├── JIT 模式配置
│       └── AOT 模式配置
│
└── compiler/src/vm/
    ├── mod.rs              # 修改：支持多模块加载
    ├── multi_module.rs     # 新增：多模块支持
    │   ├── 加载多个 .auc 文件
    │   ├── 跨模块符号解析
    │   └── 跨模块函数调用
    ├── ffi_cache.rs        # 新增：FFI 调用缓存
    │   ├── 预加载函数地址
    │   ├── 内联缓存
    │   └── 调用计数
    ├── jit/mod.rs          # 新增：JIT 支持
    │   ├── 热点检测
    │   ├── 代码生成（含 AOT 直连）
    │   └── 去优化
    └── aot/mod.rs          # 新增：AOT 支持
        ├── LLVM IR 生成（含 AOT 直连）
        ├── 机器代码生成
        └── 链接器集成
```

**关键代码**：
```rust
// compiler/src/codegen/ffi_aot.rs
pub fn configure_ffi_aot_direct(
    module: &mut BytecodeModule,
    execution_mode: ExecutionMode,
) -> Result<(), String> {
    // 1. 解析所有 extern "c" 声明
    let ffi_decls = module.get_ffi_declarations()?;
    
    // 2. 根据执行模式配置 AOT 直连
    match execution_mode {
        ExecutionMode::Vm => {
            // VM 模式：预加载函数地址
            let ffi_cache = FfiCache::new();
            for decl in &ffi_decls {
                let func_addr = ffi_cache.preload_function(&decl.name)?;
                module.set_ffi_address(&decl.name, func_addr);
            }
            module.enable_ffi_aot_direct();
        }
        ExecutionMode::Jit => {
            // JIT 模式：生成直接调用指令标记
            for decl in &ffi_decls {
                module.mark_ffi_direct_call(&decl.name);
            }
            module.enable_ffi_aot_direct();
        }
        ExecutionMode::Aot => {
            // AOT 模式：生成 LLVM IR 直接调用
            for decl in &ffi_decls {
                module.emit_ffi_direct_call(&decl.name);
            }
            module.enable_ffi_aot_direct();
        }
    }
    
    Ok(())
}

// compiler/src/vm/ffi_cache.rs
pub struct FfiCache {
    function_addresses: HashMap<String, usize>,
    call_sites: HashMap<String, FfiCallSite>,
}

impl FfiCache {
    pub fn new() -> Self {
        Self {
            function_addresses: HashMap::new(),
            call_sites: HashMap::new(),
        }
    }
    
    pub fn preload_function(&mut self, name: &str) -> Result<usize, String> {
        // 从 libc 或自定义库加载函数地址
        let addr = unsafe { load_function_address(name)? };
        self.function_addresses.insert(name.to_string(), addr);
        Ok(addr)
    }
    
    pub fn get_function_address(&self, name: &str) -> Option<usize> {
        self.function_addresses.get(name).copied()
    }
}

// compiler/src/codegen/mod.rs
pub fn compile_source_with_stdlib(
    source: &str,
    stdlib_auc_dir: &Path,
    execution_mode: ExecutionMode,
    ffi_mode: FfiMode,
) -> Result<BytecodeModule, String> {
    // 1. 编译应用源码
    let mut module = compile_source(source)?;
    
    // 2. 加载标准库 .auc 文件
    let stdlib_modules = load_stdlib_modules(stdlib_auc_dir)?;
    
    // 3. 链接标准库符号
    link_stdlib_symbols(&mut module, &stdlib_modules)?;
    
    // 4. 解析标准库调用
    resolve_stdlib_calls(&mut module, &stdlib_modules)?;
    
    // 5. 根据执行模式配置
    match execution_mode {
        ExecutionMode::Vm => configure_vm_mode(&mut module),
        ExecutionMode::Jit => configure_jit_mode(&mut module),
        ExecutionMode::Aot => configure_aot_mode(&mut module),
    }
    
    // 6. 配置 FFI AOT 直连（默认模式）
    if matches!(ffi_mode, FfiMode::Aot) {
        configure_ffi_aot_direct(&mut module, execution_mode)?;
    }
    
    Ok(module)
}
```

**AOT 直连实现**：
- VM 模式：FfiCache 预加载函数地址，直接调用
- JIT 模式：emit_call_direct 生成直接调用指令
- AOT 模式：emit_ffi_direct_call 生成 LLVM IR 直接调用

**验收标准**：
- [x] 编译器能加载标准库 .auc 文件
- [x] 编译器能链接标准库符号
- [x] 编译器能解析标准库调用
- [x] VM 能执行多模块程序
- [x] JIT 能编译热点方法
- [x] AOT 能生成机器代码
- [x] 支持三态模式切换
- [x] FFI AOT 直连正常工作（三态模式）

**测试用例**：
```rust
// compiler/tests/stdlib_integration_test.rs
#[test]
fn test_stdlib_linking() {
    // 测试标准库符号链接
}

#[test]
fn test_stdlib_call_resolution() {
    // 测试标准库调用解析
}

#[test]
fn test_multi_module_vm() {
    // 测试多模块 VM 执行
}

#[test]
fn test_jit_hot_method() {
    // 测试 JIT 热点方法编译
}

#[test]
fn test_aot_compilation() {
    // 测试 AOT 编译
}

#[test]
fn test_ffi_aot_direct_vm() {
    // 测试 VM 模式 FFI AOT 直连
}

#[test]
fn test_ffi_aot_direct_jit() {
    // 测试 JIT 模式 FFI AOT 直连
}

#[test]
fn test_ffi_aot_direct_aot() {
    // 测试 AOT 模式 FFI AOT 直连
}
```

---

### Phase 4: 替换 Rust native 实现（第 8-11 周）

**目标**：用 Aura 实现替换可上移的 Rust native 实现，支持三态模式 + AOT 直连。

**替换清单**：

| 模块 | 文件 | 函数数量 | VM 模式 | JIT 模式 | AOT 模式 |
|------|------|---------|---------|----------|----------|
| Math | std_math.rs → Math.aura | 15 | Aura 解释 | Aura → JIT | Aura AOT |
| String | std_string.rs → String.aura | 25 | Aura 解释 | Aura → JIT | Aura AOT |
| Path | std_path.rs → Path.aura | 10 | Aura 解释 | Aura → JIT | Aura AOT |
| Encoding | std_encoding.rs → Encoding.aura | 8 | Aura 解释 | Aura → JIT | Aura AOT |
| Builtin | std_builtin.rs → Builtin.aura | 12 | Aura 解释 | Aura → JIT | Aura AOT |
| Time | std_time.rs → Time.aura | 6 | Aura 解释 | Aura → JIT | Aura AOT |
| Collections | std_collections.rs → Collections.aura | 20 | Aura 解释 | Aura → JIT | Aura AOT |

**保留 Rust 实现**（Layer 1，不能上移）：

| 模块 | 文件 | VM 模式 | JIT 模式 | AOT 模式 |
|------|------|---------|----------|----------|
| Any 核心 | any_core.rs | Rust 调用 | Rust 调用 | Rust 调用 |
| 类型内省 | type_core.rs | Rust 调用 | Rust 调用 | Rust 调用 |
| 空值检查 | value_check.rs | Rust 调用 | Rust 调用 | Rust 调用 |
| 内存管理 | memory.rs | Rust 调用 | Rust 调用 | Rust 调用 |
| 运行时 | runtime.rs | Rust 调用 | Rust 调用 | Rust 调用 |
| FileSystem | std_fs.rs | C FFI（AOT 直连） | C FFI（AOT 直连） | C FFI（AOT 直连） |
| IO | std_io.rs | C FFI（AOT 直连） | C FFI（AOT 直连） | C FFI（AOT 直连） |
| Network | std_net.rs | C FFI（AOT 直连） | C FFI（AOT 直连） | C FFI（AOT 直连） |

**三态执行策略**：

```
┌─────────────────────────────────────────────────────────────────────────┐
│  模块执行策略                                                            │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Math.abs(x):                                                           │
│  VM 模式:   Aura 字节码  ──→  VM 解释执行                               │
│  JIT 模式:   Aura 字节码  ──→  VM 解释  ──→  热点检测  ──→  JIT 编译    │
│  AOT 模式:   Aura 字节码  ──→  LLVM IR  ──→  机器代码                   │
│                                                                         │
│  FileSystem.read(path):                                                │
│  VM 模式:   C FFI（AOT 直连）  ──→  fopen/fread/fclose                 │
│  JIT 模式:   C FFI（AOT 直连）  ──→  fopen/fread/fclose                 │
│  AOT 模式:   C FFI（AOT 直连）  ──→  fopen/fread/fclose                 │
│                                                                         │
│  Any.toString():                                                       │
│  VM 模式:   Rust native 调用  ──→  any_core.rs                         │
│  JIT 模式:   Rust native 调用  ──→  any_core.rs                         │
│  AOT 模式:   Rust native 调用  ──→  any_core.rs                         │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

**开发内容**：

```
├── compiler/src/std/
│   ├── std_math.rs         # 修改：仅保留 Layer 1 函数
│   ├── std_string.rs       # 修改：仅保留复杂函数（正则/格式化）
│   ├── std_path.rs         # 修改：删除，全部上移
│   ├── std_encoding.rs     # 修改：删除，全部上移
│   ├── std_builtin.rs      # 修改：仅保留 Layer 1 函数
│   ├── std_time.rs         # 修改：仅保留 syscall 函数
│   └── std_collections.rs  # 修改：仅保留复杂数据结构
│
└── phantom-source/aura/lang/std/
    ├── Math.aura           # 完整 Aura 实现
    ├── String.aura         # 完整 Aura 实现
    ├── Path.aura           # 完整 Aura 实现
    ├── Encoding.aura       # 完整 Aura 实现
    ├── Builtin.aura        # 完整 Aura 实现
    ├── Time.aura           # 完整 Aura 实现
    └── Collections.aura    # 完整 Aura 实现
```

**验收标准**：
- [x] Math.aura 全部函数可调用（三态模式）
- [x] String.aura 全部函数可调用（三态模式）
- [x] Path.aura 全部函数可调用（三态模式）
- [x] Encoding.aura 全部函数可调用（三态模式）
- [x] Builtin.aura 全部函数可调用（三态模式）
- [x] Time.aura 全部函数可调用（三态模式）
- [x] Collections.aura 全部函数可调用（三态模式）
- [x] 保留的 Rust native 函数正常工作
- [x] FFI AOT 直连正常工作（三态模式）
- [x] 功能测试结果一致（三态模式）

**测试用例**：
```aura
// examples/language-test/stdlib_test.aura
fun main() {
    // Math 测试（三态模式都通过）
    assert(Math.abs(-5) == 5)
    assert(Math.min(3, 5) == 3)
    assert(Math.max(3, 5) == 5)
    
    // String 测试（三态模式都通过）
    assert(String.contains("hello", "ell") == true)
    assert(String.split("a,b,c", ",") == ["a", "b", "c"])
    assert(String.replace("hello", "l", "L") == "heLLo")
    
    // Path 测试（三态模式都通过）
    assert(Path.join("/a", "b") == "/a/b")
    assert(Path.split("/a/b/c") == ["/", "a", "b", "c"])
    
    // FileSystem 测试（FFI AOT 直连）
    assert(FileSystem.exists("/tmp") == true)
    assert(FileSystem.writeFile("/tmp/test.txt", "hello") == null)
    assert(FileSystem.readFile("/tmp/test.txt") == "hello")
    
    // Builtin 测试（三态模式都通过）
    assert(toInt("42") == 42)
    assert(toFloat(4) == 4.0)
    assert(toBool(1) == true)
    
    println("✓ 所有标准库测试通过")
}
```

---

### Phase 5: FFI AOT 直连完善（第 12-14 周）

**目标**：完善 FFI AOT 直连支持，优化性能，三态模式全部支持。

**开发内容**：

```
├── compiler/src/codegen/
│   ├── ffi_aot.rs          # 修改：完善 AOT 直连支持
│   │   ├── VM 模式：预加载函数地址
│   │   ├── JIT 模式：生成直接调用指令
│   │   ├── AOT 模式：生成 LLVM IR 直接调用
│   │   ├── 符号延迟解析（JIT/AOT）
│   │   └── 内联缓存优化
│   ├── ffi_cache.rs        # 新增：FFI 调用缓存
│   │   ├── 函数地址预加载
│   │   ├── 内联缓存
│   │   └── 调用计数统计
│   └── ffi_optimize.rs     # 新增：FFI 调用优化
│       ├── 热点检测
│       ├── 内联优化
│       └── 去虚拟化
│
├── compiler/src/vm/
│   ├── ffi_cache.rs        # 新增：VM FFI 缓存
│   │   ├── 预加载函数地址
│   │   ├── 直接调用
│   │   └── 调用计数
│   ├── jit/ffi.rs          # 新增：JIT FFI 支持
│   │   ├── 生成直接调用指令
│   │   ├── PLT 符号解析
│   │   └── 内联缓存
│   └── aot/ffi.rs          # 新增：AOT FFI 支持
│       ├── LLVM IR 直接调用
│       ├── 符号解析
│       └── 优化集成
│
└── loom/src/ffi/
    ├── aot.rs              # 新增：AOT 直连配置
    ├── cache.rs            # 新增：FFI 缓存配置
    └── optimize.rs         # 新增：FFI 优化配置
```

**关键代码**：
```rust
// compiler/src/codegen/ffi_aot.rs
pub fn configure_ffi_aot_direct(
    module: &mut BytecodeModule,
    execution_mode: ExecutionMode,
    config: &FfiAotConfig,
) -> Result<(), String> {
    // 1. 解析所有 extern "c" 声明
    let ffi_decls = module.get_ffi_declarations()?;
    
    // 2. 根据执行模式配置 AOT 直连
    match execution_mode {
        ExecutionMode::Vm => {
            // VM 模式：预加载函数地址
            let mut ffi_cache = FfiCache::new();
            for decl in &ffi_decls {
                let func_addr = ffi_cache.preload_function(&decl.name)?;
                module.set_ffi_address(&decl.name, func_addr);
            }
            module.enable_ffi_aot_direct();
            
            // 启用内联缓存（如果配置允许）
            if config.enable_inline_cache {
                module.enable_ffi_inline_cache();
            }
        }
        ExecutionMode::Jit => {
            // JIT 模式：生成直接调用指令标记
            for decl in &ffi_decls {
                module.mark_ffi_direct_call(&decl.name);
            }
            module.enable_ffi_aot_direct();
            
            // 启用 PLT 符号延迟解析
            if config.enable_plt {
                module.enable_ffi_plt();
            }
        }
        ExecutionMode::Aot => {
            // AOT 模式：生成 LLVM IR 直接调用
            for decl in &ffi_decls {
                module.emit_ffi_direct_call(&decl.name);
            }
            module.enable_ffi_aot_direct();
            
            // 启用 LLVM 优化
            if config.enable_llvm_optimize {
                module.enable_ffi_llvm_optimize();
            }
        }
    }
    
    Ok(())
}

// compiler/src/vm/ffi_cache.rs
pub struct FfiCache {
    function_addresses: HashMap<String, usize>,
    call_sites: HashMap<String, FfiCallSite>,
    inline_cache: InlineCache,
}

impl FfiCache {
    pub fn new() -> Self {
        Self {
            function_addresses: HashMap::new(),
            call_sites: HashMap::new(),
            inline_cache: InlineCache::new(),
        }
    }
    
    pub fn preload_function(&mut self, name: &str) -> Result<usize, String> {
        // 从 libc 或自定义库加载函数地址
        let addr = unsafe { load_function_address(name)? };
        self.function_addresses.insert(name.to_string(), addr);
        Ok(addr)
    }
    
    pub fn call_function(&mut self, name: &str, args: &[Value]) -> Result<Value, String> {
        // 从缓存获取函数地址（无查找开销）
        let func_addr = *self.function_addresses.get(name)
            .ok_or_else(|| format!("FFI function not found: {}", name))?;
        
        // 更新内联缓存
        self.inline_cache.update(name, func_addr);
        
        // 直接调用 C 函数
        let result = unsafe { call_c_function(func_addr, args)? };
        
        // 更新调用计数
        if let Some(call_site) = self.call_sites.get_mut(name) {
            call_site.call_count += 1;
        }
        
        Ok(result)
    }
}
```

**性能优化**：
- VM 模式：预加载函数地址，消除函数指针查找
- JIT 模式：生成直接调用指令，消除间接调用
- AOT 模式：LLVM 优化，内联、去虚拟化、死代码消除
- 内联缓存：加速重复调用
- PLT 符号解析：延迟解析，启动快

**验收标准**：
- [x] C FFI 能调用 libc 函数（三态模式）
- [x] Rust FFI 能调用 Rust native 函数（三态模式）
- [x] FFI AOT 直连能消除间接调用（三态模式）
- [x] VM 模式预加载函数地址正常工作
- [x] JIT 模式生成直接调用指令正常工作
- [x] AOT 模式 LLVM IR 直接调用正常工作
- [x] 内联缓存优化正常工作
- [x] 性能测试通过（三态模式）

**测试用例**：
```aura
// examples/language-test/ffi_test.aura
fun main() {
    // C FFI 测试（三态模式都通过，AOT 直连）
    val fd = fopen("/tmp/test.txt", "w")
    fwrite("hello", 1, 5, fd)
    fclose(fd)
    
    // Rust FFI 测试（三态模式都通过）
    val x = toString(42)
    assert(x == "42")
    
    // FFI AOT 直连测试（三态模式都通过）
    val fd = fopen("/tmp/test2.txt", "r")
    val buf = malloc(1024)
    val n = fread(buf, 1, 1024, fd)
    fclose(fd)
    val content = bufferToString(buf, n)
    free(buf)
    assert(content == "hello")
    
    println("✓ 所有 FFI 测试通过")
}
```

---

### Phase 6: 标准库打包与分发（第 15-16 周）

**目标**：支持标准库以 .auz 格式分发，支持三态模式 + AOT 直连。

**开发内容**：

```
├── loom/src/package/
│   ├── builder.rs          # 新增：标准库打包
│   │   ├── 打包 .auc 文件（VM/JIT 模式）
│   │   ├── 打包机器代码（AOT 模式）
│   │   ├── 打包 .a 静态库（FFI）
│   │   ├── 打包 FFI 函数地址映射
│   │   ├── 生成 manifest
│   │   └── 生成 checksum
│   ├── reader.rs           # 新增：标准库读取
│   │   ├── 解压 .auz 文件
│   │   ├── 验证 checksum
│   │   ├── 提取 .auc 文件/机器代码
│   │   └── 提取 FFI 函数地址映射
│   └── installer.rs        # 新增：标准库安装
│       ├── 安装标准库
│       ├── 更新标准库
│       ├── 卸载标准库
│       └── 验证 FFI 函数可用性
│
└── aura-lang.dev/          # 注册表
    ├── aura-stdlib/
    │   ├── 1.0.0/
    │   │   ├── std.auz           # 标准库制品
    │   │   ├── manifest.json     # 清单文件
    │   │   └── checksum.json     # 校验文件
    │   └── latest.json
    └── ...
```

**标准库制品格式（三态 + AOT 直连）**：
```json
{
  "name": "aura-stdlib",
  "version": "1.0.0",
  "execution-modes": ["vm", "jit", "aot"],
  "ffi-mode": "aot",
  "modules": [
    "aura.lang.std.Math",
    "aura.lang.std.String",
    "aura.lang.std.Path",
    "aura.lang.std.Encoding",
    "aura.lang.std.Builtin",
    "aura.lang.std.Time",
    "aura.lang.std.Collections",
    "aura.lang.std.FileSystem",
    "aura.lang.std.IO",
    "aura.lang.std.Network",
    "aura.lang.std.Coroutine",
    "aura.lang.std.Actor",
    "aura.lang.std.Channel"
  ],
  "artifacts": {
    "vm": {
      "format": "auc",
      "files": ["Math.auc", "String.auc", "Path.auc", // ...]
      "ffi-cache": "ffi_cache.vm.json"  // FFI 函数地址映射
    },
    "jit": {
      "format": "auc",
      "files": ["Math.auc", "String.auc", "Path.auc", // ...]
      "ffi-symbols": "ffi_symbols.jit.json"  // FFI 符号表
    },
    "aot": {
      "format": "native",
      "files": {
        "linux-x86_64": "libstd.a",
        "windows-x86_64": "std.lib",
        "macos-x86_64": "libstd.a"
      },
      "ffi-symbols": "ffi_symbols.aot.json"  // FFI 符号表
    }
  },
  "ffi": {
    "cffi": {
      "lib": "aura_std_cffi",
      "files": {
        "linux-x86_64": "libaura_std_cffi.so",
        "windows-x86_64": "aura_std_cffi.dll",
        "macos-x86_64": "libaura_std_cffi.dylib"
      }
    },
    "rustffi": {
      "modules": ["aura.lang.std.Any", "aura.lang.std.Type", "aura.lang.std.Value"]
    },
    "aot": {
      "inline": true,
      "optimize": 3,
      "static-link": false
    }
  },
  "checksum": {
    "sha256": "..."
  }
}
```

**CLI 命令**：
```bash
# 编译标准库（三态 + AOT 直连）
loom build --lib --execution-mode auto --ffi-mode aot --output std.auz

# 安装标准库
aura install std.auz

# 更新标准库
aura update aura-stdlib

# 卸载标准库
aura uninstall aura-stdlib

# 查看标准库信息
aura info aura-stdlib
```

**验收标准**：
- [x] 能打包标准库为 .auz 格式（三态模式）
- [x] 能打包 FFI 函数地址映射
- [x] 能安装标准库
- [x] 能更新标准库
- [x] 能卸载标准库
- [x] 能验证 checksum
- [x] 能分发标准库制品
- [x] 支持三态模式 + AOT 直连

**测试用例**：
```rust
// loom/tests/package_test.rs
#[test]
fn test_stdlib_package_vm() {
    // 测试 VM 模式打包（含 FFI 函数地址映射）
}

#[test]
fn test_stdlib_package_jit() {
    // 测试 JIT 模式打包（含 FFI 符号表）
}

#[test]
fn test_stdlib_package_aot() {
    // 测试 AOT 模式打包（含 FFI 符号表）
}

#[test]
fn test_stdlib_install() {
    // 测试标准库安装
}

#[test]
fn test_stdlib_update() {
    // 测试标准库更新
}

#[test]
fn test_ffi_function_availability() {
    // 测试 FFI 函数可用性验证
}
```

---

### Phase 7: 集成测试与性能验证（第 17-20 周）

**目标**：全面验证功能正确性和性能，覆盖三态模式 + AOT 直连。

**测试内容**：

```
├── 功能测试（三态模式 + AOT 直连）
│   ├── 单元测试
│   │   ├── Math 模块测试
│   │   ├── String 模块测试
│   │   ├── Path 模块测试
│   │   ├── Encoding 模块测试
│   │   ├── Builtin 模块测试
│   │   ├── Time 模块测试
│   │   ├── Collections 模块测试
│   │   ├── FileSystem 模块测试（FFI AOT 直连）
│   │   ├── IO 模块测试（FFI AOT 直连）
│   │   └── Network 模块测试（FFI AOT 直连）
│   ├── 集成测试
│   │   ├── 标准库调用测试
│   │   ├── 跨模块调用测试
│   │   ├── FFI AOT 直连测试（三态模式）
│   │   ├── 多模块加载测试
│   │   └── 模式切换测试（VM → JIT）
│   └── 回归测试
│       ├── 现有测试用例通过（三态模式）
│       ├── 向后兼容性测试
│       └── 边界条件测试
│
├── 性能测试（三态模式 + AOT 直连）
│   ├── 基准测试
│   │   ├── Math 性能基准
│   │   ├── String 性能基准
│   │   ├── Path 性能基准
│   │   ├── Encoding 性能基准
│   │   ├── Collections 性能基准
│   │   └── FFI 性能基准（AOT 直连 vs 间接调用）
│   ├── 对比测试
│   │   ├── Rust native vs Aura VM
│   │   ├── Rust native vs Aura JIT
│   │   ├── Rust native vs Aura AOT
│   │   ├── FFI 间接调用 vs FFI AOT 直连
│   │   └── 三态模式性能对比
│   ├── 模式切换测试
│   │   ├── VM → JIT 切换开销
│   │   ├── JIT 热点检测准确性
│   │   └── AOT 编译时间
│   └── 压力测试
│       ├── 大规模数据测试
│       ├── 长时间运行测试
│       └── 并发测试
│
└── 兼容性测试
    ├── 平台测试
    │   ├── Linux x86_64
    │   ├── Windows x86_64
    │   └── macOS x86_64
    ├── 编译器版本测试
    │   ├── 当前版本
    │   └── 旧版本兼容
    └── 标准库版本测试
        ├── 当前版本
        └── 旧版本兼容
```

**性能基准**：
```rust
// benches/stdlib_bench.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_math_vm(c: &mut Criterion) {
    c.bench("Math.abs [VM]", || {
        vm_execute("Math.abs(-42)")
    });
}

fn bench_math_jit(c: &mut Criterion) {
    c.bench("Math.abs [JIT]", || {
        jit_execute("Math.abs(-42)")
    });
}

fn bench_math_aot(c: &mut Criterion) {
    c.bench("Math.abs [AOT]", || {
        aot_execute("Math.abs(-42)")
    });
}

fn bench_ffi_indirect(c: &mut Criterion) {
    c.bench("FFI indirect call", || {
        // 传统间接调用
        ffi_indirect_call("fopen", &["/tmp/test.txt", "w"])
    });
}

fn bench_ffi_aot_direct(c: &mut Criterion) {
    c.bench("FFI AOT direct call", || {
        // AOT 直连调用
        ffi_aot_direct_call("fopen", &["/tmp/test.txt", "w"])
    });
}

criterion_group!(
    benches,
    bench_math_vm,
    bench_math_jit,
    bench_math_aot,
    bench_ffi_indirect,
    bench_ffi_aot_direct
);
criterion_main!(benches);
```

**性能报告模板**：
```markdown
# 性能报告

## 测试环境
- CPU: Intel i7-12700
- Memory: 32GB
- OS: Windows 11
- Compiler: Aura 1.0.0

## 三态模式性能对比

| 模块 | 函数 | Rust native | Aura VM | Aura JIT | Aura AOT | 最优 |
|------|------|-------------|---------|----------|----------|------|
| Math | abs | 1.0x | 3.2x | 1.8x | 0.9x | AOT |
| Math | min | 1.0x | 3.5x | 1.6x | 0.8x | AOT |
| String | contains | 1.0x | 4.1x | 2.2x | 0.7x | AOT |
| String | split | 1.0x | 5.2x | 2.8x | 0.6x | AOT |
| Path | join | 1.0x | 4.5x | 2.5x | 0.5x | AOT |
| Path | split | 1.0x | 5.8x | 3.2x | 0.4x | AOT |

## FFI AOT 直连性能对比

| 调用方式 | 相对性能 | 说明 |
|----------|----------|------|
| 间接调用 | 1.0x | 传统 FFI，函数指针查找 + 间接调用 |
| AOT 直连（VM） | 1.5x | 预加载函数地址，直接调用 |
| AOT 直连（JIT） | 2.0x | 生成直接调用指令，无间接调用 |
| AOT 直连（AOT） | 2.5x | LLVM 优化，内联、去虚拟化 |

## 模式切换开销
| 切换类型 | 开销 | 说明 |
|----------|------|------|
| VM → JIT | 50-100ms | 热点检测方法编译 |
| JIT → VM | 0ms | 去优化（deoptimization） |
| AOT 编译 | 100-500ms | 生成机器代码 |

## 结论
- AOT 模式性能最优（平均 1.2x-1.5x 优于 Rust native）
- FFI AOT 直连性能提升显著（1.5x-2.5x 优于间接调用）
- JIT 模式性能中等（平均 1.5x-2x 慢于 Rust native）
- VM 模式性能最低（平均 3x-6x 慢于 Rust native）
- 推荐：性能敏感代码用 AOT 模式 + AOT 直连，交互式应用用 JIT，开发调试用 VM
```

**验收标准**：
- [x] 所有功能测试通过（三态模式 + AOT 直连）
- [x] 性能测试完成（三态模式 + AOT 直连）
- [x] 兼容性测试通过
- [x] 生成性能报告
- [x] 修复发现的问题

---

## 6. 风险与缓解

| 风险 | 影响 | 概率 | 缓解措施 |
|------|------|------|----------|
| Bootstrap 循环依赖 | 编译器依赖标准库，标准库依赖编译器 | 高 | 预编译最小引导层，两阶段编译 |
| JIT 性能不稳定 | JIT 编译开销不可预测 | 中 | 设置合理热点阈值，提供 AOT 选项 |
| AOT 编译时间长 | 影响开发体验 | 中 | 增量编译，后台编译 |
| 三态模式一致性 | 三态模式行为不一致 | 中 | 统一的语义规范，充分的测试 |
| FFI 兼容性 | C FFI 在不同平台行为不同 | 中 | 平台抽象层，条件编译 |
| FFI AOT 直连符号解析 | 符号解析失败导致运行时错误 | 中 | 启动时预加载，延迟解析兜底 |
| 标准库版本冲突 | 应用依赖不同版本标准库 | 低 | 语义版本控制，依赖解析 |
| 模式切换开销 | VM → JIT 切换有性能损失 | 低 | 智能热点检测，渐进式优化 |
| 内联缓存失效 | 内联缓存命中率低导致性能回退 | 低 | 自适应缓存策略，冷启动预热 |

---

## 7. 里程碑与交付物

### 7.1 里程碑

| 里程碑 | 时间 | 交付物 |
|--------|------|--------|
| M1: Bootstrap 完成 | Week 2 | 最小 VM/JIT/AOT 核心 + Layer 1 函数 + FFI AOT 直连支持 |
| M2: 标准库编译完成 | Week 4 | .auc 文件 + 机器代码 + 标准库索引 + FFI 函数映射 |
| M3: 编译器集成完成 | Week 7 | 三态模式支持 + 标准库链接 + FFI AOT 直连 |
| M4: Rust native 替换完成 | Week 11 | 7 个模块 Aura 实现（三态模式 + AOT 直连） |
| M5: FFI AOT 直连完善 | Week 14 | 三态模式 FFI AOT 直连 + 性能优化 |
| M6: 打包分发完成 | Week 16 | .auz 格式（三态）+ 注册表 + FFI 函数映射 |
| M7: 全面验证完成 | Week 20 | 测试报告 + 性能报告（三态模式 + AOT 直连） |

### 7.2 交付物清单

```
├── 代码
│   ├── compiler/src/bootstrap/          # Bootstrap 最小引导层
│   │   ├── vm_core.rs                   # VM 核心（含 FFI AOT 直连）
│   │   ├── jit_core.rs                  # JIT 核心（含 FFI AOT 直连）
│   │   └── aot_core.rs                  # AOT 核心（含 FFI AOT 直连）
│   ├── compiler/src/codegen/            # 代码生成
│   │   ├── link_stdlib.rs               # 标准库链接
│   │   ├── resolve_stdlib.rs            # 标准库解析
│   │   ├── ffi_aot.rs                   # FFI AOT 直连支持（三态模式）
│   │   ├── ffi_cache.rs                 # FFI 调用缓存
│   │   ├── ffi_optimize.rs              # FFI 调用优化
│   │   └── execution.rs                 # 执行模式选择
│   ├── compiler/src/vm/                 # 虚拟机
│   │   ├── multi_module.rs              # 多模块支持
│   │   ├── ffi_cache.rs                 # VM FFI 缓存
│   │   ├── jit/ffi.rs                   # JIT FFI 支持
│   │   └── aot/ffi.rs                   # AOT FFI 支持
│   ├── loom/src/plugin/convention.rs    # aura-stdlib 插件改造
│   ├── loom/src/task/compile_stdlib.rs  # 标准库编译任务
│   ├── loom/src/stdlib/mod.rs           # 标准库管理
│   ├── loom/src/ffi/                    # FFI 配置（AOT 直连）
│   └── loom/src/package/                # 标准库打包分发
│
├── 标准库
│   ├── phantom-source/aura/lang/std/    # 标准库 Aura 源码
│   │   ├── Math.aura
│   │   ├── String.aura
│   │   ├── Path.aura
│   │   ├── Encoding.aura
│   │   ├── Builtin.aura
│   │   ├── Time.aura
│   │   ├── Collections.aura
│   │   ├── FileSystem.aura
│   │   ├── IO.aura
│   │   ├── Network.aura
│   │   ├── Coroutine.aura
│   │   ├── Actor.aura
│   │   └── Channel.aura
│   └── compiler/src/std/cffi/           # C FFI 实现
│       ├── aura_std_cffi.c
│       ├── aura_std_cffi.h
│       └── CMakeLists.txt
│
├── 测试
│   ├── tests/bootstrap_test.rs          # Bootstrap 测试（含 FFI AOT 直连）
│   ├── tests/stdlib_test.rs             # 标准库测试（三态模式 + AOT 直连）
│   ├── tests/ffi_test.rs                # FFI 测试（三态模式 + AOT 直连）
│   ├── tests/package_test.rs            # 打包测试（三态模式）
│   └── benches/stdlib_bench.rs          # 性能基准（三态模式 + AOT 直连）
│
├── 文档
│   ├── docs/完全Aura化技术方案.md       # 本方案
│   ├── docs/三态执行模式.md             # 三态模式说明
│   ├── docs/FFI-AOT直连设计.md          # FFI AOT 直连设计
│   ├── docs/标准库迁移指南.md           # 迁移指南
│   ├── docs/FFI使用指南.md              # FFI 使用指南
│   ├── docs/性能优化指南.md             # 性能优化指南
│   └── docs/标准库API.md                # API 文档
│
└── 制品
    ├── aura-stdlib-1.0.0.auz            # 标准库制品（三态模式）
    ├── manifest.json                    # 清单文件（含 FFI 函数映射）
    └── checksum.json                    # 校验文件
```

### 7.3 时间线总览

```
Week 1-2:   Phase 1 - Bootstrap 最小引导层
          ─────────────────────────────────
          ├── 提取最小 VM 核心（含 FFI AOT 直连）
          ├── 提取最小 JIT 核心（含 FFI AOT 直连）
          ├── 提取最小 AOT 核心（含 FFI AOT 直连）
          ├── Any 核心虚方法
          ├── 类型内省核心
          ├── 空值/数值检查
          └── 内存管理 + 运行时函数

Week 3-4:   Phase 2 - loom aura-stdlib 插件改造
          ─────────────────────────────────
          ├── 扫描 phantom-source 目录
          ├── 编译 .aura 为 .auc（VM/JIT）
          ├── 编译 .auc 为机器代码（AOT）
          ├── 编译 C FFI 库（AOT 直连）
          └── 生成标准库索引 + FFI 函数映射

Week 5-7:   Phase 3 - 编译器集成标准库
          ─────────────────────────────────
          ├── 加载标准库 .auc 文件
          ├── 链接标准库符号
          ├── 解析标准库调用
          ├── 多模块 VM 支持
          ├── JIT 热点检测
          ├── AOT 编译集成
          └── FFI AOT 直连支持（三态模式）

Week 8-11:  Phase 4 - 替换 Rust native 实现
          ─────────────────────────────────
          ├── Math.aura 完整实现
          ├── String.aura 完整实现
          ├── Path.aura 完整实现
          ├── Encoding.aura 完整实现
          ├── Builtin.aura 完整实现
          ├── Time.aura 完整实现
          └── Collections.aura 完整实现

Week 12-14: Phase 5 - FFI AOT 直连完善
          ─────────────────────────────────
          ├── VM 模式：预加载函数地址
          ├── JIT 模式：生成直接调用指令
          ├── AOT 模式：LLVM IR 直接调用
          ├── 内联缓存优化
          └── 符号延迟解析

Week 15-16: Phase 6 - 标准库打包与分发
          ─────────────────────────────────
          ├── .auz 格式支持（三态模式）
          ├── FFI 函数地址映射
          ├── 标准库打包
          ├── 标准库安装/更新/卸载
          └── 注册表支持

Week 17-20: Phase 7 - 集成测试与性能验证
          ─────────────────────────────────
          ├── 功能测试（三态模式 + AOT 直连）
          ├── 性能测试（三态模式 + AOT 直连）
          ├── 兼容性测试
          └── 生成性能报告
```

---

## 附录

### A. FFI 声明语法

```aura
// C FFI 声明（AOT 直连）
extern "c" "libc" fun fopen(path: String, mode: String): Pointer
extern "c" "libc" fun fread(buf: Pointer, size: Int, count: Int, stream: Pointer): Int
extern "c" "libc" fun fclose(stream: Pointer): Int
extern "c" "libc" fun malloc(size: Int): Pointer
extern "c" "libc" fun free(ptr: Pointer): Unit

// 自定义 C 库
extern "c" "mylib" fun my_function(arg: Int): Int

// Rust FFI 声明（Layer 1 函数）
extern "rust" fun any_toString(value: Any): String
extern "rust" fun any_equals(a: Any, b: Any): Boolean

// 混合使用示例
fun readText(path: String): String {
    // C FFI，AOT 直连
    val fd = fopen(path, "r")
    if fd == null then return ""
    
    val buf = malloc(1024)      // C FFI，AOT 直连
    val n = fread(buf, 1, 1024, fd)  // C FFI，AOT 直连
    fclose(fd)                  // C FFI，AOT 直连
    
    val result = bufferToString(buf, n)
    free(buf)                   // C FFI，AOT 直连
    return result
}
```

### B. 配置文件

```toml
# aura.toml（默认配置）
[build]
execution-mode = "auto"  # vm | jit | aot | auto（默认自动选择）
ffi-mode = "aot"         # aot | cffi | rustffi（默认 AOT 直连）

[build.ffi.aot]
inline = true            # 允许内联 C 代码
optimize = 3             # 优化级别（0-3）
static-link = false      # 是否静态链接 libc
enable-inline-cache = true  # 启用内联缓存
enable-plt = true        # 启用 PLT 符号延迟解析

[build.ffi.cffi]
lib = "aura_std_cffi"    # C FFI 库名称
include = ["compiler/src/std/cffi"]

[build.ffi.rustffi]
modules = [
    "aura.lang.std.Any",
    "aura.lang.std.Type",
    "aura.lang.std.Value"
]

# 三态模式自动选择策略
[build.auto-mode]
vm = "开发调试、交互式应用"
jit = "桌面应用、服务器应用"
aot = "高性能计算、嵌入式"

# 自动降级策略
[build.fallback]
aot-failed = "jit"       # AOT 编译失败降级为 JIT
jit-failed = "vm"        # JIT 编译失败降级为 VM
ffi-failed = "rustffi"   # FFI AOT 直连失败降级为 Rust FFI
```

### C. 性能优化建议

| 场景 | 推荐模式 | FFI 模式 | 说明 |
|------|----------|----------|------|
| 开发调试 | VM | AOT 直连 | 启动快，调试友好 |
| 桌面应用 | JIT | AOT 直连 | 性能中等，自适应优化 |
| 服务器应用 | JIT | AOT 直连 | 性能中等，启动快 |
| 高性能计算 | AOT | AOT 直连 | 性能最优，编译时间长 |
| 嵌入式 | AOT | AOT 直连（静态链接） | 性能最优，体积小 |
| 混合应用 | auto | AOT 直连 | 自动选择最优模式 |

### D. 常见问题

**Q: FFI AOT 直连失败怎么办？**
A: 自动降级为 Rust FFI，然后降级为 C FFI 间接调用。

**Q: 如何查看 FFI 调用性能？**
A: 使用 `aura profile --ffi` 查看 FFI 调用统计。

**Q: 如何自定义 FFI 库？**
A: 在 `aura.toml` 中配置 `[build.ffi.cffi]`，指定库名称和包含目录。

**Q: 三态模式可以混用吗？**
A: 可以，同一应用可以混合使用 VM/JIT/AOT 模式，但标准库需要统一。

**Q: 如何迁移旧代码？**
A: 旧代码继续工作，Rust native 实现保留。逐步迁移纯逻辑模块到 Aura。

---

**文档结束**
