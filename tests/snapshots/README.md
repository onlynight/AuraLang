# 源码快照对比机制

> 纯 Aura 化迁移 · Phase 0 交付物 0.5

用于验证「Aura 编写的编译器」与「Rust 参考编译器」在**词法 / 语法**层面输出一致。

## 结构

```
tests/snapshots/
├── cases/                     # 输入用例（.aura）
│   ├── lexer_control_flow.aura
│   ├── lexer_declaration.aura
│   └── parser_generic_function.aura
└── baseline/                  # 基线（Rust 编译器输出，已提交）
    ├── <case>.tokens.txt      # `aura tokens <case>` 输出
    └── <case>.ast.txt         # `aura ast <case>` 输出
```

## 用法

| 目的 | 命令 |
|------|------|
| 重新生成基线 | `scripts/snapshot.ps1 -Update` / `scripts/snapshot.sh --update` |
| 与基线对比 | `scripts/snapshot.ps1` / `scripts/snapshot.sh` |
| 用 Aura 编译器对比 | `scripts/snapshot.ps1 -Compiler .\build\aura-compiler.exe` |

对比失败时脚本退出码为 `1`，并打印前 5 处差异行，可直接用于 CI 门禁。

## 约定

- **基线由 Rust 编译器生成**：Phase 0 的基线固化 Rust 参考实现的行为；
  Phase 1 起用 `--compiler` 指向 Aura 编译器，与同一份基线逐字节比对。
- **行尾规范化**：所有输出统一为 `LF`，并去掉结尾空行，保证 Windows / Linux 基线一致。
- **基线文件为 UTF-8 无 BOM**。
- 新增用例：在 `cases/` 放置 `.aura` 文件后运行 `--update`，并将生成的基线一并提交。
