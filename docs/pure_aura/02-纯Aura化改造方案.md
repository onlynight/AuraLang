# 纯 Aura 化改造方案（不依赖 Rust）

> **文档定位**：详细改造路线（Phase 1–5）
> **配套文档**：`01-现状分析.md`（现状评估）
> **目标**：除 `compiler/src/bootstrap/`（最小引导层，明确保留 Rust）外，完全脱离 Rust 编译器
> **日期**：2026-09-13
> **预计工期**：11–19 周（约 2.5–4 个月，单人全职）

---

## 〇、目标架构总览

```
┌─────────────────────────────────────────────────────────────────────┐
│  最终架构                                                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Layer 0-A: 最小引导层（Rust，保留）                                    │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  compiler/src/bootstrap/                                       │   │
│  │  ├── vm_core.rs     最小编译器 + VM + FFI 直连                  │   │
│  │  ├── aot_core.rs    LLVM IR 直发（调用 C 符号）                  │   │
│  │  ├── jit_core.rs    JIT 基线编译 + 去优化                       │   │
│  │  ├── memory.rs      malloc / free / arc / string_*             │   │
│  │  ├── runtime.rs     协程 + 最小 GC                              │   │
│  │  ├── any_core.rs    toString / equals / hashCode               │   │
│  │  ├── type_core.rs   typeOf / isOfType / cast                   │   │
│  │  └── value_check.rs 空值/数值检查                                │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  Layer 0-B: C 运行库（C，保留）                                        │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  compiler/src/std/cffi/aura_std_cffi.c                        │   │
│  │  ├── aura_println / aura_print / aura_puts                     │   │
│  │  ├── aura_io_*  IO + 文件                                       │   │
│  │  ├── aura_math_*  数学函数                                     │   │
│  │  ├── aura_string_*  字符串操作                                  │   │
│  │  ├── aura_collections_*  集合操作                              │   │
│  │  ├── aura_concurrent_*  并发 API（Actor/Channel/Coroutine）    │   │
│  │  └── ...  ~100 个 C ABI 函数                                    │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  Layer 1+: 全部 Aura 实现（新）                                       │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  aura/compiler/aura/lang/compiler/   完整编译器                 │   │
│  │  aura/core/aura/lang/std/            标准库                    │   │
│  │  aura/core/aura/lang/ffi/            Syscalls / Cpu / Memory    │   │
│  │  aura/ 下的 CLI / LSP / 调试器 / loom 构建系统                   │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  外部工具链（非 Rust）：                                               │
│    • LLVM (llc / clang / lld-link)   AOT 机器码生成                   │
│    • C 编译器                          编译 aura_std_cffi.c           │
│    • (可选) Git                         包管理                         │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 删除清单（阶段 D/E 完成后）

| 模块 | 文件数 | 总行数（粗估） | 替代 |
|------|--------|----------------|------|
| `compiler/src/vm/**` | 20 | ~15000 | `aura/compiler/aura/lang/compiler/vm/` |
| `compiler/src/codegen/**` | 12 | ~12000 | `aura/compiler/aura/lang/compiler/codegen/` |
| `compiler/src/std/**`（除 cffi/） | 24 | ~15000 | `aura/core/aura/lang/std/` |
| `compiler/src/`（parser, lexer, ast, sema, linker, signature, signing, lsp, docgen, package, source_map, span, token） | 13 | ~45000 | `aura/compiler/aura/lang/compiler/` |
| `cli/src/**` | 4 | ~25000 | `aura/.../cli/` |
| `loom/src/**` | 50 | ~15000 | `aura/.../loom/` |
| **合计可删除** | **~123** | **~127000** | — |

### 保留清单（bootstrap + C）

| 模块 | 文件数 | 总行数 | 说明 |
|------|--------|--------|------|
| `compiler/src/bootstrap/` | 9 | ~4000 | 最小 Rust 引导层（Layer 0-A） |
| `compiler/src/std/cffi/` | 2 | ~3000 | C 运行库（Layer 0-B） |
| **合计保留** | **11** | **~7000** | — |

---

## 一、阶段 A：加固 Rust AOT 后端（1–2 周）

### A.0 目标

让 `compiler/src/codegen/aot/emit.rs` 能正确编译 `Main.aura`，产出可用的 `aura-compiler-native.exe`，作为「原生载体」跑后续 Aura 侧自举。

> 本阶段仍属 Rust 侧改动，但属 bootstrap 范畴（Layer 0-A），是「脱 Rust」的必经之路。

### A.1 类类型按引用传递

**问题**：AOT 把类类型形参按值传递（`%struct.X` 拷贝），导致被调方对字段的修改丢失。

**复现**（`build/probe7/identity.aura`）：
```aura
class Holder { var v: Int = 0 }
fun bump(h: Holder) { h.v = 1 }
fun main(): Int { val h = Holder(); bump(h); return h.v }
```
- VM 返回 `1`（正确）
- AOT 返回 `0`（错误——`h` 被拷贝）

**改造**：
- 位置：`compiler/src/codegen/aot/emit.rs`（`emitCall`、`emitStructInit`、`emitReturn`、`structFieldAccess`）
- 改动：
  - `TypeMapper::to_llvm_type` 对用户类返回 `%struct.X*`（指针）而非 `%struct.X`
  - 形参声明 `@fn(%struct.X %arg)` → `@fn(%struct.X* %arg)`
  - 构造结果 `= alloca %struct.X` + `ret %struct.X*`（已部分实现）
  - 字段访问 `h.v` → `getelementptr %struct.X, %struct.X* %arg, 0, i32 field`（已有）
  - `Call` 实参为 `%struct.X` → `%struct.X*`
  - 函数返回值 `ret %struct.X %v` → `ret %struct.X* %v`

**风险**：影响所有 AOT 编译的用户程序，需全量回归。

### A.2 String 表示统一

**问题**：AOT 把 `String` 映射为 `{i8*, i64}`，但 String 方法（`contains`/`startsWith`/`split`…）按 `i8*` 调用点实现。在「重度使用字符串方法」的 Aura 编译器上出现静默错误。

**改造**：
- 位置：`compiler/src/codegen/aot/types.rs`、`emit.rs`、`runtime.rs`、`aura_std_cffi.h/c`
- 改动：
  - 全后端统一为 `i8*`（C ABI `char*` 风格），长度由 `strlen` 或显式参数携带
  - `aura_std_cffi.h` 中 `AuraString` 结构体改为 `const char *`
  - `Runtime.declarations` 同步更新（`aura_string_concat` 等）
  - VM 侧 `Value::Str(Rc<str>)` 在 AOT 边界转换为 `CString`（NUL 结尾）

**验证**：`build/probe7/identity.aura` 中所有 String 方法调用 AOT 与 VM 结果一致。

### A.3 `emit_call` 尊重返回类型

**问题**：`emit.rs:1039` 硬编码 `let ret_ty = "i32".to_string()`，所有调用返回值被假设为 i32。native 函数返回指针时 `llc` 报错。

**改造**：
- 位置：`compiler/src/codegen/aot/emit.rs:1039`
- 改动：从 `hir::HirCall` 读取被调用函数的返回类型，经 `TypeMapper` 映射为 LLVM 类型
- 覆盖：
  - 用户函数返回值（已由 HIR 类型系统提供）
  - native 函数返回值（从 `NativeRegistry` 签名表读取）
  - std 静态调用返回值（从 `StdSigs` 签名表读取）

### A.4 验证

| 测试 | 命令 | 预期 |
|------|------|------|
| 类实例字段修改 | `build/probe7/identity.aura` | VM/AOT 结果一致 |
| String 方法调用 | `build/probe7/str_methods.aura`（新增） | `contains`/`startsWith`/`split` 等结果一致 |
| native 返回指针 | `build/probe7/ptr_ret.aura`（新增） | `llc` 不再报类型不匹配 |
| 全量回归 | `cargo test --workspace` | 全部通过 |
| 单文件自举 | `aura-compiler-native.exe build/probe7/hello.aura -o x.exe` | `x.exe` 正确运行 |

### A.5 交付物

- `compiler/src/codegen/aot/emit.rs`（~50 行改动）
- `compiler/src/codegen/aot/types.rs`（~10 行改动）
- `compiler/src/std/cffi/aura_std_cffi.{h,c}`（String 表示同步）
- 3 个新增验证用例

---

## 二、阶段 B：补全 Aura 侧 HIR（1 周）

### B.0 目标

让 `Main.aura` 用到的全部语法在 Aura HIR 层都能正确降级。这是「Aura 编译器 AOT 自身」的前置条件。

### B.1 `when` 表达式降级

**位置**：`aura/compiler/aura/lang/compiler/hir/Desugar.aura`

**现状**：`when` 在 HIR 层尚未降级（`Decl::Enum` 也被丢弃）。

**改动**：
- 新增 `HirStmt::When { subject, branches }`（或降级为嵌套 `if/else` 链）
- 支持：
  - 条件 when（`when (x) { 1 -> ...; else -> ... }`）
  - 主体 when（`when (x) { 1 -> ...; 2 -> ... }`）
  - 范围 when（`in 90..100 -> ...`）
  - `is Type -> ...` 类型窄化

**验证**：`tests/phase6_5_aot_tests.aura` 中新增 `when` 用例，12 组覆盖。

### B.2 lambda / 闭包降级

**位置**：`aura/compiler/aura/lang/compiler/hir/Desugar.aura`、`aot/Emit.aura`

**现状**：`Expr::Lambda | Closure` 在 HIR 中降级为 `HirExpr::Call { callee: "__lambda", args: vec![] }`（占位符）。

**改动**：
- HIR：`HirExpr::Lambda { params, body, captures }`
- AOT Emit：函数指针 + 环境结构体（`%struct.Lambda_N = type { i8*, ... }`）
- 闭包捕获：静态分析捕获变量，按引用拷贝到环境结构体

**验证**：`tests/phase7_jit_tests.aura` 中新增 lambda 用例。

### B.3 字符串插值降级

**位置**：`aura/compiler/aura/lang/compiler/hir/Desugar.aura`

**现状**：Lexer 已支持 `StringLiteral` + `StringInterpStart` + `StringLiteral` 拆分，但 HIR 未降级。

**改动**：
- 复用 Rust 侧方案：`Expr::StrInterp` → `toString(expr) + "+" + toString(expr) + ...`
- 字符串字面量部分直接拼接

**验证**：`tests/phase2_sema_hir_tests.aura` 中新增插值用例。

### B.4 `try/catch/finally` 降级

**位置**：`aura/compiler/aura/lang/compiler/hir/Hir.aura`、`mir/MirLower.aura`

**现状**：HIR 中 `catch` 块被丢弃（README §73）。

**改动**：
- HIR：`HirStmt::Try { body, catch_var, catch_body, finally }`
- MIR：`MirInstr::PushHandler { handler, slot }` / `PopHandler`
- VM 字节码：`PushHandler(offset, slot)` / `PopHandler`（已有 opcode 定义）
- AOT Emit：`setjmp/longjmp` 桥（平台 ABI）

**验证**：`tests/phase5_vm_tests.aura` 中新增 try/catch 用例（catch 绑定异常值、无异常跳过 catch、finally 双路径、跨函数栈展开、嵌套 try）。

### B.5 std 签名表接入 sema

**位置**：`aura/compiler/aura/lang/compiler/sema/TypeChecker.aura`、`aot/StdSigs.aura`

**现状**：`StdSigs.aura` 已有签名表，但未接入 sema——`String.split` 等推断为 `Any`。

**改动**：
- `TypeChecker` 在 `checkCall` 时查询 `StdSigs.stdSignature(name)`
- 命中则按签名推断参数类型 + 返回类型
- 未命中则回退为 `Any`

**验证**：`tests/phase6_5_aot_tests.aura` 中 std 签名表用例。

### B.6 验证

| 测试 | 预期 |
|------|------|
| `aura run tests/phase6_5_aot_tests.aura` | 全部 PASS（12 组 + 新增 when/lambda/try 组） |
| `aura run tests/phase7_jit_tests.aura` | 全部 PASS（12 组 + 新增 lambda 组） |
| `aura run tests/phase5_vm_tests.aura` | 全部 PASS（+ try/catch 组） |

### B.7 交付物

- `hir/Desugar.aura`（~200 行改动）
- `hir/Hir.aura`（~50 行新增）
- `mir/MirLower.aura`（~100 行改动）
- `sema/TypeChecker.aura`（~50 行改动）
- `aot/Emit.aura`（lambda 降级，~150 行）
- 5 个新增验证用例文件

---

## 三、阶段 C：Aura 编译器自举闭环（2–3 周）

### C.0 目标

让 `aura-compiler-native.exe`（阶段 A 产出）能重新编译自身，产出的 `aura-compiler-native2.exe` 与原 exe 行为一致。

### C.1 多模块链接器生产化

**位置**：`aura/compiler/aura/lang/compiler/aot/ModuleLink.aura`

**现状**：已实现（6.5.12），支持相对文件导入、点分包名、`a.*` / `a.{B,C}` 展开、去重与路径规范化。

**改造**：
- 增加 `aura/compiler/aura/lang/compiler/` 包根的显式解析
- 增加 `aura/core/aura/lang/` 包根的显式解析（std 源码）
- 增加错误报告（不可解析即忽略 → 改为报错）

### C.2 AOT 发射器生产化

**位置**：`aura/compiler/aura/lang/compiler/aot/Emit.aura`

**现状**：已实现（6.5.10 前置），支持类/集合/std 签名表/多模块。

**改造**：
- 修复性能瓶颈（见 C.3）
- 增加 `when` / lambda / `try` 发射（阶段 B 产物）
- 增加内存追踪（`--mem-trace` 已有）

### C.3 性能优化

**位置**：`aura/compiler/aura/lang/compiler/aot/Emit.aura`

**现状**：VM 解释执行 ≈ 1 µs/op；发射 15k 节点需数分钟。

**已修复**（6.5.15）：
- HIR arena 由「字符串 `+=`」改为 `List<String>`（`arrayListOf`，O(1) 追加）
- 表/变量查找由 O(len²) 改为单次 `split` 线性扫描（~17x 提速）

**剩余瓶颈**：
- 发射器本身跑在 Rust VM 上，受 VM 解释执行速度限制
- 解决路径：阶段 A 完成后，编译器以原生进程运行，不再受 VM 解释限制

### C.4 自举验证脚本

**新增**：`scripts/self-bootstrap.ps1` / `scripts/self-bootstrap.sh`

**流程**：
```bash
# 阶段 1: 编译原生载体（Rust AOT 后端）
cargo build --release -p cli --features llvm
./target/release/aura.exe build aura/compiler/aura/lang/compiler/Main.aura --aot -o build/bin/aura-compiler-native.exe

# 阶段 2: 用原生载体重新编译自身（Aura 侧 AOT 后端）
./build/bin/aura-compiler-native.exe aura/compiler/aura/lang/compiler/Main.aura -o build/bin/aura-compiler-native2.exe

# 阶段 3: 行为一致性验证
./build/bin/aura-compiler-native.exe  tests/phase9_compiler_tests.aura -o build/test/native1.exe
./build/bin/aura-compiler-native2.exe tests/phase9_compiler_tests.aura -o build/test/native2.exe
./build/test/native1.exe > output1.txt
./build/test/native2.exe > output2.txt
diff output1.txt output2.txt  # 应无差异

# 阶段 4: 性能对比
time ./build/bin/aura-compiler-native.exe  aura/compiler/aura/lang/compiler/Main.aura -o /dev/null
time ./build/bin/aura-compiler-native2.exe aura/compiler/aura/lang/compiler/Main.aura -o /dev/null
```

### C.5 验证标准

| 标准 | 判定 |
|------|------|
| 自举编译成功 | `aura-compiler-native2.exe` 生成，大小合理（> 500KB） |
| 行为一致 | `diff output1.txt output2.txt` 无差异 |
| 性能可接受 | 差异 < 20%（原生载体 vs 自举载体） |
| 可重复 | 连续 3 次自举，输出一致 |

### C.6 交付物

- `aot/ModuleLink.aura`（~50 行改动）
- `aot/Emit.aura`（性能 + 新语法发射，~300 行改动）
- `scripts/self-bootstrap.{ps1,sh}`（新增，~200 行）
- `docs/pure_aura/03-自举验证报告.md`（新增）

---

## 四、阶段 D：std native 层上移到 Aura（3–5 周，最大投入）

### D.0 目标

把 Aura 编译器运行时调用的 std 函数从 Rust native 替换为 Aura 源码实现。完成后 Aura 编译器在 VM 上跑时不再调用任何 Rust native 函数，仅调用 C（通过 `Syscalls.aura` / `FFI`）。

### D.1 单一真相源机制

**问题**：`aura/core/aura/lang/std/*.aura`（38 文件）与 `compiler/src/std/*.rs`（24 文件）双层漂移。

**改造**：
1. `aura/core/aura/lang/std/` 成为唯一真相源
2. `compiler/src/std/*.rs` 改为「包装器」——仅做 Aura 函数名 → C 符号的映射（或直接调用 Aura 函数）
3. `embedded_stdlib.rs` 机制扩展——所有纯逻辑模块预编译为 AOT 机器码嵌入 `.auc`
4. 建立「一致性检查」：`cargo test -p compiler --test stdlib_consistency` 验证 Aura 源码与 Rust 包装器签名一致

### D.2 优先级矩阵

| 优先级 | 模块 | 文件 | 改造方式 | 工期 |
|--------|------|------|----------|------|
| **P0** | `FileSystem.*` / `IO.*` / `Process.*` | `std_fs.rs` / `std_io.rs` / `std_process.rs` | 经 `Syscalls.aura` + C 最小 syscall 层（`aura_syscalls.c`） | 1 周 |
| **P1** | `String.*` / `Math.*` / `List.*` / `Map.*` | `std_string.rs` / `std_math.rs` / `std_collections.rs` | Aura 源码实现（已有），修复编译器缺口 | 1 周 |
| **P2** | `JSON.*` / `Base64.*` / `Hex.*` | `std_json.rs` / `std_encoding.rs` | Aura 实现，纯逻辑上移 | 3 天 |
| **P3** | `Actor.*` / `Channel.*` / `Coroutine.*` | `std_concurrent.rs` | Aura 调度器 + C 线程原语（`pthread`） | 1 周 |
| **P4** | `Hash` / `SHA256` / `HMAC` / `Ed25519` | `signature.rs` / `signing.rs` | Aura 实现纯算法；底层字节操作走 syscall | 3 天 |
| **P5** | `Tar` / `Zstd` | — | **保留 C 实现 + FFI**（Zstd 性能敏感） | 1 天 |

### D.3 P0：syscalls 层（关键路径）

#### D.3.1 编译 `Syscalls.aura` 的 `@native` 语法

**问题**：`Syscalls.aura` 中的 `@native(SYS_READ)` 等注解目前**未被编译器解析**。

**改造**：
- 位置：`compiler/src/parser.rs`、`compiler/src/ast.rs`、`compiler/src/sema/checker.rs`
- 新增 AST 节点：`Decl::ExternObject { name, methods }`、`ExternMethod { name, params, ret, native_attr }`
- `@native(N)` 表示 syscall 号（int）
- `@native(asm = "...")` 表示内联汇编（`Cpu.aura`）
- `@native` 无参数表示编译器内置（`Memory.aura`）
- HIR 降级：`HirCall::ExternMethod { obj, method, args }`
- AOT 发射：
  - syscall → `call i64 @syscall(i64 N, ...args)`（LLVM `@syscall` 内建或 `call i64 i64(@abi("sysv") i32)`）
  - asm → 内联汇编 `call i64 asm "...", "r" (...)`
  - 内置 → 直接发射 VM/JIT 指令

#### D.3.2 新增 `aura_syscalls.c`

**新增文件**：`compiler/src/std/cffi/aura_syscalls.c`

```c
// 通用 syscall 分发（x86_64 Linux）
extern int64_t aura_syscall(int64_t nr, ...);

// Windows 等价实现（使用 syscall API 或封装 Nt* 函数）
// 平台检测：#if defined(_WIN32) ... #else ... #endif
```

**说明**：这是 Layer 0-B 的扩展，用 C 实现而非 Rust。

#### D.3.3 `Memory.aura` 内置指令

**位置**：`compiler/src/vm/interp.rs`、`compiler/src/codegen/aot/emit.rs`

**改动**：
- `Memory.read(addr)` → 发射 VM `LOAD_RAW addr` 指令 / AOT `load i8, i8* %addr`
- `Memory.alloc(n)` → 发射 `CallNative` 到 `aura_mmap` / AOT `call i64 @aura_mmap(i64, i32, i32, i32, i64)`
- `Memory.free(addr)` → `CallNative` 到 `aura_munmap`

### D.4 P1：纯逻辑模块上移

#### D.4.1 `String.aura` 实现修复

**位置**：`aura/core/aura/lang/std/String.aura`

**已知问题**（README §524-533）：
- `String.charCodeAt` / `String.fromCharCode` 返回异常值
- native 同名回退拦截（`String.toInt` 被解析为原生）

**改动**：
- 修复 `charCodeAt` 实现（按 UTF-16 code unit 读取）
- 修复 `fromCharCode` 实现
- 建立「Aura 实现优先」机制：`do_call_native` 中 Aura 实现优先于 Rust native

#### D.4.2 `Math.aura` 实现修复

**位置**：`aura/core/aura/lang/std/Math.aura`

**已知问题**：`Math.min/max` 被解析为原生 prelude，返回错值。

**改动**：
- 同 D.4.1，建立「Aura 实现优先」机制
- `Math` 类静态方法（`Math.sin` / `Math.cos`…）经 C 运行库调用

#### D.4.3 `Array<T>` 下标降级

**位置**：`aura/compiler/aura/lang/compiler/hir/Desugar.aura`

**已知问题**：`Array<T>` / `ArrayList<T>` 的 `data[i]` 无法索引。

**改动**：
- HIR 降级 `HirIndex { base, index, elemTy }`
- VM 字节码：`GET_FIELD data` + `GET_INDEX i`
- AOT 发射：`getelementptr [N x T], ...` 或 `call i8* @aura_list_get_at(i8*, i64)`

#### D.4.4 函数类型参数与容器构建

**位置**：`aura/compiler/aura/lang/compiler/hir/Desugar.aura`

**已知问题**：`Iter` 中带 `(Any)->*` 回调的函数（`countWhere`/`every`/`some`）在 VM 中丢失结果。

**改动**：
- `HirType::Function { params, ret }` 变体
- `arrayListOf()` 构建后返回的 `map`/`filter`/`reverse`/`take`/`range` 在 VM 中正确返回

### D.5 P2：JSON / 编码上移

**位置**：`aura/core/aura/lang/std/Json.aura`、`Encoding.aura`

**现状**：已有 Aura 源码实现，但未接入运行时。

**改动**：
- 修复 `Json.aura` 中的 `"\b"` / `"\f"` 转义（已修复，见 README §493）
- 验证 `Json.stringify` / `Json.parse` 与 Rust 实现一致
- 接入 `embedded_stdlib.rs` 机制

### D.6 P3：并发运行时上移

**位置**：`aura/core/aura/lang/coroutine/Actor.aura`、`Coroutine.aura`

**已知问题**（README §43-52）：
- 协程调度器是单线程协作式（无 OS 线程）
- Actor 运行时单线程（无 `Arc`/`Mutex`）
- `select` 是多路轮询
- Channel 无超时机制
- Actor 监督无死亡传播
- `await` 在非协程上下文静默 no-op

**改动**：
- 新增 `aura_syscalls.c` 中的 `pthread_create` / `pthread_join` / `pthread_mutex_*` 封装
- Aura 侧 `Actor.aura` 增加 OS 线程调度
- `Channel.aura` 增加超时机制
- `select` 改为事件驱动（poll/epoll 封装）

### D.7 P4：密码学上移

**位置**：`aura/core/aura/lang/std/Encoding.aura`（扩展 SHA256/HMAC）

**改动**：
- `SHA256`：Aura 实现（纯逻辑，无 FFI）
- `HMAC`：Aura 实现（基于 SHA256）
- `Ed25519`：**保留 C 实现 + FFI**（性能敏感，且涉及曲线运算）
- `Base64` / `Hex`：Aura 实现

### D.8 验证

| 测试 | 预期 |
|------|------|
| `aura run tests/phase8_stdlib_tests.aura` | 全部 PASS（46 断言 + 新增组） |
| `aura stdlib-compile aura/core/aura/lang --output build` | 45/45 成功（已实现） |
| `aura run tests/self_bootstrap/vm_test.aura` | 15/15 PASS |
| `cargo test -p compiler --test stdlib_consistency` | 全部 PASS（新增一致性检查） |

### D.9 交付物

- `compiler/src/parser.rs`（`extern object` 解析，~100 行）
- `compiler/src/ast.rs`（`Decl::ExternObject`，~50 行）
- `compiler/src/sema/checker.rs`（`@native` 注解处理，~80 行）
- `compiler/src/codegen/aot/emit.rs`（syscall/asm/内置发射，~200 行）
- `compiler/src/std/cffi/aura_syscalls.c`（新增，~500 行）
- `aura/core/aura/lang/std/*.aura`（修复 + 补齐，~1500 行改动）
- `aura/core/aura/lang/coroutine/*.aura`（并发运行时，~500 行改动）
- `compiler/src/std/mod.rs`（Aura 实现优先机制，~50 行）
- 新增一致性检查测试

---

## 五、阶段 E：CLI / LSP / loom 上移（4–8 周，可选）

### E.0 目标

让 `aura`、`aura-lsp`、`aura-debug`、`loom` 也都用 Aura 实现。完成后**完全删掉 `compiler/` + `cli/` + `loom/`**，仅保留 `compiler/src/bootstrap/` + `aura_std_cffi.c`。

> 本阶段为「可选」——如果用户接受「Rust 构建 Rust AOT 编译器，再用 Aura 编译器编译 Aura 程序」的模式，可以跳过本阶段。但「完全脱 Rust」需要本阶段。

### E.1 `AuraCli.aura`（替代 `cli/src/main.rs`）

**位置**：`aura/toolchain/aura/lang/cli/AuraCli.aura`（新增）

**覆盖命令**：
- `aura build <file.aura> [--output <out>]`
- `aura build <file.aura> --aot [--output <exe>]`
- `aura build <file.aura> --lib [--output <out>]`
- `aura run <file.aura>`
- `aura check <file.aura>`
- `aura disasm <file.auc>`
- `aura tokens <file.aura>`
- `aura ast <file.aura>`
- `aura fmt <file.aura> [--check]`
- `aura leak-check <file.aura>`
- `aura doc [--output <dir>]`
- `aura eval [--expr <code>]`
- `aura repl`
- `aura install` / `update` / `publish` / `deps` / `new`
- `aura package <file.aura>` / `inspect <file.auz>` / `verify <file.auz>`
- `aura lsp`
- `aura debug <file.aura>`

**工期**：2 周

### E.2 `AuraLsp.aura`（替代 `compiler/src/lsp.rs`）

**位置**：`aura/toolchain/aura/lang/lsp/AuraLsp.aura`（新增）

**覆盖**：
- JSON-RPC over stdio
- `textDocument/didOpen` / `didChange` / `didClose`
- `textDocument/completion` / `hover` / `definition` / `references`
- `textDocument/diagnostic`
- `textDocument/formatting`

**工期**：2 周

### E.3 `AuraDebugger.aura`（替代 `cli/src/debugger.rs`）

**位置**：`aura/toolchain/aura/lang/debugger/AuraDebugger.aura`（新增）

**覆盖**：
- 断点管理（源文件 + 行号 → 字节码地址映射）
- 单步执行（step over / step into / step out）
- 变量查看（栈帧 + 局部变量）
- 表达式求值（REPL）
- 调用栈查看

**工期**：1 周

### E.4 `Loom.aura`（替代 `loom/src/**`）

**位置**：`aura/toolchain/aura/lang/loom/Loom.aura`（新增）

**覆盖**：
- `aura.toml` 解析（Manifest）
- 任务 DAG（TaskGraph）
- 任务执行器（TaskExecutor）
- 增量构建（fingerprint + cache）
- 插件系统（convention / explicit / external）
- 包管理（builder / installer / reader）
- CI/CD 集成（config / phases）
- Watch 模式（monitor）

**工期**：3 周

### E.5 验证

| 测试 | 预期 |
|------|------|
| `aura --help` | 与 Rust CLI 输出一致 |
| `aura build tests/phase9_compiler_tests.aura` | 成功 |
| `aura run examples/compiler/showcase.aura` | 成功 |
| `aura repl` | 交互式 REPL 可用 |
| `aura lsp`（接 VS Code 扩展） | LSP 协议完整 |
| `loom build` | 构建成功 |
| `loom test` | 测试通过 |
| `loom watch` | 增量重建正确 |

### E.6 交付物

- `aura/toolchain/aura/lang/cli/AuraCli.aura`（新增，~3000 行）
- `aura/toolchain/aura/lang/lsp/AuraLsp.aura`（新增，~2000 行）
- `aura/toolchain/aura/lang/debugger/AuraDebugger.aura`（新增，~1500 行）
- `aura/toolchain/aura/lang/loom/Loom.aura` + 子模块（新增，~3000 行）
- 验证脚本 `scripts/verify-pure-aura.ps1`（新增）

---

## 六、阶段 D/E 完成后的删除清单

### D.1 可删除（阶段 D 完成后）

| 模块 | 说明 |
|------|------|
| `compiler/src/std/*.rs`（除 `cffi/`、`decl.rs`、`embedded_stdlib.rs`） | std native 实现（~24 文件） |
| `compiler/src/vm/native.rs` | NativeRegistry 注册表（~300 行） |
| `compiler/src/vm/interp.rs` 中 `do_call_native` / `do_call_native_args` | 原生函数派发（~200 行） |

### D.2 可删除（阶段 E 完成后）

| 模块 | 说明 |
|------|------|
| `compiler/src/parser.rs` | 解析器（144KB） |
| `compiler/src/lexer.rs` | 词法器（67KB） |
| `compiler/src/ast.rs` | AST 定义（25KB） |
| `compiler/src/sema/**` | 语义分析（~20KB） |
| `compiler/src/codegen/**`（除 `aot/`） | HIR/MIR/字节码发射（~30KB） |
| `compiler/src/codegen/aot/` | Rust AOT 后端（~50KB） |
| `compiler/src/vm/**`（除 `native.rs` 已删） | 主 VM（~40KB） |
| `compiler/src/linker.rs` / `signature.rs` / `signing.rs` | 模块链接 / 签名（~25KB） |
| `compiler/src/lsp.rs` | LSP 服务（39KB） |
| `compiler/src/docgen.rs` | 文档生成（70KB） |
| `compiler/src/package.rs` | 包管理（65KB） |
| `compiler/src/source_map.rs` / `span.rs` / `token.rs` / `errors.rs` | 基础设施（~20KB） |
| `cli/src/**` | CLI（~90KB） |
| `loom/src/**` | 构建系统（~50 文件） |

### D.3 保留

| 模块 | 说明 |
|------|------|
| `compiler/src/bootstrap/`（9 文件） | 最小 Rust 引导层（Layer 0-A） |
| `compiler/src/std/cffi/`（2 文件） | C 运行库（Layer 0-B） |
| `compiler/Cargo.toml` | Cargo 配置（仅 bootstrap + cffi） |
| `Cargo.toml`（根） | workspace 配置 |
| `aura/core/aura/lang/std/cffi/` | C 运行库声明 |
| `aura/compiler/` | Aura 编译器 |
| `aura/core/` | Aura 标准库 |
| `aura/toolchain/` | Aura CLI / LSP / 调试器 / loom |
| `scripts/` | 构建脚本 |
| `docs/` / `book/` | 文档 |
| `tests/` | 测试 |

---

## 七、阶段化交付与里程碑

| 里程碑 | 阶段 | 工期 | 验收标准 | 可删除 Rust 代码 |
|--------|------|------|----------|------------------|
| **M1** | A（Rust AOT 后端加固） | 1–2 周 | `aura-compiler-native.exe` 正确 AOT 单文件程序 | — |
| **M2** | B（Aura HIR 补全） | 1 周 | 全部 phase 测试 PASS | — |
| **M3** | C（自举闭环） | 2–3 周 | `aura-compiler-native2.exe` 与原 exe 行为一致 | — |
| **M4** | D.1–D.4（P0–P1 上移） | 2 周 | std 核心模块纯 Aura 实现 | `compiler/src/std/*.rs`（P0/P1 部分） |
| **M5** | D.5–D.7（P2–P4 上移） | 1–2 周 | std 全部模块纯 Aura 实现 | `compiler/src/std/*.rs`（除 cffi/decl/embedded） |
| **M6** | E（CLI/LSP/loom 上移） | 4–8 周 | `aura` / `aura-lsp` / `loom` 全部 Aura 实现 | `cli/**`、`loom/**`、`compiler/src/**`（除 bootstrap） |

**总工期**：11–19 周（约 2.5–4 个月）

---

## 八、风险矩阵

| # | 风险 | 概率 | 影响 | 缓解 |
|---|------|------|------|------|
| R1 | 阶段 A 的 AOT 改动破坏现有用户程序 | 中 | 高 | 全量回归测试 + 保留 Rust 编译器 fallback |
| R2 | 阶段 D 的 `Syscalls.aura` 跨平台问题 | 中 | 高 | 先支持 x86_64 Linux，再补 Windows（Nt* API） |
| R3 | 阶段 D 的并发运行时（Actor/Channel）性能不达标 | 中 | 中 | 保留 C 实现 + FFI，仅调度逻辑上移 |
| R4 | 阶段 D/E 期间用户感知不到改善 | 高 | 低 | 保留 Rust 编译器作为 fallback，直到 M6 |
| R5 | 阶段 C 的性能瓶颈（VM 解释 1µs/op）无法突破 | 低 | 高 | 阶段 A 完成后编译器以原生进程运行，不受 VM 限制 |
| R6 | 阶段 E 的 loom 上移工作量大（50 文件） | 高 | 中 | 可跳过阶段 E，接受「Rust 构建 Rust 编译器」模式 |
| R7 | `embedded_stdlib.rs` 机制与 Aura 实现优先机制冲突 | 中 | 中 | 建立「调用优先级」：AOT 机器码 > Aura 字节码 > Rust native |
| R8 | `@native` 注解语法与现有语法冲突 | 低 | 中 | 新增 AST 节点 + 专用解析路径，不影响现有语法 |
| R9 | 阶段 D 期间 std 双层漂移导致 bug | 高 | 中 | 建立「一致性检查」测试（M4 前） |
| R10 | 外部工具链（LLVM/C 编译器）版本漂移 | 中 | 低 | 五级探测机制已就位，补充 Linux 路径 |

---

## 九、阶段间依赖关系

```
阶段 A ──→ 阶段 B ──→ 阶段 C ──┬──→ 阶段 D.1 ──→ D.2 ──→ D.3 ──→ D.4 ──┬──→ 阶段 E
                                │                                          │
                                └──→ (可选) 直接停在此，保留 Rust CLI/loom │
                                                                         │
                                                               ───→ 完全脱 Rust ───┘
```

**关键路径**：A → B → C → D.1 → D.2 → D.3 → D.4 → E

**最短路径**（接受 Rust CLI/loom）：A → B → C → D.1 → D.2 → D.3 → D.4

---

## 十、验证体系

### 10.1 自动化验证脚本

**新增**：`scripts/verify-pure-aura.ps1` / `scripts/verify-pure-aura.sh`

```bash
# 阶段 A 验证
cargo test -p compiler --test aot_regression  # 全量回归
./build/probe7/identity.aura                  # 类实例字段修改

# 阶段 B 验证
./target/release/aura.exe run tests/phase6_5_aot_tests.aura
./target/release/aura.exe run tests/phase7_jit_tests.aura
./target/release/aura.exe run tests/phase5_vm_tests.aura

# 阶段 C 验证
./scripts/self-bootstrap.sh                   # 自举闭环
diff output1.txt output2.txt                  # 行为一致

# 阶段 D 验证
./target/release/aura.exe run tests/phase8_stdlib_tests.aura
cargo test -p compiler --test stdlib_consistency

# 阶段 E 验证
./aura --help                                 # CLI
./aura build tests/phase9_compiler_tests.aura  # 编译
./aura run examples/compiler/showcase.aura     # 运行
./loom build                                  # 构建
```

### 10.2 行为一致性矩阵

| 测试集 | Rust 编译器 | Aura 编译器（VM） | Aura 编译器（AOT） | 自举载体 |
|--------|-------------|-------------------|-------------------|----------|
| `tests/phase0_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/phase1_lexer_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/phase2_sema_hir_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/phase3_mir_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/phase5_vm_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/phase6_aot_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/phase6_5_aot_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/phase7_jit_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/phase8_stdlib_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/phase9_compiler_tests.aura` | ✅ | ✅ | ✅ | ✅ |
| `tests/self_bootstrap/vm_test.aura` | ✅ | ✅ | ✅ | ✅ |

---

## 十一、总结

### 11.1 阶段划分

| 阶段 | 工期 | 核心目标 | 可删除 Rust 代码 |
|------|------|----------|------------------|
| **A** | 1–2 周 | 加固 Rust AOT 后端（类按引用 + String 统一 + emit_call） | — |
| **B** | 1 周 | 补全 Aura 侧 HIR（when/lambda/插值/try/std 签名表） | — |
| **C** | 2–3 周 | Aura 编译器自举闭环 | — |
| **D** | 3–5 周 | std native 上移到 Aura | `compiler/src/std/*.rs`（~24 文件） |
| **E** | 4–8 周 | CLI/LSP/调试器/loom 上移 | `cli/**` + `loom/**` + `compiler/src/**`（除 bootstrap） |

### 11.2 最终形态

```
保留（不可脱）：
  compiler/src/bootstrap/     最小 Rust 引导层（~4000 行）
  compiler/src/std/cffi/      C 运行库（~3000 行）

删除（阶段 D/E 完成后）：
  compiler/src/（除 bootstrap + cffi）  ~120KB
  cli/src/                                ~90KB
  loom/src/                               ~50 文件

全部用 Aura 实现：
  aura/compiler/aura/lang/compiler/   完整编译器（65 文件）
  aura/core/aura/lang/std/            标准库（38 文件）
  aura/toolchain/aura/lang/           CLI/LSP/调试器/loom（新增）

外部工具链（非 Rust）：
  LLVM (llc/clang/lld-link)           AOT 机器码生成
  C 编译器                              编译 aura_std_cffi.c
```

### 11.3 关键决策点

| 决策 | 选项 | 影响 |
|------|------|------|
| 是否做阶段 E？ | 做 / 不做 | 不做则保留 Rust CLI/loom，但 aura 编译器本身已脱 Rust |
| 是否保留 Rust 编译器 fallback？ | 保留 / 删除 | 保留则用户无感，删除则彻底脱 Rust |
| 是否实现 `@native(asm)`？ | 实现 / 不实现 | 不实现则无法内联汇编（`Cpu.aura`），但可保留 C 实现 |
| 是否实现并发上移？ | 实现 / 保留 C | 实现则完整脱 Rust，保留 C 则性能更优 |

### 11.4 结论

> **按本方案推进，Aura 编译器可在 11–19 周内完全脱离 Rust**（除 bootstrap 引导层 + C 运行库 + LLVM 工具链）。阶段 A–C 是「自举闭环」的必经之路（5–6 周），阶段 D 是「std native 上移」的核心投入（3–5 周），阶段 E 是「工具链完整上移」的可选项（4–8 周）。
>
> **最短路径**（接受 Rust CLI/loom）：A → B → C → D（约 7–11 周），此时 `aura` 命令本身仍为 Rust，但编译器 + VM + std 全部为 Aura。
>
> **完全路径**（彻底脱 Rust）：A → B → C → D → E（约 11–19 周），此时 `aura` / `aura-lsp` / `aura-debug` / `loom` 全部为 Aura，仅保留 `compiler/src/bootstrap/` + `aura_std_cffi.c` + LLVM。

---

*本文档为「详细改造方案」，配套 `01-现状分析.md`（现状评估）一并阅读。*

---

## 十二、阶段 D 实施记录（2026-07-05）

### 12.1 完成内容

| 交付物 | 状态 | 说明 |
|--------|------|------|
| `compiler/src/token.rs` | ✅ | 新增 `Native` token 类型 |
| `compiler/src/lexer.rs` | ✅ | 新增 `"native"` 关键字 |
| `compiler/src/ast.rs` | ✅ | 新增 `NativeAttr` 枚举 + `FnDecl.native_attr` 字段 |
| `compiler/src/parser.rs` | ✅ | `@native(N)` / `@native(asm="...")` / `native fun` 解析 |
| `compiler/src/ast.rs` | ✅ | `Decl::ExternObject` 命名修复（原 ExternInterface） |
| `compiler/src/sema/checker.rs` | ✅ | ExternObject 处理（@aot 校验 + 符号表注册） |
| `compiler/src/codegen/hir.rs` | ✅ | ExternObject → HirFunction（is_native） |
| `compiler/src/std/cffi/aura_syscalls.c` | ✅ | 系统调用分发层（17 个 syscall + 3 个 CPU 指令） |
| `compiler/src/std/embedded_stdlib.rs` | ✅ | 嵌入式标准库从 4 模块扩展到 16 模块（292 函数） |
| `compiler/tests/stdlib_consistency.rs` | ✅ | 一致性检查测试（2 个测试用例） |

### 12.2 验证结果

| 测试 | 结果 |
|------|------|
| `aura run tests/phase0_tests.aura` | ✅ PASS |
| `aura run tests/phase1_lexer_tests.aura` | ✅ PASS |
| `aura run tests/phase3_mir_tests.aura` | ✅ PASS |
| `aura run tests/phase5_vm_tests.aura` | ✅ PASS |
| `aura run tests/phase6_aot_tests.aura` | ✅ PASS |
| `aura run tests/phase7_jit_tests.aura` | ✅ PASS |
| `aura run tests/phase8_stdlib_tests.aura` | ✅ PASS（46 断言） |
| `cargo test -p compiler --test stdlib_consistency` | ✅ PASS（2 测试） |
| `aura stdlib-compile aura/core/aura/lang/std --output build` | ✅ 20/20 成功 |

### 12.3 嵌入式标准库扩展

阶段 D 前：4 模块（Math, Time, Collections, Test）= 84 函数
阶段 D 后：16 模块 = 292 函数

新增模块：Ascii, Assert, Encoding, Iter, Json, StringBuilder, TestHelper, Path, String, Actor, Channel, Coroutine

### 12.4 遗留项

- AOT 发射器（`emit.rs`）尚未完全处理 @native 注解的 LLVM IR 发射（syscall → `call i64 @aura_syscall_dispatch(i64 N, ...)`，asm → 内联汇编，builtin → 直接 VM 指令）
- `@native(asm = "...")` 内联汇编支持需在 AOT 发射器中实现
- `Memory.aura` 的 `native fun` 已在解析层支持，但 AOT 发射器需增加对应的 LLVM 指令映射
- 部分测试文件（phase2_sema_hir, phase6_5_aot, phase9_compiler）存在导入路径解析问题（pre-existing）

