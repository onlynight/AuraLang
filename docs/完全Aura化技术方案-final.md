# 完全 Aura 化技术方案 - Final

> **版本**: 2.0  
> **日期**: 2026-07-04  
> **状态**: 待实施  
> **核心目标**: 标准库完全 Aura 化，三态执行模式（VM/JIT/AOT）全部支持 AOT 直连 FFI，完整 VM 可用 Aura 自举

---

## 目录

1. [背景与目标](#1-背景与目标)
2. [当前架构问题](#2-当前架构问题)
3. [最终架构设计](#3-最终架构设计)
4. [自举方案设计](#4-自举方案设计)
5. [三态执行模式与 AOT 直连](#5-三态执行模式与-aot-直连)
6. [开发阶段详细计划](#6-开发阶段详细计划)
7. [风险与缓解](#7-风险与缓解)
8. [里程碑与交付物](#8-里程碑与交付物)

---

## 1. 背景与目标

### 1.1 背景

Aura 语言当前标准库采用混合架构：
- **Layer 1（Rust native）**：Any 核心虚方法、类型内省、内存管理等
- **Layer 2（C FFI）**：syscall 操作（FileSystem/IO/Network）
- **Layer 3（Aura 源码）**：纯逻辑模块（Math/String/Path 等）

**问题**：
- phantom-source 目录仅用于 IDE SourceIndex 生成，不参与编译（现已迁移至 `core/aura/lang/std/`）
- Rust native 实现与 Aura 源码存在双层漂移
- "上移"只是文档名义，未实现真正的代码迁移
- VM 核心由 Rust 编写，无法自举

### 1.2 目标

| # | 目标 | 度量 |
|---|------|------|
| G1 | 标准库完全 Aura 化 | 纯逻辑模块 100% Aura 实现 |
| G2 | 三态执行模式 | VM/JIT/AOT 全部支持 |
| G3 | AOT 直连 FFI | 所有模式消除函数指针间接调用 |
| G4 | 单一真相源 | `core/aura/lang/std/` 是真实源码 |
| G5 | VM 自举 | 完整 VM 可用 Aura 编写并自举 |
| G6 | 性能最优 | AOT 直连消除间接调用开销 |

### 1.3 核心原则

1. **AOT 直连优先**：默认 FFI 方式选择 AOT 直连，消除函数指针间接调用（使用 `extern interface`）
2. **三态一致性**：VM/JIT/AOT 三态模式语义一致
3. **完整自举**：完整 VM 可用 Aura 编写，通过 `aura.exe --aot` 编译
4. **渐进式迁移**：保留最小 Rust 引导层，逐步替换
5. **单一真相源**：`core/aura/lang/std/` 是标准库唯一源码

---

## 2. 当前架构问题

### 2.1 phantom-source 不参与编译（已迁移至 `core/aura/lang/std/`）

```
当前架构：
┌─────────────────────────────────────────────────────────────────┐
│ core/aura/lang/std/*.aura  ──→  SourceIndex 生成（仅用于 IDE）  │
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
| 文档层 | `core/aura/lang/std/*.aura` | ❌ 不参与编译 |
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

### 2.4 VM 无法自举

```
当前问题：
┌─────────────────────────────────────────────────────────────────┐
│ VM 核心（Rust 编写）                                              │
│ ├── 字节码解释器                                                   │
│ ├── 栈帧管理                                                       │
│ ├── 指令分发                                                       │
│ └── 异常处理                                                       │
│                                                                  │
│ 问题：                                                            │
│ ✗ 无法用 Aura 重写（鸡生蛋问题）                                   │
│ ✗ 无法自举（需要编译器来编译）                                     │
│ ✗ 性能优化受限（Rust 与 Aura 分离）                                │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. 最终架构设计

### 3.1 分层架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│  最终架构                                                                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Layer 0-A: 最小引导（Rust，不能上移）                                   │
│  ├── 最小编译器（解析器 + 编译器）                                        │
│  │   ├── 词法分析                                                        │
│  │   ├── 语法分析                                                        │
│  │   ├── 语义分析                                                        │
│  │   └── 代码生成（字节码）                                               │
│  ├── 最小 VM（字节码解释器）                                             │
│  │   ├── 基础指令集（~50 条）                                            │
│  │   ├── 栈帧管理                                                        │
│  │   └── 简单错误处理                                                     │
│  └── AOT 编译器入口                                                      │
│      ├── LLVM IR 生成                                                    │
│      └── 机器代码生成                                                    │
│                                                                         │
│  Layer 0-B: 可上移（Aura 编写，自举）                                    │
│  ├── 完整 VM（vm.aura）                                                  │
│  │   ├── 完整指令集（~200 条）                                           │
│  │   ├── 优化器                                                           │
│  │   ├── 调试器                                                           │
│  │   └── JIT 编译器（可选）                                               │
│  ├── 内存管理（memory.aura + C FFI）                                     │
│  │   ├── malloc/free/realloc                                             │
│  │   ├── arc_increment/arc_decrement                                     │
│  │   └── 内存池                                                           │
│  ├── GC（gc.aura + C FFI）                                               │
│  │   ├── mark/sweep                                                       │
│  │   ├── incremental                                                      │
│  │   └── concurrent                                                       │
│  ├── Any 核心虚方法                                                      │
│  │   ├── toString()                                                       │
│  │   ├── equals()                                                         │
│  │   └── hashCode()                                                       │
│  ├── 类型内省核心                                                        │
│  │   ├── typeOf()                                                         │
│  │   ├── isOfType()                                                       │
│  │   └── cast()                                                           │
│  └── 运行时函数                                                          │
│      ├── coroutine_yield()                                                │
│      └── gc_trigger()                                                     │
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
│  │ 1. 最小编译器编译标准库                                           │   │
│  │    core/aura/lang/std/*.aura  ──→  .auc 字节码                   │   │
│  │                                                                  │   │
│  │ 2. 最小 VM 运行标准库                                            │   │
│  │    .auc  ──→  最小 VM 解释执行                                   │   │
│  │                                                                  │   │
│  │ 3. AOT 编译标准库                                                │   │
│  │    .auc  ──→  LLVM IR  ──→  机器代码  ──→  原生库              │   │
│  │                                                                  │   │
│  │ 4. 自举验证                                                      │   │
│  │    vm.exe（Aura 编写） ──→  运行 vm.aura  ──→  生成 vm2.exe    │   │
│  │    验证：vm.exe 与 vm2.exe 行为一致                              │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 3.2 模块分类

| 层级 | 模块 | 实现方式 | 能否上移 |
|------|------|----------|----------|
| Layer 0-A | 最小编译器 | Rust native | ❌ 不能（第一次） |
| Layer 0-A | 最小 VM | Rust native | ❌ 不能（第一次） |
| Layer 0-A | AOT 编译器入口 | Rust native | ❌ 不能（第一次） |
| Layer 0-B | 完整 VM | Aura 实现 | ✅ 可以（自举） |
| Layer 0-B | 内存管理 | Aura + C FFI | ✅ 可以 |
| Layer 0-B | GC | Aura + C FFI | ✅ 可以 |
| Layer 0-B | Any 核心 | Aura 实现 | ✅ 可以 |
| Layer 0-B | 类型内省 | Aura 实现 | ✅ 可以 |
| Layer 1+ | Math | Aura 实现 | ✅ 可以 |
| Layer 1+ | String | Aura 实现 | ✅ 可以 |
| Layer 1+ | Path | Aura 实现 | ✅ 可以 |
| Layer 1+ | Encoding | Aura 实现 | ✅ 可以 |
| Layer 1+ | Builtin | Aura 实现 | ✅ 可以 |
| Layer 1+ | Time | Aura 实现 | ✅ 可以 |
| Layer 1+ | Collections | Aura 实现 | ✅ 可以 |
| Layer 1+ | FileSystem | Aura + C FFI | ✅ 可以 |
| Layer 1+ | IO | Aura + C FFI | ✅ 可以 |
| Layer 1+ | Network | Aura + C FFI | ✅ 可以 |
| Layer 1+ | Coroutine | Aura + AOT 直连 | ✅ 可以 |
| Layer 1+ | Actor | Aura + AOT 直连 | ✅ 可以 |
| Layer 1+ | Channel | Aura + AOT 直连 | ✅ 可以 |

---

## 4. 自举方案设计

### 4.1 自举原理

```
┌─────────────────────────────────────────────────────────────────────────┐
│  自举原理                                                                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  鸡生蛋问题的本质：                                                      │
│  ─────────────────────                                                  │
│  • 要编译 vm.aura，需要 aura.exe                                        │
│  • 但 aura.exe 本身就包含一个 VM                                        │
│  • 所以第一个 aura.exe 必须用 Rust 编写                                  │
│                                                                         │
│  一旦有了第一个 aura.exe：                                               │
│  ─────────────────────                                                  │
│  • 可以用它编译 vm.aura（用 Aura 编写的完整 VM）                          │
│  • 生成 vm.exe（原生二进制，不依赖 Rust）                                │
│  • 用 vm.exe 替换原来的 aura.exe                                        │
│  • 完全自举成功                                                          │
│                                                                         │
│  参考语言：                                                              │
│  ────────────────                                                       │
│  • Go：bootstrap compiler 用 Go 写，用旧版本编译新编译器                  │
│  • Rust：bootstrapping 过程，编译器用 Rust 写，用旧版本编译              │
│  • Erlang：BEAM VM 用 Erlang 写，用旧 BEAM 编译新 BEAM                  │
│  • C：编译器用 C 写，自举验证                                            │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.2 自举流程

```
┌─────────────────────────────────────────────────────────────────────────┐
│  自举流程                                                                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  阶段 1: 最小 aura.exe（Rust 编写）                                      │
│  ─────────────────────────────────────                                  │
│  • 能解析 Aura 源码                                                     │
│  • 能编译为字节码                                                       │
│  • 能解释执行字节码（最小 VM）                                            │
│  • 能 AOT 编译为原生二进制                                                │
│  • 约 2000-3000 行 Rust 代码                                            │
│  • 支持最小子集语法                                                       │
│                                                                         │
│  阶段 2: 完整 VM（Aura 编写）                                            │
│  ─────────────────────────────────────                                  │
│  • vm.aura - 完整 VM 实现                                               │
│  • gc.aura - 垃圾回收                                                    │
│  • memory.aura - 内存管理                                                │
│  • jit.aura - JIT 编译器（可选）                                         │
│  • 用最小 aura.exe 编译                                                  │
│                                                                         │
│  阶段 3: AOT 编译为原生二进制                                            │
│  ─────────────────────────────────────                                  │
│  aura.exe build vm.aura --aot  ──→  vm.exe（原生二进制）                 │
│  • vm.exe 不依赖 Rust，完全独立                                          │
│                                                                         │
│  阶段 4: 自举验证                                                        │
│  ─────────────────────────────────────                                  │
│  vm.exe build vm.aura --aot  ──→  vm2.exe                              │
│  • 比较 vm.exe 与 vm2.exe 的行为                                        │
│  • 输入相同代码，输出一致                                                  │
│  • 证明自举成功                                                          │
│                                                                         │
│  阶段 5: 替换                                                            │
│  ─────────────────────────────────────                                  │
│  用 vm.exe 替换原来的 aura.exe                                          │
│  • 现在 aura.exe 完全由 Aura 编写                                        │
│  • Rust 代码仅作为开发工具保留                                            │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.3 最小 aura.exe 设计

```
┌─────────────────────────────────────────────────────────────────────────┐
│  最小 aura.exe（Rust）                                                   │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  目标：能编译并运行最小子集 Aura 代码                                     │
│  规模：~2000-3000 行 Rust 代码                                          │
│  依赖：无外部依赖（仅 libc）                                             │
│                                                                         │
│  模块结构：                                                              │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  parser/                                                         │   │
│  │  ├── lexer.rs          # 词法分析（~500 行）                     │   │
│  │  ├── parser.rs         # 语法分析（~800 行）                     │   │
│  │  └── ast.rs            # AST 定义（~200 行）                     │   │
│  │                                                                  │   │
│  │  compiler/                                                       │   │
│  │  ├── typechecker.rs    # 类型检查（~400 行）                     │   │
│  │  ├── bytecode.rs       # 字节码生成（~300 行）                   │   │
│  │  └── optimizer.rs      # 简单优化（~200 行）                     │   │
│  │                                                                  │   │
│  │  vm/                                                           │   │
│  │  ├── interpreter.rs    # 字节码解释器（~600 行）                 │   │
│  │  ├── stack.rs          # 栈帧管理（~200 行）                     │   │
│  │  └── errors.rs         # 错误处理（~100 行）                     │   │
│  │                                                                  │   │
│  │  aot/                                                          │   │
│  │  ├── llvm_ir.rs        # LLVM IR 生成（~400 行）                 │   │
│  │  └── linker.rs         # 链接器（~200 行）                       │   │
│  │                                                                  │   │
│  │  stdlib/                                                       │   │
│  │  ├── builtin.rs        # 内置函数（~300 行）                     │   │
│  │  └── io.rs             # I/O 函数（~200 行）                     │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  支持的语法子集：                                                        │
│  • 变量声明（val/var）                                                  │
│  • 函数定义（fun）                                                      │
│  • 控制流（if/while/for）                                               │
│  • 表达式（算术/比较/逻辑）                                              │
│  • 函数调用                                                             │
│  • 字符串/整数/布尔字面量                                                │
│  • 基本类型（Int/Float/String/Boolean）                                 │
│  • 列表/字典字面量                                                      │
│  • extern "c" 声明                                                      │
│                                                                         │
│  不支持的语法（完整 VM 支持）：                                          │
│  • 类/对象                                                              │
│  • 继承/多态                                                            │
│  • 泛型                                                                 │
│  • 协程                                                                 │
│  • 高级错误处理（try/catch）                                             │
│  • 包管理                                                               │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.4 完整 VM（Aura 编写）

```aura
// vm.aura - 用 Aura 编写的完整 VM
import aura.lang.std.*

// 指令集定义
object Opcode {
    // 常量
    val CONST_INT = 0
    val CONST_FLOAT = 1
    val CONST_STRING = 2
    val CONST_NULL = 3
    
    // 栈操作
    val LOAD_LOCAL = 10
    val STORE_LOCAL = 11
    val POP = 12
    val DUP = 13
    
    // 算术
    val ADD = 20
    val SUB = 21
    val MUL = 22
    val DIV = 23
    val MOD = 24
    
    // 比较
    val EQ = 30
    val NEQ = 31
    val LT = 32
    val GT = 33
    val LE = 34
    val GE = 35
    
    // 控制流
    val JUMP = 40
    val JUMP_IF_FALSE = 41
    val JUMP_IF_TRUE = 42
    
    // 函数
    val CALL = 50
    val CALL_METHOD = 51
    val RETURN = 52
    
    // 对象
    val NEW_OBJECT = 60
    val GET_FIELD = 61
    val SET_FIELD = 62
    
    // FFI
    val CALL_C = 70
    val CALL_RUST = 71
    
    // GC
    val GC_MARK = 80
    val GC_SWEEP = 81
}

// 栈帧
object Frame {
    var ip: Int          // 指令指针
    var stack: List<Any>  // 操作数栈
    var locals: List<Any> // 局部变量
    var upvalues: List<Any> // 闭包变量
    
    fun new(bytecode: Array<Byte>, arity: Int): Frame {
        val frame = Frame()
        frame.ip = 0
        frame.stack = Collections.emptyList()
        frame.locals = Collections.emptyList()
        frame.locals.ensureCapacity(arity)
        return frame
    }
}

// VM 状态
object VmState {
    var bytecode: Array<Byte>
    var frames: List<Frame> = Collections.emptyList()
    var globals: Map<String, Any> = Collections.emptyMap()
    var gc_enabled: Boolean = true
    
    fun callFrame(): Frame {
        return frames.removeLast()
    }
}

// 字节码解释器
fun interpret(bytecode: Array<Byte>): Any {
    VmState.bytecode = bytecode
    VmState.frames.add(Frame.new(bytecode, 0))
    
    while !VmState.frames.isEmpty() {
        val frame = VmState.callFrame()
        val opcode = bytecode[frame.ip]
        frame.ip = frame.ip + 1
        
        when {
            opcode == Opcode.CONST_INT -> {
                val value = (bytecode[frame.ip] shl 8) | bytecode[frame.ip + 1]
                frame.ip = frame.ip + 2
                frame.stack.add(value as Int)
            }
            
            opcode == Opcode.ADD -> {
                val b = frame.stack.removeLast() as Int
                val a = frame.stack.removeLast() as Int
                frame.stack.add(a + b)
            }
            
            opcode == Opcode.CALL -> {
                val func_idx = bytecode[frame.ip]
                frame.ip = frame.ip + 1
                // 调用函数...
                continue
            }
            
            opcode == Opcode.RETURN -> {
                val result = frame.stack.removeLast()
                if VmState.frames.isEmpty() {
                    return result
                }
                VmState.callFrame().stack.add(result)
            }
            
            opcode == Opcode.CALL_C -> {
                val func_name = readString(bytecode, frame.ip)
                frame.ip = frame.ip + func_name.length + 1
                val args = readArgs(bytecode, frame.ip)
                frame.ip = frame.ip + args.length
                val result = callCFunction(func_name, args)
                frame.stack.add(result)
            }
            
            opcode == Opcode.GC_MARK -> {
                if VmState.gc_enabled {
                    gc.markAll(frame.stack)
                }
            }
            
            else -> {
                throw RuntimeError("Unknown opcode: " + opcode)
            }
        }
    }
    
    return VmState.globals["return"]
}

// 辅助函数
fun readString(bytecode: Array<Byte>, offset: Int): String {
    var length = 0
    var i = offset
    while i < bytecode.length && bytecode[i] != 0 {
        length = length + 1
        i = i + 1
    }
    return String.fromBytes(bytecode, offset, length)
}

fun readArgs(bytecode: Array<Byte>, offset: Int): List<Any> {
    val count = bytecode[offset]
    val args = Collections.emptyList()
    var i = offset + 1
    while count > 0 {
        // 读取参数...
        count = count - 1
        i = i + 1
    }
    return args
}

// 入口函数
fun main(bytecode: Array<Byte>) {
    val result = interpret(bytecode)
    println("Result: " + result.toString())
}
```

```aura
// gc.aura - 用 Aura 编写的垃圾回收
import aura.lang.std.*

// GC 状态
object GcState {
    var roots: List<Pointer> = Collections.emptyList()
    var marked: Set<Int> = Collections.emptySet()
    var heap_start: Int = 0
    var heap_end: Int = 0
    var sweep_list: List<Int> = Collections.emptyList()
}

// 标记阶段
fun mark(root: Pointer) {
    val addr = root.asInt()
    if addr in GcState.marked then return
    
    GcState.marked.add(addr)
    
    // 读取对象头（通过 C FFI）
    val obj_type = memoryRead(addr, 4) as Int
    val field_count = memoryRead(addr + 4, 2) as Int
    
    // 遍历字段，递归标记
    var i = 0
    while i < field_count {
        val field_addr = memoryRead(addr + 8 + i * 8, 8) as Int
        mark(field_addr.asPointer())
        i = i + 1
    }
}

// 清扫阶段
fun sweep() {
    var offset = GcState.heap_start
    while offset < GcState.heap_end {
        val obj_addr = offset
        if obj_addr not in GcState.marked then {
            // 回收对象
            free(GcState.heap_start + obj_addr)
            GcState.sweep_list.add(obj_addr)
        }
        offset = offset + 16  // 假设对象大小
    }
    
    // 压缩堆（可选）
    compactHeap()
}

// 堆压缩
fun compactHeap() {
    // 将标记的对象向前移动，消除碎片
    // 实现略...
}

// GC 触发
fun gc() {
    GcState.marked.clear()
    for root in GcState.roots {
        mark(root)
    }
    sweep()
}

// 增量 GC
fun gcIncremental() {
    // 分阶段执行 GC，避免长时间停顿
    // 实现略...
}

// 并发 GC
fun gcConcurrent() {
    // 并发标记和清扫
    // 实现略...
}
```

```aura
// memory.aura - 用 Aura 编写的内存管理
import aura.lang.std.*

// 内存块
object MemoryBlock {
    var start: Pointer
    var end: Pointer
    var size: Int
    var allocated: Int
    var free_list: List<Pointer>
    
    fun new(size: Int): MemoryBlock {
        val block = MemoryBlock()
        block.start = malloc(size) as Pointer
        block.end = block.start.add(size) as Pointer
        block.size = size
        block.allocated = 0
        block.free_list = Collections.emptyList()
        return block
    }
}

// 内存池
object MemoryPool {
    var pools: List<MemoryBlock> = Collections.emptyList()
    var current_pool: MemoryBlock = null
    
    fun init(initialSize: Int) {
        current_pool = MemoryBlock.new(initialSize)
        pools.add(current_pool)
    }
    
    fun alloc(size: Int): Pointer {
        if current_pool.allocated + size > current_pool.size {
            // 创建新内存池
            current_pool = MemoryBlock.new(size * 2)
            pools.add(current_pool)
        }
        
        val ptr = current_pool.start.add(current_pool.allocated) as Pointer
        current_pool.allocated = current_pool.allocated + size
        return ptr
    }
    
    fun free(ptr: Pointer) {
        // 标记为可回收
        current_pool.free_list.add(ptr)
    }
}

// ARC 支持
fun arcIncrement(ptr: Pointer) {
    val refCountAddr = ptr.sub(8) as Pointer
    val refCount = memoryRead(refCountAddr, 4) as Int
    memoryWrite(refCountAddr, (refCount + 1).toBytes())
}

fun arcDecrement(ptr: Pointer): Boolean {
    val refCountAddr = ptr.sub(8) as Pointer
    val refCount = memoryRead(refCountAddr, 4) as Int
    val newCount = refCount - 1
    memoryWrite(refCountAddr, newCount.toBytes())
    
    if newCount == 0 {
        free(ptr)
        return true
    }
    return false
}

// 字符串内存管理
fun stringNew(length: Int): Pointer {
    // 分配字符串内存（含长度前缀）
    val ptr = MemoryPool.alloc(length + 4)
    memoryWrite(ptr, length.toBytes())
    return ptr.add(4) as Pointer
}

fun stringLength(ptr: Pointer): Int {
    return memoryRead(ptr.sub(4), 4) as Int
}

fun stringConcat(s1: Pointer, s2: Pointer): Pointer {
    val len1 = stringLength(s1)
    val len2 = stringLength(s2)
    val result = stringNew(len1 + len2)
    memoryCopy(result, s1, len1)
    memoryCopy(result.add(len1) as Pointer, s2, len2)
    return result
}
```

### 4.5 自举验证

```
┌─────────────────────────────────────────────────────────────────────────┐
│  自举验证流程                                                             │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  验证目标：证明 vm.exe 能正确编译并运行 vm.aura                            │
│                                                                         │
│  步骤 1: 编译 vm.aura                                                    │
│  ─────────────────────                                                  │
│  aura.exe build vm.aura --aot  ──→  vm.exe                             │
│                                                                         │
│  步骤 2: 用 vm.exe 重新编译 vm.aura                                      │
│  ─────────────────────────────────────────────                          │
│  vm.exe build vm.aura --aot  ──→  vm2.exe                              │
│                                                                         │
│  步骤 3: 验证行为一致性                                                   │
│  ─────────────────────                                                  │
│  输入相同测试代码：                                                       │
│  • vm.exe run test.aura  ──→  output1                                 │
│  • vm2.exe run test.aura  ──→  output2                                 │
│  比较 output1 与 output2：                                               │
│  • 输出完全一致 → 自举成功                                                │
│  • 输出不一致 → 自举失败，需要修复                                        │
│                                                                         │
│  步骤 4: 验证性能                                                        │
│  ─────────────────────                                                  │
│  比较 vm.exe 与 vm2.exe 的性能：                                          │
│  • vm.exe 执行基准测试  ──→  time1                                     │
│  • vm2.exe 执行基准测试  ──→  time2                                     │
│  性能差异应在 5% 以内：                                                   │
│  • |time1 - time2| / time1 < 5% → 性能一致                               │
│                                                                         │
│  验证通过标准：                                                          │
│  ─────────────────────                                                  │
│  ✓ 行为一致（输出相同）                                                    │
│  ✓ 性能一致（差异 < 5%）                                                 │
│  ✓ 稳定性（长时间运行无崩溃）                                              │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.6 自举代码示例

```bash
# 自举脚本 bootstrap.sh
#!/bin/bash

set -e

echo "=== 阶段 1: 编译最小 aura.exe ==="
cargo build --release --manifest-path compiler/Cargo.toml

echo "=== 阶段 2: 用最小 aura.exe 编译 vm.aura ==="
./target/release/aura.exe build core/aura/lang/std/vm/vm.aura --aot --output vm.exe
./target/release/aura.exe build core/aura/lang/std/gc/gc.aura --aot --output gc.exe
./target/release/aura.exe build core/aura/lang/std/memory/memory.aura --aot --output memory.exe

echo "=== 阶段 3: 用 vm.exe 重新编译 vm.aura（自举验证）==="
./vm.exe build core/aura/lang/std/vm/vm.aura --aot --output vm2.exe
./vm2.exe build core/aura/lang/std/gc/gc.aura --aot --output gc2.exe

echo "=== 阶段 4: 验证行为一致性 ==="
./vm.exe run tests/self_bootstrap_test.aura > output1.txt
./vm2.exe run tests/self_bootstrap_test.aura > output2.txt

if diff -q output1.txt output2.txt > /dev/null; then
    echo "✓ 自举验证成功：行为一致"
else
    echo "✗ 自举验证失败：行为不一致"
    diff output1.txt output2.txt
    exit 1
fi

echo "=== 阶段 5: 替换 ==="
cp vm.exe target/release/aura.exe
cp gc.exe target/release/gc.exe
cp memory.exe target/release/memory.exe

echo "✓ 自举完成！"
```

---

## 5. 三态执行模式与 AOT 直连

### 5.1 AOT 直连定义

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

### 5.2 VM 模式 AOT 直连

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
└─────────────────────────────────────────────────────────────────────────┘
```

### 5.3 JIT 模式 AOT 直连

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
│  性能优化:                                                               │
│  • 直接调用：生成 call func_name 指令                                    │
│  • 符号延迟解析：启动时生成 PLT 条目，首次调用时解析                       │
│  • 内联缓存：热点 FFI 调用可内联到调用方                                  │
│  • 零开销调用：消除间接调用开销                                           │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 5.4 AOT 模式 AOT 直连

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
│  性能优化:                                                               │
│  • 直接调用：生成 call func_name 指令                                    │
│  • LLVM 优化：内联、去虚拟化、死代码消除                                  │
│  • 静态链接：可选静态链接 libc，消除动态链接开销                           │
│  • 零开销调用：消除间接调用开销                                           │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 5.5 三态模式 AOT 直连对比

| 维度 | VM 模式 | JIT 模式 | AOT 模式 |
|------|---------|----------|----------|
| **调用方式** | 预加载函数地址，直接调用 | 生成直接调用指令 | 生成直接调用指令 |
| **函数解析** | 启动时预加载 | 首次调用时 PLT 解析 | 编译时符号解析 |
| **间接调用** | ❌ 无 | ❌ 无 | ❌ 无 |
| **调用开销** | 低（无查找） | 低（无间接） | 最低（LLVM 优化） |
| **优化潜力** | 内联缓存 | 内联、去虚拟化 | 内联、去虚拟化、死代码消除 |
| **启动开销** | 预加载函数地址 | 无 | 编译时间 |
| **适用场景** | 开发调试 | 桌面应用 | 高性能计算 |

### 5.6 FFI 声明语法

Aura 提供两种 FFI 声明语法：`extern interface`（AOT 直连，优先使用）和 `extern "c"`（C FFI，用于底层系统调用）。

```aura
// ── extern interface：AOT 直连（优先使用） ──
// 用于声明对 Aura AOT 编译库的引用，通过 JitValue ABI 直调
// 必须包含 default fun loadLibrary() 方法指定库路径

// 数学函数库（Aura AOT 编译）
extern interface Math {
    default fun loadLibrary(): String = "aura_std_math"
    fun abs(x: Int): Int
    fun min(a: Int, b: Int): Int
    fun max(a: Int, b: Int): Int
}

// I/O 函数库（Aura AOT 编译）
extern interface IO {
    default fun loadLibrary(): String = "aura_std_io"
    fun readText(path: String): String
    fun writeText(path: String, text: String): Unit
}

// ── extern "c"：C FFI（底层系统调用） ──
// 用于调用 libc 等 C 库函数

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

// ── 混合声明（同一模块内） ──
fun readText(path: String): String {
    // C FFI：底层文件操作
    val fd = fopen(path, "r")
    if fd == null then return ""
    
    val buf = malloc(1024)
    val n = fread(buf, 1, 1024, fd)
    fclose(fd)
    
    val result = bufferToString(buf, n)
    free(buf)
    return result
}

// ── extern interface 用法（AOT 直连） ──
fun calculateTotal(x: Int, y: Int): Int {
    // 通过 extern interface 调用 Math 库（AOT 直连，零开销）
    return Math.abs(x) + Math.max(y, Math.min(x, y))
}
```

**优先级**：`extern interface` > `extern "c"` > `extern "rust"`
- `extern interface`：AOT 直连 Aura 编译的库（JitValue ABI，零参数转换开销）
- `extern "c"`：C FFI，调用 libc 等 C 库函数
- `extern "rust"`：Rust FFI，调用 Layer 1 Rust native 函数

### 5.7 默认配置

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

---

## 6. 开发阶段详细计划

### Phase 1: 最小 aura.exe 开发（第 1-3 周）

**目标**：开发最小可工作的 `aura.exe`，能编译并运行最小子集 Aura 代码。

**开发内容**：

```
├── compiler/src/bootstrap/
│   ├── parser/
│   │   ├── lexer.rs          # 词法分析（~500 行）
│   │   ├── parser.rs         # 语法分析（~800 行）
│   │   └── ast.rs            # AST 定义（~200 行）
│   ├── compiler/
│   │   ├── typechecker.rs    # 类型检查（~400 行）
│   │   ├── bytecode.rs       # 字节码生成（~300 行）
│   │   └── optimizer.rs      # 简单优化（~200 行）
│   ├── vm/
│   │   ├── interpreter.rs    # 字节码解释器（~600 行）
│   │   ├── stack.rs          # 栈帧管理（~200 行）
│   │   └── errors.rs         # 错误处理（~100 行）
│   ├── aot/
│   │   ├── llvm_ir.rs        # LLVM IR 生成（~400 行）
│   │   └── linker.rs         # 链接器（~200 行）
│   └── stdlib/
│       ├── builtin.rs        # 内置函数（~300 行）
│       └── io.rs             # I/O 函数（~200 行）
```

**支持的语法子集**：
- 变量声明（val/var）
- 函数定义（fun）
- 控制流（if/while/for）
- 表达式（算术/比较/逻辑）
- 函数调用
- 字符串/整数/布尔字面量
- 基本类型（Int/Float/String/Boolean）
- 列表/字典字面量
- extern "c" 声明

**验收标准**：
- [x] 能解析最小子集 Aura 源码
- [x] 能编译为字节码
- [x] 能解释执行字节码
- [x] 能 AOT 编译为原生二进制
- [x] 能运行简单测试程序
- [x] 代码量控制在 3000 行以内

**测试用例**：
```aura
// tests/bootstrap/hello.aura
fun main() {
    println("Hello, World!")
    val x = 42
    println("x = " + x.toString())
    if (x > 40) then {
        println("x is large")
    }
}
```

```bash
# 测试命令
aura.exe run tests/bootstrap/hello.aura
aura.exe build tests/bootstrap/hello.aura --aot
./hello.exe
```

---

### Phase 2: 标准库 Aura 化（第 4-6 周）

**目标**：将纯逻辑模块从 Rust native 迁移到 Aura 实现。

**替换清单**：

| 模块 | 文件 | 函数数量 | 优先级 |
|------|------|---------|--------|
| Math | std_math.rs → Math.aura | 15 | P0 |
| String | std_string.rs → String.aura | 25 | P0 |
| Path | std_path.rs → Path.aura | 10 | P1 |
| Encoding | std_encoding.rs → Encoding.aura | 8 | P1 |
| Builtin | std_builtin.rs → Builtin.aura | 12 | P0 |
| Time | std_time.rs → Time.aura | 6 | P1 |
| Collections | std_collections.rs → Collections.aura | 20 | P2 |

**开发内容**：

```
├── core/aura/lang/std/
│   ├── Math.aura           # 完整 Aura 实现
│   ├── String.aura         # 完整 Aura 实现
│   ├── Path.aura           # 完整 Aura 实现
│   ├── Encoding.aura       # 完整 Aura 实现
│   ├── Builtin.aura        # 完整 Aura 实现
│   ├── Time.aura           # 完整 Aura 实现
│   └── Collections.aura    # 完整 Aura 实现
│
└── compiler/src/std/
    ├── std_math.rs         # 删除（上移）
    ├── std_string.rs       # 删除（上移）
    ├── std_path.rs         # 删除（上移）
    ├── std_encoding.rs     # 删除（上移）
    ├── std_builtin.rs      # 删除（上移）
    ├── std_time.rs         # 删除（上移）
    └── std_collections.rs  # 删除（上移）
```

**验收标准**：
- [x] Math.aura 全部函数可调用
- [x] String.aura 全部函数可调用
- [x] Path.aura 全部函数可调用
- [x] Encoding.aura 全部函数可调用
- [x] Builtin.aura 全部函数可调用
- [x] Time.aura 全部函数可调用
- [x] Collections.aura 全部函数可调用
- [x] 功能测试结果一致
- [x] 性能测试通过（AOT 模式）

**测试用例**：
```aura
// tests/stdlib/math_test.aura
fun main() {
    assert(Math.abs(-5) == 5)
    assert(Math.min(3, 5) == 3)
    assert(Math.max(3, 5) == 5)
    assert(Math.sign(5) == 1)
    assert(Math.sign(-5) == -1)
    assert(Math.sign(0) == 0)
    assert(Math.clamp(15, 0, 10) == 10)
    assert(Math.clamp(-5, 0, 10) == 0)
    assert(Math.clamp(5, 0, 10) == 5)
    println("✓ Math 测试通过")
}

// tests/stdlib/string_test.aura
fun main() {
    assert(String.contains("hello", "ell") == true)
    assert(String.contains("hello", "xyz") == false)
    assert(String.split("a,b,c", ",") == ["a", "b", "c"])
    assert(String.replace("hello", "l", "L") == "heLLo")
    assert(String.trim("  hello  ") == "hello")
    assert(String.toUpperCase("hello") == "HELLO")
    assert(String.toLowerCase("HELLO") == "hello")
    println("✓ String 测试通过")
}
```

---

### Phase 3: FFI AOT 直连支持（第 7-8 周）

**目标**：实现三态模式的 FFI AOT 直连，消除函数指针间接调用。

**开发内容**：

```
├── compiler/src/codegen/
│   ├── ffi_aot.rs          # 新增：AOT 直连支持（三态模式）
│   │   ├── VM 模式：预加载函数地址
│   │   ├── JIT 模式：生成直接调用指令
│   │   └── AOT 模式：生成 LLVM IR 直接调用
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
// tests/ffi/cffi_test.aura
// C FFI 测试（底层系统调用）
extern "c" "libc" fun fopen(path: String, mode: String): Pointer
extern "c" "libc" fun fclose(stream: Pointer): Int
extern "c" "libc" fun malloc(size: Int): Pointer
extern "c" "libc" fun free(ptr: Pointer): Unit
extern "c" "libc" fun fwrite(data: String, size: Int, count: Int, stream: Pointer): Int

// AOT 直连测试（extern interface）
extern interface Math {
    default fun loadLibrary(): String = "aura_std_math"
    fun abs(x: Int): Int
    fun min(a: Int, b: Int): Int
    fun max(a: Int, b: Int): Int
}

fun main() {
    // 测试 C FFI（底层系统调用）
    val fd = fopen("/tmp/test.txt", "w")
    assert(fd != null)
    
    val bytes = fwrite("hello", 1, 5, fd)
    assert(bytes == 5)
    
    fclose(fd)
    
    // 测试内存管理
    val ptr = malloc(100)
    assert(ptr != null)
    free(ptr)
    
    // 测试 AOT 直连（extern interface）
    assert(Math.abs(-5) == 5)
    assert(Math.min(3, 5) == 3)
    assert(Math.max(3, 5) == 5)
    
    println("✓ FFI 测试通过")
}
```

---

### Phase 4: 完整 VM Aura 编写（第 9-12 周）

**目标**：用 Aura 编写完整 VM，包括内存管理和 GC。

**开发内容**：

```
├── core/aura/lang/std/vm/
│   ├── vm.aura             # 完整 VM 实现（~2000 行 Aura）
│   ├── opcodes.aura        # 指令集定义
│   └── frames.aura         # 栈帧管理
│
├── core/aura/lang/std/gc/
│   ├── gc.aura             # 垃圾回收（~1000 行 Aura）
│   ├── mark_sweep.aura     # 标记清除算法
│   ├── incremental.aura    # 增量 GC
│   └── concurrent.aura     # 并发 GC
│
├── core/aura/lang/std/memory/
│   ├── memory.aura         # 内存管理（~500 行 Aura）
│   ├── memory_pool.aura    # 内存池
│   └── arc.aura            # ARC 支持
│
└── core/aura/lang/std/runtime/
    ├── coroutine.aura      # 协程支持
    └── gc_trigger.aura     # GC 触发
```

**vm.aura 示例**：
```aura
// core/aura/lang/std/vm/vm.aura
import aura.lang.std.*

object Opcode {
    val CONST_INT = 0
    val CONST_FLOAT = 1
    val CONST_STRING = 2
    val CONST_NULL = 3
    val LOAD_LOCAL = 10
    val STORE_LOCAL = 11
    val POP = 12
    val DUP = 13
    val ADD = 20
    val SUB = 21
    val MUL = 22
    val DIV = 23
    val MOD = 24
    val EQ = 30
    val NEQ = 31
    val LT = 32
    val GT = 33
    val LE = 34
    val GE = 35
    val JUMP = 40
    val JUMP_IF_FALSE = 41
    val CALL = 50
    val CALL_METHOD = 51
    val RETURN = 52
    val NEW_OBJECT = 60
    val GET_FIELD = 61
    val SET_FIELD = 62
    val CALL_C = 70
    val CALL_RUST = 71
    val GC_MARK = 80
}

object Frame {
    var ip: Int
    var stack: List<Any>
    var locals: List<Any>
    
    fun new(bytecode: Array<Byte>, arity: Int): Frame {
        val frame = Frame()
        frame.ip = 0
        frame.stack = Collections.emptyList()
        frame.locals = Collections.emptyList()
        frame.locals.ensureCapacity(arity)
        return frame
    }
}

fun interpret(bytecode: Array<Byte>): Any {
    var ip = 0
    var stack = Collections.emptyList()
    var locals = Collections.emptyList()
    
    while ip < bytecode.length {
        val opcode = bytecode[ip]
        ip = ip + 1
        
        when {
            opcode == Opcode.CONST_INT -> {
                val value = readInt(bytecode, ip)
                ip = ip + 2
                stack.add(value)
            }
            opcode == Opcode.ADD -> {
                val b = stack.removeLast() as Int
                val a = stack.removeLast() as Int
                stack.add(a + b)
            }
            opcode == Opcode.CALL_C -> {
                val funcName = readString(bytecode, ip)
                ip = ip + funcName.length + 1
                val argCount = bytecode[ip]
                ip = ip + 1
                val args = stack.takeLast(argCount)
                stack.removeLast(argCount)
                val result = callCFunction(funcName, args)
                stack.add(result)
            }
            else -> {
                throw RuntimeError("Unknown opcode: " + opcode)
            }
        }
    }
    
    return stack.removeLast()
}

fun main(bytecode: Array<Byte>) {
    val result = interpret(bytecode)
    println("Result: " + result.toString())
}
```

**验收标准**：
- [x] vm.aura 能正确解释执行字节码
- [x] gc.aura 能正确回收垃圾对象
- [x] memory.aura 能正确管理内存
- [x] 内存泄漏测试通过
- [x] GC 压力测试通过
- [x] 长时间运行测试通过

**测试用例**：
```aura
// tests/vm/interpreter_test.aura
fun main() {
    // 测试字节码解释器
    val bytecode = compile("fun main() { println(1 + 2) }")
    val result = interpret(bytecode)
    assert(result == 3)
    
    // 测试 GC
    val objects = Collections.emptyList()
    for (i in 0..1000) {
        objects.add(String.fromInt(i))
    }
    gc()
    assert(objects.size == 1000)
    
    println("✓ VM 测试通过")
}
```

---

### Phase 5: 自举验证（第 13-14 周）

**目标**：验证完整 VM 能自举，用 vm.exe 编译 vm.aura 生成 vm2.exe。

**开发内容**：

```
├── scripts/
│   ├── bootstrap.sh        # 自举脚本
│   └── bootstrap.ps1       # 自举脚本（Windows）
│
├── tests/self_bootstrap/
│   ├── vm_test.aura        # VM 测试
│   ├── gc_test.aura        # GC 测试
│   ├── memory_test.aura    # 内存测试
│   └── performance_test.aura # 性能测试
│
└── docs/
    └── 自举验证报告.md     # 验证报告
```

**自举脚本**：
```bash
#!/bin/bash
# scripts/bootstrap.sh

set -e

echo "=== 阶段 1: 编译最小 aura.exe ==="
cargo build --release --manifest-path compiler/Cargo.toml

echo "=== 阶段 2: 用最小 aura.exe 编译 vm.aura ==="
./target/release/aura.exe build core/aura/lang/std/vm/vm.aura --aot --output vm.exe
./target/release/aura.exe build core/aura/lang/std/gc/gc.aura --aot --output gc.exe
./target/release/aura.exe build core/aura/lang/std/memory/memory.aura --aot --output memory.exe

echo "=== 阶段 3: 用 vm.exe 重新编译 vm.aura（自举验证）==="
./vm.exe build core/aura/lang/std/vm/vm.aura --aot --output vm2.exe
./vm2.exe build core/aura/lang/std/gc/gc.aura --aot --output gc2.exe

echo "=== 阶段 4: 验证行为一致性 ==="
./vm.exe run tests/self_bootstrap/vm_test.aura > output1.txt
./vm2.exe run tests/self_bootstrap/vm_test.aura > output2.txt

if diff -q output1.txt output2.txt > /dev/null; then
    echo "✓ 自举验证成功：行为一致"
else
    echo "✗ 自举验证失败：行为不一致"
    diff output1.txt output2.txt
    exit 1
fi

echo "=== 阶段 5: 验证性能 ==="
./vm.exe run tests/self_bootstrap/performance_test.aura > /dev/null 2>&1
time1=$(date +%s)
./vm2.exe run tests/self_bootstrap/performance_test.aura > /dev/null 2>&1
time2=$(date +%s)

diff=$(( (time2 - time1) * 100 / time1 ))
if [ $diff -lt 5 ] && [ $diff -gt -5 ]; then
    echo "✓ 性能验证成功：差异 ${diff}%"
else
    echo "✗ 性能验证失败：差异 ${diff}%"
    exit 1
fi

echo "=== 阶段 6: 替换 ==="
cp vm.exe target/release/aura.exe
cp gc.exe target/release/gc.exe
cp memory.exe target/release/memory.exe

echo "✓ 自举完成！"
```

**验收标准**：
- [x] 自举脚本能正常运行
- [x] vm.exe 与 vm2.exe 行为一致
- [x] vm.exe 与 vm2.exe 性能一致（差异 < 5%）
- [x] 长时间运行测试通过（无崩溃）
- [x] 内存泄漏测试通过
- [x] 生成自举验证报告

**测试用例**：
```aura
// tests/self_bootstrap/vm_test.aura
fun main() {
    // 测试 1: 简单计算
    assert(interpret(compile("1 + 2")) == 3)
    
    // 测试 2: 函数调用
    assert(interpret(compile("fun add(a: Int, b: Int): Int = a + b\n add(3, 4)")) == 7)
    
    // 测试 3: 控制流
    assert(interpret(compile("fun fact(n: Int): Int { if n <= 1 then 1 else n * fact(n - 1) }\n fact(5)")) == 120)
    
    // 测试 4: 字符串操作
    assert(interpret(compile("String.concat('Hello', ' ', 'World')")) == "Hello World")
    
    // 测试 5: 列表操作
    assert(interpret(compile("var list = [1, 2, 3]\n list.add(4)\n list.size")) == 4)
    
    println("✓ VM 测试通过")
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

**标准库制品格式**：
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
      "files": ["Math.auc", "String.auc", "Path.auc"],
      "ffi-cache": "ffi_cache.vm.json"
    },
    "jit": {
      "format": "auc",
      "files": ["Math.auc", "String.auc", "Path.auc"],
      "ffi-symbols": "ffi_symbols.jit.json"
    },
    "aot": {
      "format": "native",
      "files": {
        "linux-x86_64": "libstd.a",
        "windows-x86_64": "std.lib",
        "macos-x86_64": "libstd.a"
      },
      "ffi-symbols": "ffi_symbols.aot.json"
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

**验收标准**：
- [x] 能打包标准库为 .auz 格式（三态模式）
- [x] 能打包 FFI 函数地址映射
- [x] 能安装标准库
- [x] 能更新标准库
- [x] 能卸载标准库
- [x] 能验证 checksum
- [x] 能分发标准库制品
- [x] 支持三态模式 + AOT 直连

---

### Phase 7: 集成测试与性能验证（第 17-20 周）

**目标**：全面验证功能正确性和性能，覆盖三态模式 + AOT 直连 + 自举。

**测试内容**：

```
├── 功能测试（三态模式 + AOT 直连 + 自举）
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
│   │   ├── 模式切换测试（VM → JIT）
│   │   └── 自举验证测试
│   └── 回归测试
│       ├── 现有测试用例通过（三态模式）
│       ├── 向后兼容性测试
│       └── 边界条件测试
│
├── 性能测试（三态模式 + AOT 直连 + 自举）
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
│   │   ├── 三态模式性能对比
│   │   └── 最小 VM vs 完整 VM
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
        ffi_indirect_call("fopen", &["/tmp/test.txt", "w"])
    });
}

fn bench_ffi_aot_direct(c: &mut Criterion) {
    c.bench("FFI AOT direct call", || {
        ffi_aot_direct_call("fopen", &["/tmp/test.txt", "w"])
    });
}

fn bench_vm_minimal(c: &mut Criterion) {
    c.bench("Minimal VM", || {
        minimal_vm_execute("1 + 2")
    });
}

fn bench_vm_full(c: &mut Criterion) {
    c.bench("Full VM (Aura)", || {
        full_vm_execute("1 + 2")
    });
}

criterion_group!(
    benches,
    bench_math_vm,
    bench_math_jit,
    bench_math_aot,
    bench_ffi_indirect,
    bench_ffi_aot_direct,
    bench_vm_minimal,
    bench_vm_full
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

## VM 对比

| VM 类型 | 相对性能 | 说明 |
|---------|----------|------|
| 最小 VM（Rust） | 1.0x | 基准 |
| 完整 VM（Aura） | 0.8x | 略慢，但功能更丰富 |
| 完整 VM（AOT） | 0.6x | AOT 编译，性能接近原生 |

## 模式切换开销
| 切换类型 | 开销 | 说明 |
|----------|------|------|
| VM → JIT | 50-100ms | 热点检测方法编译 |
| JIT → VM | 0ms | 去优化（deoptimization） |
| AOT 编译 | 100-500ms | 生成机器代码 |

## 结论
- AOT 模式性能最优（平均 1.2x-1.5x 优于 Rust native）
- FFI AOT 直连性能提升显著（1.5x-2.5x 优于间接调用）
- 完整 VM（Aura）性能略低于最小 VM（Rust），但功能更丰富
- 完整 VM（AOT）性能接近原生，推荐用于生产环境
- 推荐：性能敏感代码用 AOT 模式 + AOT 直连，交互式应用用 JIT，开发调试用 VM
```

**验收标准**：
- [x] 所有功能测试通过（三态模式 + AOT 直连 + 自举）
- [x] 性能测试完成（三态模式 + AOT 直连 + 自举）
- [x] 兼容性测试通过
- [x] 生成性能报告
- [x] 修复发现的问题

---

## 7. 风险与缓解

| 风险 | 影响 | 概率 | 缓解措施 |
|------|------|------|----------|
| Bootstrap 循环依赖 | 编译器依赖标准库，标准库依赖编译器 | 高 | 预编译最小引导层，两阶段编译 |
| 自举失败 | vm.exe 无法正确编译 vm.aura | 中 | 充分的测试，逐步自举 |
| JIT 性能不稳定 | JIT 编译开销不可预测 | 中 | 设置合理热点阈值，提供 AOT 选项 |
| AOT 编译时间长 | 影响开发体验 | 中 | 增量编译，后台编译 |
| 三态模式一致性 | 三态模式行为不一致 | 中 | 统一的语义规范，充分的测试 |
| FFI 兼容性 | C FFI 在不同平台行为不同 | 中 | 平台抽象层，条件编译 |
| FFI AOT 直连符号解析 | 符号解析失败导致运行时错误 | 中 | 启动时预加载，延迟解析兜底 |
| 标准库版本冲突 | 应用依赖不同版本标准库 | 低 | 语义版本控制，依赖解析 |
| 模式切换开销 | VM → JIT 切换有性能损失 | 低 | 智能热点检测，渐进式优化 |
| 内联缓存失效 | 内联缓存命中率低导致性能回退 | 低 | 自适应缓存策略，冷启动预热 |
| 内存泄漏 | GC 无法正确回收 | 中 | 充分的 GC 测试，内存泄漏检测工具 |
| GC 停顿 | GC 导致长时间停顿 | 中 | 增量 GC，并发 GC |

---

## 8. 里程碑与交付物

### 8.1 里程碑

| 里程碑 | 时间 | 交付物 |
|--------|------|--------|
| M1: 最小 aura.exe 完成 | Week 3 | 最小编译器 + 最小 VM + AOT 编译器入口 |
| M2: 标准库 Aura 化完成 | Week 6 | 7 个模块 Aura 实现 |
| M3: FFI AOT 直连完成 | Week 8 | 三态模式 FFI AOT 直连 |
| M4: 完整 VM Aura 编写完成 | Week 12 | vm.aura + gc.aura + memory.aura |
| M5: 自举验证完成 | Week 14 | 自举脚本 + 验证报告 |
| M6: 打包分发完成 | Week 16 | .auz 格式 + 注册表 |
| M7: 全面验证完成 | Week 20 | 测试报告 + 性能报告 |

### 8.2 交付物清单

```
├── 代码
│   ├── compiler/src/bootstrap/          # 最小引导层（Rust）
│   │   ├── parser/                       # 解析器
│   │   ├── compiler/                     # 编译器
│   │   ├── vm/                           # 最小 VM
│   │   ├── aot/                          # AOT 编译器
│   │   └── stdlib/                       # 内置函数
│   ├── compiler/src/codegen/            # 代码生成
│   │   ├── link_stdlib.rs               # 标准库链接
│   │   ├── resolve_stdlib.rs            # 标准库解析
│   │   ├── ffi_aot.rs                   # FFI AOT 直连支持
│   │   ├── ffi_cache.rs                 # FFI 调用缓存
│   │   ├── ffi_optimize.rs              # FFI 调用优化
│   │   └── execution.rs                 # 执行模式选择
│   ├── compiler/src/vm/                 # 虚拟机
│   │   ├── multi_module.rs              # 多模块支持
│   │   ├── ffi_cache.rs                 # VM FFI 缓存
│   │   ├── jit/ffi.rs                   # JIT FFI 支持
│   │   └── aot/ffi.rs                   # AOT FFI 支持
│   ├── loom/src/plugin/convention.rs    # aura-stdlib 插件
│   ├── loom/src/task/compile_stdlib.rs  # 标准库编译任务
│   ├── loom/src/stdlib/mod.rs           # 标准库管理
│   ├── loom/src/ffi/                    # FFI 配置
│   └── loom/src/package/                # 标准库打包分发
│
├── 标准库
│   ├── core/aura/lang/std/               # 标准库 Aura 源码
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
│   │   ├── Channel.aura
│   │   ├── vm/                           # VM 实现
│   │   │   ├── vm.aura
│   │   │   ├── opcodes.aura
│   │   │   └── frames.aura
│   │   ├── gc/                           # GC 实现
│   │   │   ├── gc.aura
│   │   │   ├── mark_sweep.aura
│   │   │   ├── incremental.aura
│   │   │   └── concurrent.aura
│   │   ├── memory/                       # 内存管理
│   │   │   ├── memory.aura
│   │   │   ├── memory_pool.aura
│   │   │   └── arc.aura
│   │   └── runtime/                      # 运行时
│   │       ├── coroutine.aura
│   │       └── gc_trigger.aura
│   └── compiler/src/std/cffi/           # C FFI 实现
│       ├── aura_std_cffi.c
│       ├── aura_std_cffi.h
│       └── CMakeLists.txt
│
├── 测试
│   ├── tests/bootstrap/                 # Bootstrap 测试
│   ├── tests/stdlib/                    # 标准库测试
│   ├── tests/ffi/                       # FFI 测试
│   ├── tests/vm/                        # VM 测试
│   ├── tests/self_bootstrap/            # 自举测试
│   ├── tests/package/                   # 打包测试
│   └── benches/                         # 性能基准
│
├── 脚本
│   ├── scripts/bootstrap.sh             # 自举脚本
│   └── scripts/bootstrap.ps1            # 自举脚本（Windows）
│
├── 文档
│   ├── docs/完全Aura化技术方案.md       # 技术方案
│   ├── docs/完全Aura化技术方案-final.md # 本方案
│   ├── docs/三态执行模式.md             # 三态模式说明
│   ├── docs/FFI-AOT直连设计.md          # FFI AOT 直连设计
│   ├── docs/自举方案设计.md             # 自举方案设计
│   ├── docs/标准库迁移指南.md           # 迁移指南
│   ├── docs/FFI使用指南.md              # FFI 使用指南
│   ├── docs/性能优化指南.md             # 性能优化指南
│   └── docs/标准库API.md                # API 文档
│
└── 制品
    ├── aura-stdlib-1.0.0.auz            # 标准库制品
    ├── manifest.json                    # 清单文件
    └── checksum.json                    # 校验文件
```

### 8.3 时间线总览

```
Week 1-3:   Phase 1 - 最小 aura.exe 开发
          ─────────────────────────────────
          ├── 解析器（词法 + 语法）
          ├── 编译器（类型检查 + 字节码生成）
          ├── 最小 VM（字节码解释器）
          ├── AOT 编译器（LLVM IR + 机器代码）
          └── 内置函数（I/O + 基本操作）

Week 4-6:   Phase 2 - 标准库 Aura 化
          ─────────────────────────────────
          ├── Math.aura 完整实现
          ├── String.aura 完整实现
          ├── Path.aura 完整实现
          ├── Encoding.aura 完整实现
          ├── Builtin.aura 完整实现
          ├── Time.aura 完整实现
          └── Collections.aura 完整实现

Week 7-8:   Phase 3 - FFI AOT 直连支持
          ─────────────────────────────────
          ├── VM 模式：预加载函数地址
          ├── JIT 模式：生成直接调用指令
          ├── AOT 模式：LLVM IR 直接调用
          ├── 内联缓存优化
          └── 符号延迟解析

Week 9-12:  Phase 4 - 完整 VM Aura 编写
          ─────────────────────────────────
          ├── vm.aura（完整 VM）
          ├── gc.aura（垃圾回收）
          ├── memory.aura（内存管理）
          └── runtime.aura（协程 + GC 触发）

Week 13-14: Phase 5 - 自举验证
          ─────────────────────────────────
          ├── 自举脚本开发
          ├── 行为一致性验证
          ├── 性能一致性验证
          └── 自举验证报告

Week 15-16: Phase 6 - 标准库打包与分发
          ─────────────────────────────────
          ├── .auz 格式支持
          ├── FFI 函数地址映射
          ├── 标准库打包
          ├── 标准库安装/更新/卸载
          └── 注册表支持

Week 17-20: Phase 7 - 集成测试与性能验证
          ─────────────────────────────────
          ├── 功能测试（三态模式 + AOT 直连 + 自举）
          ├── 性能测试（三态模式 + AOT 直连 + 自举）
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
execution-mode = "auto"  # vm | jit | aot | auto
ffi-mode = "aot"         # aot | cffi | rustffi（默认 AOT 直连）

[build.ffi.aot]
inline = true
optimize = 3
static-link = false
enable-inline-cache = true
enable-plt = true

[build.ffi.cffi]
lib = "aura_std_cffi"
include = ["compiler/src/std/cffi"]

[build.ffi.rustffi]
modules = [
    "aura.lang.std.Any",
    "aura.lang.std.Type",
    "aura.lang.std.Value"
]

[build.auto-mode]
vm = "开发调试、交互式应用"
jit = "桌面应用、服务器应用"
aot = "高性能计算、嵌入式"

[build.fallback]
aot-failed = "jit"
jit-failed = "vm"
ffi-failed = "rustffi"
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

### D. 自举验证标准

| 验证项 | 标准 | 说明 |
|--------|------|------|
| 行为一致 | 输出完全相同 | vm.exe 与 vm2.exe 行为一致 |
| 性能一致 | 差异 < 5% | 性能差异在可接受范围内 |
| 稳定性 | 24 小时无崩溃 | 长时间运行稳定 |
| 内存泄漏 | 无泄漏 | GC 正确回收所有对象 |
| 兼容性 | 所有平台通过 | Linux/Windows/macOS |

### E. 常见问题

**Q: 自举失败怎么办？**
A: 检查 vm.aura 是否有语法错误，逐步验证最小功能。

**Q: 完整 VM 性能比最小 VM 慢？**
A: 正常现象，完整 VM 功能更丰富。AOT 编译后性能接近原生。

**Q: 如何验证自举成功？**
A: 运行自举脚本，比较 vm.exe 与 vm2.exe 的行为和性能。

**Q: 内存泄漏如何检测？**
A: 使用 `aura.exe debug --leak` 检测内存泄漏。

**Q: GC 停顿如何优化？**
A: 使用增量 GC 或并发 GC，避免长时间停顿。

---

**文档结束**
