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
    ├── lexer/                             # Phase 1  Lexer.aura
    ├── parser/                            # Phase 1  Parser.aura
    ├── ast/                               # Phase 1  Ast.aura
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

# 3) 构建 Aura 编译器骨架（默认产出 .auc；--aot 产出原生可执行文件）
scripts/build-aura-compiler.sh
scripts/build-aura-compiler.sh --aot

# 4) 源码快照一致性检查
scripts/snapshot.sh
```

Windows（PowerShell）：

```powershell
aura run aura\compiler\aura\lang\compiler\test\TestRunnerSelfTest.aura
aura run tests\phase0_tests.aura
scripts\build-aura-compiler.ps1
scripts\snapshot.ps1
```

> **门禁判据**：Aura 侧测试以 stdout 末行 `RESULT: PASS` 为准（当前 VM 无法通过
> `throw` / `exit` 影响进程退出码，CI 用 `grep "RESULT: PASS"` 判定）。

## 构建产物

所有构建产物统一输出到仓库根的 `build/` 目录（不污染源码树）。
