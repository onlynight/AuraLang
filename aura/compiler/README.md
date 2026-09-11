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
    ├── Main.aura                          # 编译器入口（Phase 0 骨架）
    ├── lexer/
    │   ├── Span.aura                      # Phase 1 ✅ 源码位置
    │   ├── Token.aura                     # Phase 1 ✅ Token 数据模型 + 关键字表
    │   └── Lexer.aura                     # Phase 1 ✅ 词法分析器
    ├── parser/                            # Phase 1 ✅ Parser.aura
    ├── ast/                               # Phase 1 ✅ Ast.aura
    ├── sema/                              # Phase 2  TypeChecker.aura
    ├── hir/                               # Phase 2  Hir.aura
    ├── mir/                               # Phase 3  Mir.aura
    ├── codegen/                           # Phase 4  Emit.aura
    ├── vm/                                # Phase 4/5 Vm.aura / Opcodes.aura / Frames.aura
    ├── aot/                               # Phase 6  AOT LLVM 后端
    ├── gc/ memory/ runtime/               # 已有 VM 运行时雏形
    └── test/
        └── TestRunner.aura                # Phase 0  Aura 测试框架
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

# 4) 构建 Aura 编译器骨架（默认产出 .auc；--aot 产出原生可执行文件）
scripts/build-aura-compiler.sh
scripts/build-aura-compiler.sh --aot

# 5) 源码快照一致性检查
scripts/snapshot.sh
```

Windows（PowerShell）：

```powershell
aura run aura\compiler\aura\lang\compiler\test\TestRunnerSelfTest.aura
aura run tests\phase0_tests.aura
aura run tests\phase1_lexer_tests.aura
scripts\build-aura-compiler.ps1
scripts\snapshot.ps1
```

> **门禁判据**：Aura 侧测试以 stdout 末行 `RESULT: PASS` 为准（当前 VM 无法通过
> `throw` / `exit` 影响进程退出码，CI 用 `grep "RESULT: PASS"` 判定）。

## 构建产物

所有构建产物统一输出到仓库根的 `build/` 目录（不污染源码树）。
