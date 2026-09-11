# Aura 编译器（纯 Aura 化 · 独立代码层）

本目录是**用 Aura 语言本身编写的 Aura 编译器**，是
[`docs/编译器LLVM交互分析与纯Aura化迁移计划.md`](../../docs/编译器LLVM交互分析与纯Aura化迁移计划.md)
方案四（分阶段迁移）的落地位置。

> **核心原则**：Rust 编译器（仓库根 `compiler/`）**完全保留、零修改**，作为 fallback 与
> 参考实现。本目录与 `compiler/` **并行独立**，可并行构建、对比测试，任何 Phase 失败
> 只需放弃本目录，不影响现有编译能力。

## 目录布局

包根为 `aura/compiler/`，其内部按模块路径镜像为 `aura/lang/compiler/...`
（与 `aura/core/aura/lang/std/...` 的约定一致）。

```
aura/compiler/
├── README.md                              # 本文件
└── aura/lang/compiler/
    ├── Main.aura                          # 编译器入口 + 编译管线（Phase 9）
    ├── lexer/
    │   ├── Span.aura                      # Phase 1 ✅ 源码位置
    │   ├── Token.aura                     # Phase 1 ✅ Token 数据模型 + 关键字表
    │   └── Lexer.aura                     # Phase 1 ✅ 词法分析器
    ├── ast/                               # Phase 1 ✅ Ast.aura
    ├── parser/                            # Phase 1 ✅ Parser.aura
    ├── errors/                            # Phase 1 ✅ CompileError.aura
    ├── sema/                              # Phase 2 ✅ Type/TypeInfo/SymbolTable/TypeChecker
    ├── hir/                               # Phase 2 ✅ Hir/Desugar/Mono/Inline/Fold
    ├── mir/                               # Phase 3 ✅ Mir/MirLower/MirOpt
    ├── codegen/                           # Phase 4 ✅ Codegen.aura（MIR → 字节码）
    ├── vm/                                # Phase 4/5 ✅ Vm/VmRunner/Opcodes/Frames/
    │                                      #            FrameManager/TailCall/Closures
    ├── aot/                               # Phase 6 ✅ AOT LLVM 后端（Aura 化）
    ├── jit/                               # Phase 7 ✅ JIT（Cranelift）Aura 化
    │                                      # Phase 9 ✅ 单一字节码 + 模块级 JIT 入口
    ├── gc/ memory/ runtime/               # 已有 VM 运行时雏形
    └── test/
        ├── TestRunner.aura                # Phase 0  Aura 测试框架
        └── TestRunnerSelfTest.aura        # Phase 0  自检入口
```

## Phase 1 交付物（前端 Aura 化）

| # | 交付物 | 路径 | 状态 |
|---|--------|------|------|
| 1.1 | Token 模型（TokenKind 名称表 + 关键字表） | `.../lexer/Token.aura` | ✅ |
| 1.2 | Span（源码位置 + 合并） | `.../lexer/Span.aura` | ✅ |
| 1.4 | Lexer（单遍扫描，全部 TokenKind） | `.../lexer/Lexer.aura` | ✅ |
| 1.5 | AST 定义 | `.../ast/Ast.aura` | ✅ |
| 1.6 | Parser（递归下降 + Pratt） | `.../parser/Parser.aura` | ✅ |
| 1.7 | CompileError | `.../errors/CompileError.aura` | ✅ |
| 1.9/1.10 | Phase 1 验证用例 + 与 Rust 输出对比 | `tests/phase1_lexer_tests.aura` | ✅（Lexer 部分） |

**Lexer 验收结论**：4 个快照用例（`tests/snapshots/cases/*.aura`）经 Aura 词法器产出的
TokenKind 序列与 Rust 参考实现（`aura tokens` 基线）**逐字节一致**（含字符串插值）。

### 上游运行时缺口的修复记录

Phase 1 期间暴露的 VM / 标准库缺陷已逐项修复（`compiler/` 侧），并配套回归测试
`compiler/tests/runtime_gap_tests.rs`（48 项）：

| # | 缺陷 | 根因 | 修复位置 | 状态 |
|---|------|------|----------|------|
| 1 | `isNull(v)` 恒返回 `false`；`listOf(...)` 被忽略 | prelude 调用被解析为**裸名**原生调用，而注册表只有 `aura.lang.std.Builtin.*` 命名空间形式 | `std::register_prelude`（新增）+ `vm::native` 两条注册路径 | ✅ |
| 2 | `s[i]` 字符串索引返回 `null` | `Instr::GetIndex` 未处理 `Value::Str` | `vm::interp` | ✅ |
| 3 | `substring` / `repeat` / `padStart` / `padEnd` 结果错误 | 原生函数把 `args[0]` 当作长度/起点（接收者在 `args[0]`） | `std::std_string`（兼容两种参数顺序） | ✅ |
| 4 | `xs.size` / `xs.first` / `xs.last` / `xs.isEmpty` 不可用 | `GetField` 对 `Value::List`/`Str` 一律返回 null | `vm::interp::builtin_member`（FNV 字段名哈希识别） | ✅ |
| 5 | `"abc".length()` / `"abc"[1]` 解析失败 | 字面量主表达式提前 `return`，未进入后缀链 | `parser::parse_postfix_chain` | ✅ |
| 6 | `object` 单例字段读取为 `null`（`var n: Int = 5`） | `create_singletons` 只能置 `Null`，拿不到字段默认值 | HIR 合成 `<Object>.__singletonInit` + VM 入口前调用 | ✅ |
| 7 | `listOf("a","b")` 得到的值无法索引/取大小 | 同名内嵌 Aura 实现（返回 `ArrayList` 类实例）优先于原生实现 | `vm::interp`：同名原生已注册时以原生为准 | ✅ |
| 8 | 实参多于形参时 VM panic | `Frame::new` 越界写入 locals | `vm::Frame::new` 加边界保护 | ✅ |
| 9 | **异常传播缺失**：`try/catch` 不生效、`throw` 被静默忽略 | HIR 丢弃 `catch` 块；VM 无 handler 栈 | 全链路新增异常支持（见下） | ✅ |
| 10 | `Process.exit(code)` 不影响退出码 | ① 原生名解析为 `Process.exit` 但只注册了命名空间形式；② 单例方法注入 `self` 使 `args[0]` 错位 | `std::register_prelude` 补别名 + `last_int_arg` + VM `request_exit` + CLI 采用退出码 | ✅ |
| 11 | 无 import 的程序 `"abc".length()` 静默得到 **0** | `NativeRegistry::new()` 文档声称「全量注册」，实际只注册了硬编码 prelude 子集，未调用 `std::register_all()` | `vm::native` 的 `new()` 先执行 `std::register_all()` | ✅ |
| 12 | `import ...Actor.*` 的 `spawnActor` 爆栈 | import 解析产出 `aura.lang.std.Actor.spawnActor`，但 HIR 只注册了 `Coroutine.spawnActor` → 调用退化为 `Call(0)`（main）无限递归 | HIR 补注册 `Actor.spawnActor` | ✅ |
| 13 | docgen 一致性检查失败（7 项缺失） | `NativeRegistry::new()` 缺 `Coroutine.spawnActor` 与 `aura.ffi.*` 别名 | 抽出 `register_concurrent` / `register_ffi_aliases` 供两条注册路径共用 | ✅ |
| 14 | `source_index` 预加载断言失败 | 测试只加了 15 个基础类型 + 3 个模块（18），却断言 `>= 20` | 补齐 stdlib 模块并按实际数量修正注释 | ✅ |
| 15 | 3 个 insta 快照过期 | 基线记录的是「无插值」的旧行为，词法/语法已支持 `StrInterp` | 经 pristine 对比确认新输出正确后接受新基线 | ✅ |
| 16 | `is String -> value.length` 未窄化 | ① 窄化未剥离 `is` 模式的 `__is__` 前缀；② 窄化产生 `Ty::Named("String")`，拿不到内置成员表 | sema 剥离前缀 + 新增 `Ty::from_name` 映射基础类型 | ✅ |
| 17 | p10 并发测试随机失败（1~12 项浮动） | VM 实例栈 `VM_STACK` 是**进程全局**（注释却写 thread-local），并行执行时 `get_vm_ref()` 返回别的线程的 VM | 改为 `thread_local!` 的线程局部栈 | ✅ |
| 18 | `bootstrap::test_ffi_aot_direct_vm` 随机得到 6 而非 5 | 对**非 NUL 结尾**的 `Rc<str>` 直接调用 libc `strlen`，越过串尾读相邻堆内存（未定义行为） | 直接取字节长度，不再调用 `strlen` | ✅ |

#### 异常传播（`try/catch/finally`）实现

跨 5 层的最小实现，语义为标准栈式异常处理：

| 层 | 变更 |
|----|------|
| AST | 复用既有 `Expr::Try { block, catches, finally }` |
| HIR | 新增 `HirStmt::Try { body, catch_var, catch_body, finally }`（此前 `catch` 被丢弃） |
| MIR | `MirInstr::PushHandler { handler, slot }` / `PopHandler`；`lower_try` 生成「try 体 / 正常 finally / 异常 finally / merge」四块结构 |
| OpCode | `PushHandler(i32 offset, u16 slot)` = 字节 82（7 字节）；`PopHandler` = 83 |
| VM | `handlers: Vec<Handler>`（帧号 + 目标 ip + 栈高 + 落点槽）；`throw` 触发 `raise()` 弹出最近 handler、**截断帧栈**、把异常值写入 catch 变量槽后跳转；`pop_frame` 清理同帧 handler |

已覆盖的场景（回归测试逐项验证）：catch 绑定异常值、无异常时跳过 catch、
finally 双路径执行、**跨函数栈展开**、嵌套 try、handler 正常结束后注销、
循环内每轮重新注册、仅 finally 时**重抛给外层**、未捕获异常报运行时错误。

#### 字符串插值（`$var` / `${expr}`）

- **Lexer**：`scanString` 按 Rust 协议拆分为
  `StringLiteral(前缀)` + `StringInterpStart("$var" | "${expr}")` + `StringLiteral(中段)` … + `StringLiteral(尾部)`；
  `$5`（`$` 后接数字）与 `$$` 仍作字面量。**Aura 词法器产出与 Rust 基线逐字节一致**。
- **Parser / HIR**：既有实现已支持（`Expr::StrInterp` → `toString` + 加法链），无需改动。

**仍待实现的已知差异**：

- **`catch (e: Type)` 类型过滤**：当前等价于 catch-all（仅建模首个 catch 子句）。
- **AOT 路径**：`aot/emit.rs` 是独立的 HIR→LLVM 通路，尚未支持 `try/catch`；
  解释器路径（默认）行为正确。
- **`sema` 对 `List<T>` 索引误报**：运行期正常，仅告警噪音（`String` 索引被推断为 `Char`，
  需显式 `toStr` 转换）。
- **未解析调用退化为递归**：裸名 `exit(1)` 之类未解析调用会退化成 `Call(0)`（递归入口），
  属既有 codegen 隐患，建议后续改为编译期报错。

#### 已修复：无 import 的程序拿不到 std 原生函数 → **静默得到错值**

- **现象**：未写 `import` 的程序调用 `"abc".length()` 得到 **0**（应为 3）。
- **根因链**：
  1. `Vm::new` 按 `module.enabled_modules` 是否为空选择注册策略：空 → `NativeRegistry::new()`；
     非空 → `with_modules([...])`（按需注册，by design）。
  2. 无 import 时 `enabled_modules` 确实为空，于是走 `NativeRegistry::new()`。
     但该函数的文档写的是「注册**全部**内置原生函数」，实现却只注册了一份**硬编码的
     prelude 子集**，从未调用 `std::register_all()`。
  3. 结果：无 import 的程序反而拿不到 `aura.lang.std.String.length`、`Math.sin` 等
     模块原生函数；调用落入 `do_call_native` 的「未链接的外部函数」兜底分支 ——
     仅向 stderr 打印提示，**却返回 `Int(0)`**，于是 `.length()` 静默得到 0。
  4. 反之，写了 `import aura.lang.std.String` 的程序走 `with_modules(["string"])`
     反而能拿到正确的实现——形成了「不 import 更糟」的反直觉行为。
- **修复**：`NativeRegistry::new()` 改为先调用 `std::register_all()` 做真正全量注册，
  再保留原有硬编码 prelude（置于其后，使基础条目在重名时仍优先，仅补齐缺失模块）。
- **收益**：
  - `tests/self_bootstrap/vm_test.aura` 从 14/15 提升到 **15/15 全部通过**（此前
    「测试 4：字符串操作」长期静默失败）；
  - 修复两处既有失败：`p1c_volume_tests`（5/1 → 6/0）、`p3_feature_tests`（3/3 → 3/0）。
- **遗留风险**：「未链接的外部函数」兜底仍返回 `Int(0)`（仅 stderr 提示）。若某原生
  函数因 feature 未启用而缺失，仍会静默得到 0。建议后续改为加载/编译期报错。

**全部测试通过（本轮结束时）**

- `cargo test --workspace` **全绿**：连续 3 次全量运行 0 失败。
- `cargo test -p compiler`：lib 287、import_syntax 20、p10 27、p1c 6、p3 3、
  sema 72、snapshots 18、runtime_gaps 48 等，全部通过。
- `cargo test -p aura-loom`：lib 477、integration 21 等，全部通过（此前并行时
  有 1~4 项随机失败）。
- Aura 侧：TestRunner 自检 / Phase 0 / Phase 1（4 项差分）/ 快照 8 项 / 构建全部
  PASS，`vm_test.aura` 15/15。

#### `aura-loom` 的两类问题（均已修复）

- **确定性失败** `test_execute_package_enabled`：`project_dir()` 硬编码为 `"."`，
  使 `package` / `compile` / `test` 等任务一律相对**进程 CWD** 解析 `aura.toml`、
  `src/` 等项目内路径。
  - 修复：`ResolvedBuildConfig` 新增 `project_dir` 字段（默认 `"."`）与
    `with_project_dir()` 构造器，`project_dir()` 改为读取该字段；同时补全该用例
    缺失的夹具（`aura.toml`、`src/main.aura`、已编译的 `.auc` 产物），并新增
    `test_execute_package_honors_project_dir` 锁定「错误应指向配置的项目根」。

- **并行 flaky**（`task::scheduler::*`、`task::executor::*`、integration 的
  `test_build_after_source_change` 等）：测试使用 `ResolvedBuildConfig::default()`，
  其 `out_dir` / `cache_dir` 是相对路径（`target/build`、`target/cache`），而
  `cargo test` 的 CWD 是包目录 —— 并行用例共用同一目录并互相删除/覆盖
  （`clean` 任务直接 `remove_dir_all`），失败数随并发度浮动。
  - 修复：新增测试专用 `ResolvedBuildConfig::isolated()`（每个用例独立临时目录），
    并在单元测试中替换 `default()`；integration 侧加等价的 `isolated_config()`。

> 注：`loom/src/plugin/context.rs` 的 `new_default()` 属**非测试**公开 API，
> 保持 `default()` 不变，避免把生产环境的默认输出目录改成临时目录。

## Phase 2 交付物（语义分析 + HIR）✅

| # | 交付物 | 路径 | 状态 |
|---|--------|------|------|
| 2.1 | 类型表示 `Type` | `.../sema/Type.aura` | ✅ |
| 2.2 | 类型信息 `TypeInfo` | `.../sema/TypeInfo.aura` | ✅ |
| 2.3 | 符号表（作用域链） | `.../sema/SymbolTable.aura` | ✅ |
| 2.4 | 类型检查器 | `.../sema/TypeChecker.aura` | ✅ |
| 2.5 | HIR 定义 + AST→HIR 降级 | `.../hir/Hir.aura` | ✅ |
| 2.6 | 语法糖消解 | `.../hir/Desugar.aura` | ✅ |
| 2.7 | 泛型单态化 | `.../hir/Mono.aura` | ✅ |
| 2.8 | 内联优化 | `.../hir/Inline.aura` | ✅ |
| 2.9 | 常量折叠 | `.../hir/Fold.aura` | ✅ |
| 2.10 | Phase 2 验证用例 | `tests/phase2_sema_hir_tests.aura`（+ `phase2_*` 最小用例） | ✅ |

HIR 采用与 AST 一致的「扁平 arena」表示（`kinds/texts/tys/spans/kids`
五条「每行一项」的字符串），以规避当前 VM 对 `List`/`Map` 的若干限制。

## Phase 3 交付物（MIR + 优化）✅

| # | 交付物 | 路径 | 状态 |
|---|--------|------|------|
| 3.1 | MIR 定义（TAC/基本块） | `.../mir/Mir.aura` | ✅ |
| 3.2 | HIR→MIR 降级 | `.../mir/MirLower.aura` | ✅ |
| 3.3 | MIR 优化（DCE/常量传播） | `.../mir/MirOpt.aura` | ✅ |
| 3.4 | Phase 3 验证用例 | `tests/phase3_mir_tests.aura` | ✅ |

## Phase 4 交付物（字节码发射 + VM）✅

| # | 交付物 | 路径 | 状态 |
|---|--------|------|------|
| 4.1 | MIR→字节码发射器 | `.../codegen/Codegen.aura` | ✅ |
| 4.2 | 指令集定义 | `.../vm/Opcodes.aura` | ✅ |
| 4.3 | 栈帧定义 | `.../vm/Frames.aura` | ✅ |
| 4.4 | VM 解释器 | `.../vm/Vm.aura` | ✅（增强版运行器见 Phase 5） |
| 4.5 | Phase 3/4 验证用例 | `tests/phase3_mir_tests.aura` | ✅ |

## Phase 5 交付物（VM 增强：闭包 / 尾调用 / 栈帧）✅

| # | 交付物 | 路径 | 状态 |
|---|--------|------|------|
| 5.1 | VM 指令执行循环 | `.../vm/VmRunner.aura` | ✅ |
| 5.2 | 闭包与上值管理 | `.../vm/Closures.aura` | ✅ |
| 5.3 | 尾调用优化 | `.../vm/TailCall.aura` | ✅ |
| 5.4 | 栈帧管理（分配/释放/溢出） | `.../vm/FrameManager.aura` | ✅ |
| 5.5 | Phase 5 验证用例 | `tests/phase5_vm_tests.aura` | ✅ |

**修复记录**：`VmRunner.pop()` 原实现移除栈顶时只截掉末尾 `\n`，被弹出的值仍残留在
`stack` 字符串中，导致下一次 `push` 拼接出错（如 `42 + 42` 得到 `"4284"`）。
已改为连同该行值一起移除，`phase5_vm_tests.aura` 由 2 项失败 → **全部通过**。

## Phase 6 交付物（AOT LLVM 后端）✅

对应迁移计划 §4.8，将 Rust `compiler/src/codegen/aot/` 上移到
`aura/lang/compiler/aot/`（文本 LLVM IR + 外部 `llc`/`clang` 子进程方案，方案一）。

| # | 任务 | Rust 源 | Aura 目标 | 状态 |
|---|------|---------|-----------|------|
| 6.1 | 类型映射 | `aot/types.rs` | `.../aot/TypeMapper.aura` | ✅ |
| 6.2 | 目标三元组 | `aot/target.rs` | `.../aot/Target.aura` | ✅ |
| 6.3 | Runtime 声明 | `aot/runtime.rs` | `.../aot/Runtime.aura` | ✅ |
| 6.4 | FFI 声明 | `aot/ffi.rs` | `.../aot/Ffi.aura` | ✅ |
| 6.5 | 优化级别 | `aot/optimize.rs` | `.../aot/Optimize.aura` | ✅ |
| 6.6 | LLVM IR 生成 | `aot/emit.rs` | `.../aot/Emit.aura` | ✅ |
| 6.7 | 目标码生成/链接 | `aot/linker.rs` | `.../aot/Linker.aura` | ✅ |
| 6.8 | DWARF 调试信息 | `aot/dwarf.rs` | `.../aot/Dwarf.aura` | ✅ |
| 6.9 | C 后端（备选） | `aot/c_backend.rs` | `.../aot/CBackend.aura` | ✅ |
| 6.10 | 编排器 + 选项 | `aot/mod.rs` | `.../aot/Aot.aura` | ✅ |
| 6.11 | 公共辅助 | — | `.../aot/AotUtil.aura` | ✅ |
| 6.12 | Phase 6 验证用例 | — | `tests/phase6_aot_tests.aura`（14 组 / 141 断言） | ✅ |

**交付内容**：

- **模块头 + 函数定义**：`Emit.aura` 从 HIR 生成 LLVM IR 文本（`target triple` /
  `target datalayout` / 结构体 / 字符串常量 / runtime 声明 / `define ... { entry: ... }`），
  局部变量在 entry 块 `alloca`；覆盖字面量、变量、二元/一元、调用、val|var、
  return、assign、if、while、block。
- **类型/目标/优化/声明**：`TypeMapper`（Int→i32、String→`{ i8*, i64 }`、用户类型→
  `%struct.*`、fnType）、`Target`（三元组构造/解析/扩展名）、`Optimize`（`-O0`…`-Oz`）、
  `Runtime`（10 个 runtime 声明 + 判定/签名）、`Ffi`（声明生成/去重/runtime 跳过/C ABI 覆盖）。
- **后端**：`Linker`（`llc`/`clang`/`lld-link` 命令构造 + 5 级工具探测说明）、
  `CBackend`（HIR→C 备选路径）、`Dwarf`（`DICompileUnit`/`DISubprogram` 元数据）、
  `Aot`（编排器：IR + 命令 + C 源码 → `AotResult`）。

**设计边界**：纯 Aura 侧以「IR 文本生成 + 命令构造」为交付边界（纯函数、可测试）；
实际的 `llc`/`clang` 子进程调用与产物写盘由引导层（`Process.spawn` / FFI）承担。

**验证**：`tests/phase6_aot_tests.aura` 覆盖上述全部模块，14 组用例全部通过
（`RESULT: PASS`）。

**修复记录（Phase 6 开发期间暴露的运行时约束）**：

- 经**字段访问得到的对象/字符串再调用方法**会错乱 `this` 绑定（`r.objectCommand.contains(...)`
  恒假）；需先取局部变量再调用。
- `String` 字面量中的 `$` 会触发插值（`"$AURA_LLVM_HOME"` 被解析为变量引用），
  文档化文本需避免裸 `$`。

> **回退策略**：任何 Phase 6 失败只需删除 `aura/lang/compiler/aot/`，Rust 编译器
> 的 AOT 后端（`compiler/src/codegen/aot/`）不受影响，始终可用。

## Phase 7 交付物（JIT / Cranelift 原生码）✅

对应迁移计划 §4.9：补全「VM 解释器 / JIT 即时编译 / AOT 静态编译」三执行路径中的
**JIT**，把 Rust 侧 `compiler/src/vm/{jit,jit_opt,abi,aot_runtime}.rs` 与
`compiler/src/bootstrap/jit_core.rs` 迁移到 `aura/lang/compiler/jit/`。

| # | 任务 | Rust 源 | Aura 目标 | 状态 |
|---|------|---------|-----------|------|
| 7.1 | 热点/白名单/递归可达状态机 | `vm/jit.rs` + `vm/mod.rs` | `.../jit/JitState.aura` | ✅ |
| 7.2 | 共享 ABI（`JitValue` / `AotEntry`） | `vm/abi.rs` | `.../jit/JitAbi.aura` | ✅ |
| 7.3 | 字节码 → Cranelift IR 文本发射 | `vm/jit.rs`（cranelift_backend） | `.../jit/JitLower.aura` | ✅ |
| 7.4 | 7 个优化传递 | `vm/jit_opt.rs` | `.../jit/JitOpt.aura` | ✅ |
| 7.5 | 原生派发 + 解释器回退 | `vm/mod.rs` | `.../jit/JitDispatch.aura` | ✅ |
| 7.6 | W^X / 描述符表 / Blob 段加载 | `vm/aot_runtime.rs` | `.../jit/JitRuntime.aura` | ✅ |
| 7.7 | 最小引导 JIT | `bootstrap/jit_core.rs` | `.../jit/JitCore.aura` | ✅ |
| 7.8 | Phase 7 验证用例 + 差分 | — | `tests/phase7_jit_tests.aura`（12 组 / 177 断言） | ✅ |
| — | 公共辅助（扁平字符串工具/函数表） | — | `.../jit/JitUtil.aura` | ✅ |

**必须继承的历史结论（`docs/JIT性能分析.md`）**：旧 JIT「与解释器同速」的四点根因
（内联删调用点、入口不计数、递归被白名单拒绝、循环热点无调用计数）。本 Phase 等价
继承两条修复：

- **Fix A（入口强制编译）**：`jitForceEntry()` 忽略阈值编译入口函数 → `run()` 直接
  派发原生入口，循环热点走原生码；
- **Fix B（递归支持）**：白名单纳入 `Call`，`jitCompileOrder()` 沿调用图**后序**
  编译被调用者（保证 `dispatch_table` 条目已存在），降级用 `call_indirect fnN(...)`。

**7 个优化传递**（顺序与 Rust 一致）：常量折叠 → 死码消除 → 跳转线程化 → 强度削弱
（`/2^n`→`sshr`、`%2^n`→`band`、`*2^n`→`ishl`）→ 指令调度 → 函数内联（候选判定）→
循环展开（2x）。

**设计边界**：纯 Aura 侧交付「IR 文本 + 映射表 + 派发/回退决策」（纯函数、可测试）；
真正的 Cranelift 代码生成、`mmap` W^X 装载与原生调用由引导层（FFI）承担。
`JitRuntime` 复用 `.auc v4` 段格式（`SEG_MACHINE` / `SEG_DESC_TABLE`）、`AuraFuncDesc`
（32 字节）与 `JitValue` ABI，使 JIT 原生码与 AOT Blob 走同一装载路径。

**与 Rust 基线的有意差异**（语义等价且更严格，详见 `JitOpt.aura` 文件头）：

- 常量折叠改为单遍前瞻，不再「折叠任意三个连续 LoadConst」；
- 强度削弱修正了操作数顺序（Rust 以 `LoadConst shift` 在前会使 `Shl` 移位方向相反）
  并去掉重复压栈；
- 函数内联与 Rust 基线一致保持占位，仅额外暴露候选判定。

**验证**：`tests/phase7_jit_tests.aura` 覆盖 ABI / 状态机 / 调用图 / 7 传递 / 派发回退 /
解释器 / 优化差分 / 段与 W^X / 引导层，12 组 177 断言全部通过（`RESULT: PASS`）。
其中 `JIT.differential` 用内置解释器对同一程序「优化前 vs 优化后」逐例比对，
锁定优化不改变语义。

**验证标准对照（迁移计划 §4.9）**：

| 标准 | Aura 侧证据 |
|------|------------|
| 热点可达（Fix A/B） | `JIT.dispatch`：`jitForceEntry` 强制编译入口；`JIT.state`：`Call` 在列白名单 |
| 优化等价（7 传递） | `JIT.opt.fold/passes/pipeline`：逐传递断言；两处有意偏差见上文 |
| 执行一致（优化差分） | `JIT.differential`：优化前后解释结果逐例一致 |
| 回退正确 | `JIT.dispatch`：不可编译函数记 skip 且 `jitFallbackReason` 可查 |
| 递归/互递归 | `JIT.callgraph`：后序编译顺序 + 自递归；`JIT.lower`：`call_indirect fnN(...)` |
| ABI 兼容（`.auc v4`） | `JIT.runtime`：段校验 / 描述符 32B / 分发表重建（入口 16 对齐）/ W^X |

> **边界说明**：真实机器码生成、`mmap` W^X 执行与「VM/JIT/AOT 三路真机差分」「性能 ≥50x」
> 需由引导层（Cranelift FFI）承担，纯 Aura 侧以「IR 文本 + 状态机 + 段装载模型」交付；
> 这与 Phase 6「IR 文本 + 命令构造」的交付边界一致。

> **回退策略**：任何 Phase 7 失败只需删除 `aura/lang/compiler/jit/`，Rust 编译器的
> JIT（`compiler/src/vm/jit*.rs`）与其余后端不受影响，始终可用。

## Phase 8 交付物（核心库与标准库 Aura 化）✅

对应迁移计划 §4.10：`aura/core/aura/lang/**` 是核心类型 / 标准库 / 集合的
Aura 源码实现，目标是使其成为唯一真相源（替换 `compiler/src/std/*.rs` 的
纯逻辑部分）。Phase 8 首先补齐「源码可编译 + 逻辑正确 + 可验证」三项前提。

### 8.1 编译器缺口修复（暴露自 Aura 核心库）

| # | 缺口 | 现象 | 修复位置 |
|---|------|------|----------|
| 1 | 接口继承无法解析 | `interface List<T> : Collection<T>` 报 `parse error: Expected Arrow, got Colon`，导致 `collection/{List,Array,Set}.aura` 无法编译 | `ast::InterfaceDecl.super_types` + `parser::parse_interface`（复用 `parse_superclass_ref`，支持多父接口 `: A, B` 与泛型实参跳过）+ `sema::checker` 登记继承链 |
| 2 | 字符串转义缺失 | `Json.aura` 中的 `"\b"` / `"\f"` 报 `lex error: Invalid escape sequence` | `lexer` 字符串/字符字面量新增 `\b` `\f` `\v` `\/` 转义 |

**效果**：`aura stdlib-compile aura/core/aura/lang` 由 **41/45 → 45/45** 全部编译成功。

### 8.2 Aura 标准库实现修复

| 文件 | 缺陷 | 修复 |
|------|------|------|
| `collection/ArrayList.aura` | `filter`/`map` 忽略回调、`every`/`any` 恒 `true` | 改为 `(T) -> Boolean` / `(T) -> Any` 函数类型并按元素调用；新增 `size` 属性 |
| `collection/List.aura` | 接口回调参数为 `Any`，无法调用 | 同步为函数类型签名 |
| `collection/HashSet.aura` | `remove` 调用不存在的 `ArrayList.removeItem` | 改用 `indexOf` + `remove(index)` |
| `Float.aura` / `Double.aura` | `ceil`/`floor`/`trunc` 占位 `return this`；`round` 调用不存在的全局 `floor` | 以 `as Int` 截断实现纯 Aura 取整 |
| `std/TestHelper.aura` | `s.length()` 与 `String.length` 属性不符；缺 `return` | 改为 `s.length` + 显式 `return` |

### 8.3 Phase 8 验证用例

`tests/phase8_stdlib_tests.aura`：通过**相对路径内联** Aura 标准库源码，直接验证
Aura 实现（而非 Rust native 回退），覆盖：

| 组 | 覆盖 | 断言数 |
|----|------|--------|
| `std.math` | abs / square / cube / sign / clamp / powInt / floor / ceil / trunc / round / lerp / mapRange | 15 |
| `std.ascii` | isAlpha / isDigit / isAlphaNumeric / isWhitespace / isUpper / isLower / toUpper / toLower / upperCaseAll / lowerCaseAll / allAlpha / allDigit | 16 |
| `std.assert` | Assert.assertApprox + TestHelper（add / multiply / isEven / reverse） | 6 |
| `std.iter` | sum / avg / product / contains / indexOf / lastIndexOf / count / none | 9 |

合计 **46 断言**，`RESULT: PASS`。

### 8.4 已知限制（Phase 8 遗留，属编译器/VM 层）

以下问题经 Phase 8 验证用例定位，**不是 Aura 源码错误**，需后续在 `compiler/`
侧解决；在此之前相关模块暂不能作为运行时真相源：

- **native 同名回退拦截**：HIR 会把 `Math.min/max`、`Iter.min/max`、`String.toInt`
  等调用解析到**同名 native prelude**（如 `min(Int,Int)`、`toInt(Any)`），
  使 Aura 实现（含重载）被遮蔽，返回错值（`min(3,7) == 0`、`"123".toInt() == 0`）。
- **`Array<T>` 下标未降级**：`Array<T>` / `ArrayList<T>` 的 `data[i]` 无法索引
  （`cannot index into 'Array'`），导致 `ArrayList` / `HashMap` / `HashSet`
  的运行时行为不可靠。
- **函数类型参数与容器构建**：`Iter` 中带 `(Any)->*` 回调的函数
  （`countWhere`/`every`/`some`）与内部 `mutableListOf()` 构建后返回的
  （`map`/`filter`/`reverse`/`take`/`range`）在 VM 中丢失结果。
- **`String.charCodeAt` / `String.fromCharCode`**：返回异常值，
  影响 `Ascii.codeAt` / `fromCode` 与 `Encoding` 的实现。

> **回退策略**：任何 Phase 8 失败只需删除新增/修改的 Aura 文件即可；
> Rust 编译器的 native 标准库实现始终可用，不受影响。

### 8.5 验证命令

```bash
aura run tests/phase8_stdlib_tests.aura     # 应输出 RESULT: PASS
aura stdlib-compile aura/core/aura/lang --output build   # 应 45/45 成功
```

## Phase 9 交付物（整体编译管线串联）✅

把散落的各阶段模块串成一条**可运行**的完整编译管线，让 Aura 编译器真正
「编译并执行 Aura 程序」，并补齐 **VM / JIT / AOT 三执行路径**的编译链路：

```
                        ┌────────────────────────────────────────────────┐
Lexer → Parser → AST → Sema → HIR → MIR ──┬─→ Codegen(字节码) → VM 解释执行
                                           ├─→ JIT 适配 → JitCore 单元执行（叶子函数 + 去优化回退）
                                           └─→ Emit(LLVM IR) → llc → clang → 原生 exe（运行）
```

### 9.1 交付物

| # | 交付物 | 路径 | 说明 |
|---|--------|------|------|
| 9.1 | 编译管线（VM/AOT/JIT 源级入口） | `.../compiler/Main.aura` | 管线由入口文件直接拥有（**无中间驱动层**）：`compile` / `compileAndRun`（VM）、`compileAotSource`（IR+命令）、`aotBuildExeSource`（IR→`llc`→`clang`→exe）、`jitCompileFunction`（JIT）、`compileWith`，另含 5 组自检样例 |
| 9.2 | 后端模块（只收本阶段输入） | `aot/Aot.aura`、`jit/JitCore.aura` | AOT：`compileAot(hir)`、`aotBuildExeFromHir`、`runBuiltExe`、`aotWinPath`、`llvmHomeDefault`；JIT：`JitLinkResult`、`tryCompileInModule`（JIT 自行定位函数）。后端**不反向依赖前端**，源码→阶段输入的串联由入口完成 |
| 9.3 | Phase 9 验证用例 | `tests/phase9_compiler_tests.aura` | 9 组 / 35 断言（前端 / 算术 / 变量 / 控制流 / 函数 / 错误 / AOT / JIT / **AOT exe**），`RESULT: PASS` |
| 9.4 | 原生进程执行 | `compiler/src/std/std_process.rs`（`Process.run`） | 新增同步 shell 执行原生函数（Windows `cmd /C`、Unix `sh -c`），供驱动调用 LLVM 工具链 |

### 9.2 串联期间修复的契约缺口

各阶段此前各自「能跑单测」，但阶段之间的数据契约不一致，串起来即失败。本轮修复：

| 层 | 缺口 | 修复 |
|----|------|------|
| HIR | `Parser` 的 `Binary.text` 存的是 **TokenKind 名**（`Plus`/`Star`），MIR 按符号匹配 → 所有二元运算退化为 `ADD` | 新增 `hirNormBinOp` / `hirNormUnOp` 归一化为 `+`/`*`/`==`… |
| HIR | `lowerCall` 把被调用者也放进 `kids[0]` 且 `text` 留空 → 函数名丢失 | 函数名写入 `HirCall.text`，`kids` 只保留实参 |
| HIR | `lowerIfStmt` 按 `Expr/Block/If` 过滤子节点 → **条件被丢弃** | 无条件降级条件表达式（任意 kind） |
| HIR | `lowerAssign` 未写目标名 → `STORE_VAR` 落到无名槽位，循环变量永不更新 | 目标标识符名写入 `HirAssign.text` |
| MIR | `lowerFunction`/`lowerCall` 期待 `HirParams`/`HirArgs` 包装节点（HIR 从不生成） | 改为读取直接子节点 `HirParam`/实参；`CALL` 节点携带实参个数 |
| MIR | `if`/`while` 的 `JUMP` 无目标 | 引入 `LABEL` 伪指令 + 唯一标签名，控制流结构化为标签跳转 |
| Codegen | 常量索引 = `constPool.length`（**字符串长度**）、槽位 = `varTable.length` → 索引错乱 | 改为按「条数」分配；两趟发射：第 1 趟记录 `LABEL→行号`，第 2 趟解析跳转目标 |
| Codegen | 每函数参数/局部共享全局槽位 | 每个 `MirFunc` 重置槽位命名空间并预分配参数槽 0..n-1 |
| VM | `setLocal` 追加而非覆盖 → 循环变量读到陈旧值（死循环） | 覆盖式写入 + 按槽位查找 |
| VM | `CALL` 不跳入函数体、`RETURN` 直接终止 | 调用栈（返回地址 + 保存局部变量块）、参数绑定、返回后把返回值压回调用者栈 |
| VM | 缺少 `MOD/NEQ/LT/GT/LE/GE/NOT/NEG`、`JUMP` 目标、内置 `println/print` | 补齐算术/比较/跳转/内置调用 |
| Lexer | 不支持 `\uXXXX` 转义 | 字符串/字符字面量新增 `\uXXXX` 解析 |
| JIT | `JitCore` 使用**独立引导指令集**，需在驱动层做字节码翻译（中间驱动） | `JitCore` 改为直接消费 **VM 字节码**（`CONST_INT`/`JUMP`/`RETURN`…），删除转换层与 `Const`/`Br`/`CallFfi` 旧词汇；`tryCompileInModule` 由 JIT 自行定位函数体，驱动层不再接触字节码结构 |
| VM（原生） | 对象单例方法调用注入 `self` 为首参（`argc = 声明参数数 + 1`），使 `FileSystem.writeText(path,content)` 等原生整体错位 | `interp.rs::do_call_native_args`：当 `argc == param_count + 1` 且 `param_count >= 1` 时剥离注入的 `self`（变长原生 `param_count==0` 不受影响） |

### 9.3 三执行后端（VM / JIT / AOT）

| 后端 | 入口 | 产物 / 执行 | 说明 |
|------|------|-------------|------|
| **VM** | `Main.aura`：`compile(source)` / `compileAndRun(source)` | 字节码字符串 + Aura `VmRunner` 解释执行 | 完整管线，见 9.2 修复清单 |
| **JIT** | `Main.aura`：`jitCompileFunction(source, fn, args)`（后端入口 `JitCore.tryCompileInModule`） | **与 VM 同一字节码** → `JitCoreCompiler.tryCompileInModule` 定位函数 + 预解码（常量池索引→字面量、分支绝对行号→单元相对偏移）→ `JitCoreVm` 执行 | 仅**叶子函数**（无 `CALL`）可编译；`CALL`/`CALL_METHOD`/字段访问等 → `deopt` 回退解释器 |
| **AOT** | `Main.aura`：`compileAotSource(source)` / `aotBuildExeSource(source, module, outDir, llvmHome)`（后端入口 `Aot.compileAot(hir)` / `aotBuildExeFromHir`） | HIR → LLVM IR 文本 + `llc`/`clang` 命令 + C 后端源码；`aotBuildExeFromHir` 进一步落盘 IR、调用 `llc`→`clang` 产出 **原生 exe**，可 `runBuiltExe` 运行 | `compileAot` 为纯函数；`aotBuildExeFromHir` 经 `FileSystem.writeText` + `Process.run` 真正调用工具链 |

> **单一字节码（不再维护两套）**：`JitCore` 直接消费 `Codegen.emit` 的字节码
> （`CONST_INT`/`LOAD_LOCAL`/`STORE_LOCAL`/`JUMP`/`JUMP_IF_FALSE`/…/`RETURN`；常量用池索引、
> 分支用绝对行号），**删除**了此前的独立引导指令集（`Const`/`LoadLocal`/`Br`/`CallFfi`/`Ret`）。
> 预解码只在 `JitCore` 内部做两件必要规范化：常量池索引→字面量、分支绝对行号→单元相对偏移。
> 模块级入口 `JitCoreCompiler.tryCompileInModule(func, name, bytecode, funcTable, consts)` 由
> `JitCore` **自行完成函数定位**（按 `funcTable` 切分、跳过 `# func` 注释行、统计槽位），
> 入口层（`Main.aura` 的 `jitCompileFunction`）**不接触任何字节码结构**，仅「跑管线 → 交给 JIT → 取结果」。
> 注：`JitLower`/`JitState`/`JitDispatch` 是 Phase 7 对 Rust `vm::jit` 的镜像（面向 Rust VM 字节码名
> 与 ARC 指令），不属于本编译管线的字节码，未纳入本次统一。

### 9.4 验证命令

```bash
aura run tests/phase9_compiler_tests.aura        # 应输出 RESULT: PASS
aura run aura/compiler/aura/lang/compiler/Main.aura   # 打印 VM/AOT/JIT 三条链路的摘要
```

**VM 样例执行结果**（由 Aura 编译器自行编译并解释执行）：

| 样例 | 源码语义 | 结果 |
|------|----------|------|
| 算术 | `2 + 3 * 4` | `14` |
| while | `0+1+2+3+4` | `10` |
| if/else | `7 > 10 ? 1 : 2` | `2` |
| 函数调用 | `add(2, 3)` | `5` |
| 递归 | `fact(5)` | `120` |

**JIT 链路结果**：

| 目标函数 | 形态 | 结果 |
|----------|------|------|
| `square(x) = x*x` | 叶子函数 | `compiled=true, deopt=false, result=25` |
| `main`（含 `CALL square`） | 非叶子 | `compiled=false, deopt=true`（回退解释器） |
| `rem(a,b) = a % b` | 含 `MOD` | `deopt=true`（不支持指令） |

**AOT 链路产物**（`aotSrc = fun main(): Int { return 2 + 3 * 4 }`）：

```llvm
; ModuleID = 'main'
source_filename = "main"
target triple = "x86_64-pc-windows-msvc"
target datalayout = "..."

; ---- Aura Runtime Declarations ----
declare void @aura_arc_increment(i8*)
; … 10 条 runtime 声明 …

define i32 @main() {
entry:
  %var.0 = mul i32 3, 4
  %var.1 = add i32 2, %var.0
  ret i32 %var.1
}
```

命令构造：`llc -mtriple x86_64-pc-windows-msvc main.ll -o main.obj -O2 -filetype=obj`
→ `clang -target x86_64-pc-windows-msvc main.obj -o main.exe -O2`。

### 9.5 边界与后续

- **执行方式**：VM 路径使用 **Aura 编写的 `VmRunner`（字符串字节码解释器）**；
  JIT 路径使用 **Aura 编写的 `JitCoreVm`**（引导指令集解释执行，非真实机器码）；
  AOT 路径仅产出 **LLVM IR 文本 + 命令**。`.auc` 二进制序列化、真实机器码/JIT、以及
  Aura 侧调用 `llc/clang` 链接成 exe 仍是后续工作。
- **覆盖范围**：标量类型、算术/比较、`val/var`、赋值、`if/else`、`while`、
  函数（含递归/互递归）、`println/print`。尚未覆盖：`for`、`when`、字符串、
  列表/集合、类/结构体、闭包、异常。
- **AOT 说明**：原生 `aura.exe` 输出需 `cargo build -p cli --features llvm` 且本机
  安装 LLVM（见 `Cargo.toml` 的 `llvm-home`）；Aura 侧的 `aot/Emit.aura` 目前只
  产出 LLVM IR 文本 + 命令构造，真正的 `llc/clang` 子进程调用与链接尚未接入。

## Phase 0 交付物

| # | 交付物 | 路径 |
|---|--------|------|
| 0.1 | 独立目录结构 | `aura/compiler/` |
| 0.2 | 包结构定义 | `aura/compiler/aura/lang/compiler/` |
| 0.3 | Aura 测试框架 | `aura/compiler/aura/lang/compiler/test/TestRunner.aura`（框架）<br>`.../test/TestRunnerSelfTest.aura`（可运行自检入口） |
| 0.4 | 构建脚本 | `scripts/build-aura-compiler.sh` / `scripts/build-aura-compiler.ps1` |
| 0.5 | 源码快照对比机制 | `tests/snapshots/` + `scripts/snapshot-*.{sh,ps1}` |
| 0.6 | CI 流水线 | `.github/workflows/ci.yml`（`aura-bootstrap` job） |
| 0.7 | Phase 0 验证用例 | `tests/phase0_tests.aura` |

## 快速验证

```bash
# 1) 测试框架自检（应输出 RESULT: PASS）
aura run aura/compiler/aura/lang/compiler/test/TestRunnerSelfTest.aura

# 2) Phase 0 验证用例
aura run tests/phase0_tests.aura

# 3) Phase 1 验证用例（Lexer / Token / Span）
aura run tests/phase1_lexer_tests.aura

# 4) Phase 2 验证用例（Sema / SymbolTable / Type / HIR）
aura run tests/phase2_sema_hir_tests.aura

# 5) Phase 3/4 验证用例（MIR / 优化 / 字节码发射）
aura run tests/phase3_mir_tests.aura

# 6) Phase 5 验证用例（闭包 / 尾调用 / 栈帧 / VM 运行器）
aura run tests/phase5_vm_tests.aura

# 7) Phase 6 验证用例（AOT LLVM 后端：类型映射 / 目标三元组 / IR 发射）
aura run tests/phase6_aot_tests.aura

# 8) Phase 7 验证用例（JIT：状态机 / 7 优化传递 / Cranelift IR / 派发回退 / 段加载）
aura run tests/phase7_jit_tests.aura

# 9) Phase 8 验证用例（核心库 / 标准库 Aura 化）
aura run tests/phase8_stdlib_tests.aura

# 10) Phase 9 验证用例（整体编译管线串联）
aura run tests/phase9_compiler_tests.aura
aura run aura/compiler/aura/lang/compiler/Main.aura

# 11) 构建 Aura 编译器 → 产物集中输出到 build/bin/
#     build/bin/aura.exe            Rust 最小 bootstrap 编译器（-Aot/--aot 时以
#                                   `cargo build -p cli --features llvm` 重建）
#     build/bin/aura-compiler.auc   Aura 编写的编译器（字节码，可 `aura run`）
#     build/bin/aura-compiler.exe   Aura 编写的编译器（AOT 原生 exe；⚠ 见下方已知限制）
#     注意：--aot 需要本机安装 LLVM
#     （路径见根 Cargo.toml 的 [workspace.metadata.aura].llvm-home）
scripts/build-aura-compiler.sh
scripts/build-aura-compiler.sh --aot

# 12) 源码快照一致性检查
scripts/snapshot.sh
```

Windows（PowerShell）：

```powershell
aura run aura\compiler\aura\lang\compiler\test\TestRunnerSelfTest.aura
aura run tests\phase0_tests.aura
aura run tests\phase1_lexer_tests.aura
aura run tests\phase2_sema_hir_tests.aura
aura run tests\phase3_mir_tests.aura
aura run tests\phase5_vm_tests.aura
aura run tests\phase6_aot_tests.aura
aura run tests\phase7_jit_tests.aura
aura run tests\phase8_stdlib_tests.aura
aura run tests\phase9_compiler_tests.aura
aura run aura\compiler\aura\lang\compiler\Main.aura
scripts\build-aura-compiler.ps1
scripts\snapshot.ps1
```

> **build/bin 产物与 AOT 现状**
>
> * `build/bin/aura.exe`（bootstrap）与 `build/bin/aura-compiler.auc` 均已可用：
>   `aura run build/bin/aura-compiler.auc` 会跑通「Aura 编译器自检样例」。
> * `build/bin/aura-compiler.exe`（把 Aura 编写的编译器 AOT 成原生 exe）**尚未打通**。
>
>   已修复的阻塞项（Rust AOT 后端）：
>   1. `cmd_build_aot` 曾把 P3 语义诊断当致命错误直接 `exit(1)`，现与字节码路径
>      `compile_source` 一致：只告警、不阻断（`Main.aura` 有 468 条 P3 诊断）。
>   2. `emit_index_access` 曾生成非法 GEP（i32 索引直塞 i64 槽、`i8*` 基址当
>      `i32*`），已改为 sext + bitcast，并按容器表示分派：
>      字符串 → `aura_lang_std_String_charAt`；动态列表 → `..._Collections_getAt`；
>      整型数组 → GEP。
>   3. AOT 调用点符号层缺失：发射器把 `aura.lang.std.String.split` 落成 LLVM 符号
>      `aura_lang_std_String_split`，而 C 运行库只导出 `aura_string_*` 旧名，
>      导致**除 prelude 外所有 std 调用链接期 undefined symbol**。
>      现已在 `compiler/src/std/cffi/aura_std_cffi.{c,h}` 增加调用点符号实现
>      （String / Collections / FileSystem / Process / Math）。
>   4. 字符串比较曾生成 `icmp i32 …, { i8*, i64 } …` 等非法 IR，现统一走
>      `aura_lang_std_String_equals` 做内容比较；整型 i32/i64 混用先统一。
>   5. 字符串 `.length` / `.size` 曾被当作结构体字段 0（数据指针），现取字段 1。
>
>   现已可端到端验证（编译 → llc → clang → 运行）：
>   `tests/aot/string_runtime.aura`、`tests/aot/list_runtime.aura`
>   （`aura build <file> --aot -o x.exe && x.exe`，退出码 0）。
>
>   剩余根因：AOT 依赖 sema 类型信息，而 `compiler/src/std/decl.rs` 只登记
>   **std 函数名**、没有签名表，因此 `String.split` 等 native 调用在 sema 中退化为
>   `Any`；再叠加「类对象 = 指针」与「值返回 = 结构体」两种表示混用
>   （如 `Token.withSpan` 声明返回 `%struct.Token` 却 `ret i8*`），
>   Aura 编译器自身仍会在这些点产出非法 IR。
>   需先补「std 签名表 + 统一的值/指针表示」，Aura 编译器才可能 AOT 成功。
>
> * **两套 AOT 发射器（重要）**：`aura build --aot` 走 **Rust** 后端
>   （`compiler/src/codegen/aot/emit.rs`，本页上述修复所在）；
>   而 `aura/compiler/.../aot/Emit.aura` 是 Phase 6 的 **Aura 侧镜像实现**，
>   能力弱得多（字符串字面量会发射成 `store i32 Hello:World`），
>   `tests/phase9_compiler_tests.aura::testAotExe` 只覆盖 `6*7` 这类无运行库依赖的程序。
>   要产出 `aura-compiler.exe`，短期内应以 Rust 后端为准。

> **门禁判据**：Aura 侧测试以 stdout 末行 `RESULT: PASS` 为准（当前 VM 无法通过
> `throw` / `exit` 影响进程退出码，CI 用 `grep "RESULT: PASS"` 判定）。

## 构建产物

所有构建产物统一输出到仓库根的 `build/` 目录（不污染源码树）。
