# Aura CFFI 实现方案：零 C 源码运行库

> 目标：消除项目源码树中的 C/C++ 代码，运行库完全用 Aura 实现，编译期直接翻译为 LLVM IR。

---

## 目标与边界

### 目标
- `git ls-files '*.c' '*.h' '*.cpp' '*.cc' '*.hpp' '*.hh' | grep -v node_modules` 返回空
- 运行库 100% 用 Aura 源码实现
- 编译期直接翻译为 LLVM IR（无 C 中间层）
- 支持跨平台（x86_64 Linux / Windows / aarch64）

### 边界（不违反约束）
- libc 仍会被链接，但只以 LLVM `declare` 形式存在，**没有 C 源文件**
- Rust 编译器代码不动
- 现有 Rust AOT 后端保留，作为过渡期编译运行库的工具

---

## 现状盘点

项目里"真正属于自己写的" C/C++ 源码：

| 路径 | 大小 | 作用 |
|---|---|---|
| `compiler/src/std/cffi/aura_std_cffi.c` | 77 KB | AOT 运行库 shim |
| `compiler/src/std/cffi/aura_std_cffi.h` | 21 KB | 头文件 |
| `examples/ext_ffi_demo/demo_cffi/utils.h` | 850 B | FFI 示例 |
| `build/test_export.c` | 155 B | 测试夹具 |

---

## 核心架构

```
                    ┌─────────────────────────────────┐
                    │  aura/core/aura/lang/runtime/   │  ← 运行库（Aura 源码）
                    │  ────────────────────────────   │
                    │  extern object Syscalls         │
                    │  extern object Memory           │
                    │  extern object Cpu              │
                    │  object Allocator / StrOps / ...│
                    └────────────────┬────────────────┘
                                     │
                    ┌────────────────▼────────────────┐
                    │  编译期翻译（Rust AOT 后端）       │
                    │  HIR → LLVM IR 文本               │
                    │  @native(N) → inline asm syscall  │
                    │  @native → load/store             │
                    │  @export → define external        │
                    └────────────────┬────────────────┘
                                     │
                    ┌────────────────▼────────────────┐
                    │  llc -emit-obj                    │
                    │  → aura_runtime.obj               │
                    └────────────────┬────────────────┘
                                     │
                ┌────────────────────▼────────────────────┐
                │  用户 Aura 程序 → aura build --aot       │
                │  → user.obj                             │
                └────────────────────┬────────────────────┘
                                     │
                ┌────────────────────▼────────────────────┐
                │  clang user.obj aura_runtime.obj -o exe  │
                └──────────────────────────────────────────┘
```

---

## 新增语法（最小集合）

| 语法 | 用途 | 示例 |
|---|---|---|
| `extern object Name { ... }` | 封装原生/外部方法 | `extern object Syscalls { ... }` |
| `@native(N)` | 系统调用（N 为 syscall 号） | `@native(1) fun write(...)` |
| `@native("libc:name")` | 外部库符号 | `@native("libc:malloc") fun malloc(...)` |
| `@native` | 编译器内置（无参数） | `@native fun read(addr: Long): Byte` |
| `@native(asm = "...")` | 内联汇编 | `@native(asm = "rdtsc") fun rdtsc(): Long` |
| `@export` | 导出符号（Aura 实现体） | `@export fun malloc(n: Long): Long { ... }` |

---

## 类型映射（C/LLVM → Aura）

| C/LLVM | Aura 类型 | 说明 |
|---|---|---|
| `i32` / `int` | `Int` | 32 位有符号 |
| `i64` / `long` | `Long` | 64 位有符号 |
| `u8` / `char` | `Byte` | 8 位字节 |
| `u16` | `Short` | 16 位 |
| `f32` / `float` | `Float` | 32 位浮点 |
| `f64` / `double` | `Double` | 64 位浮点 |
| `bool` | `Boolean` | 布尔 |
| `char*` / `string` | `String` | Aura 字符串 |
| `void` | `Unit` | 空 |
| `T*` / `ptr` | `Long`（地址）或 `CString` | 用 `Long` 表示内存地址 |
| `void*` / `any` | `Any` | 装箱值 |
| `T[]` / `vec<T>` | `List<T>` | 动态列表 |
| `null` | `null`（Aura 的 `T?` 可空类型） | 可空引用 |

---

## 运行库模块结构

```
aura/core/aura/lang/runtime/
├── Syscalls.aura              # 系统调用声明（extern object）
├── Memory.aura                # 内存操作（extern object，编译器内置）
├── Cpu.aura                   # CPU 级操作（extern object，内联汇编）
├── Runtime.aura               # 入口聚合
│
├── memory/
│   └── Allocator.aura         # 内存分配器（object + @export）
├── string/
│   └── StrOps.aura            # 字符串操作
├── console/
│   └── Console.aura           # 控制台输出
├── math/
│   └── MathCore.aura          # 数学函数
├── random/
│   └── XorShift.aura          # 随机数
├── time/
│   └── Clock.aura             # 时间
├── file/
│   └── FileOps.aura           # 文件 I/O
├── process/
│   └── ProcessOps.aura        # 进程管理
├── boxed/
│   └── PlanA.aura             # Plan A 装箱
│
└── arch/                      # 跨平台 syscall 表
    ├── x86_64_linux/Syscalls.aura
    ├── x86_64_windows/Syscalls.aura
    ├── aarch64_linux/Syscalls.aura
    └── aarch64_darwin/Syscalls.aura
```

---

## 编译期翻译规则

| Aura 语法 | LLVM IR |
|---|---|
| `@native(N) fun write(...)` | `define i64 @write(...) { call asm "mov $N, %rax; ...; syscall" }` |
| `@native fun read(addr: Long): Byte` | `load i8, ptr %addr` |
| `@native fun write64(addr, v)` | `store i64 %v, ptr %addr` |
| `@native fun copy(dst, src, n)` | `call void @llvm.memcpy(...)` |
| `@native(asm = "rdtsc") fun rdtsc(): Long` | `call i64 asm "rdtsc" { ... }` |
| `@export fun malloc(n: Long): Long { ... }` | `define i64 @malloc(i64 %n) { ... }` |
| `Syscalls.write(1, buf, n)` | `call i64 @write(i32 1, i64 %buf, i64 %n)` |
| `Memory.read(addr)` | `load i8, ptr %addr` |
| `s as Long`（CString → Long） | `ptrtoint` |
| `addr as String`（Long → String） | `inttoptr` |

---

## 构建管线

```bash
# 编译运行库 → 目标文件
aura build aura/core/aura/lang/runtime/Runtime.aura --aot --obj -o build/lib/aura_runtime.obj

# 编译用户程序 → 目标文件
aura build my_program.aura --aot --obj -o build/bin/my_program.obj

# 链接
clang build/bin/my_program.obj build/lib/aura_runtime.obj -o build/bin/my_program.exe
```

或一步到位：
```bash
aura build my_program.aura --aot -o build/bin/my_program.exe
# 自动链接 aura_runtime.obj
```

---

## 分阶段路线

### Phase S0：语法与发射器（3-5 天）

- 新增 `@native` 语法
- 新增 `extern object` 语法
- IR 翻译：`@native(N)` → inline asm；`@native` → load/store
- 测试：`tests/phase_s0_native_syntax_tests.aura`

### Phase S1：最小运行库（1 周）

- `Syscalls.aura`（x86_64 Linux）
- `Memory.aura`（内存操作）
- 内存分配器（bump allocator）
- 字符串 / 内存操作
- Console（`Console.println`）
- **目标**：Hello World 无 libc 跑通

### Phase S2：完整运行库（2 周）

- 文件 I/O
- 进程 / 环境
- 时间 / 随机
- 数学
- 集合（已有，对接新运行库）

### Phase S3：跨平台（2 周）

- Windows / macOS / aarch64 syscall 表
- CRT startup
- 平台特性（Windows 的 `NtWriteVirtualMemory` 等）

### Phase S4：删除 C 源码（1 天）

- 删除 `compiler/src/std/cffi/aura_std_cffi.{c,h}`
- 删除 `examples/ext_ffi_demo/demo_cffi/utils.h`
- CI 加 C 源检查门禁

### Phase S5：自举（长期，可选）

- Aura 编译器自身用 Aura AOT 编译
- 完全无 Rust 参与

---

## 最终效果

```bash
$ git ls-files '*.c' '*.h' '*.cpp' '*.cc' | grep -v node_modules
# (空)

$ ls aura/core/aura/lang/runtime/
Syscalls.aura  Memory.aura  Cpu.aura  Runtime.aura
boxed/         console/     file/     math/
memory/        process/     random/   string/
time/          arch/

$ aura build my_program.aura --aot -o my_program.exe
my_program.exe

$ file my_program.exe
my_program.exe: PE32+ executable, 3 sections
# 链接 aura_runtime.obj，无 libc，无 C 源码
```

---

## 相关文件

- [design.md](./design.md) — 详细设计（语法、类型、架构）
- [runtime-modules.md](./runtime-modules.md) — 运行库模块源码
- [implementation-plan.md](./implementation-plan.md) — 分阶段实施计划
