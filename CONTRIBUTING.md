# 贡献指南

感谢你参与 Aura 语言的开发！本指南说明代码规范、开发流程与测试要求。

## 开发环境

- Rust stable（edition 2024，见 `Cargo.toml`）
- 推荐组件：`rustfmt`、`clippy`、`cargo-insta`（快照审查）、`cargo-llvm-cov`（覆盖率）

```bash
rustup component add rustfmt clippy
cargo install cargo-insta cargo-llvm-cov
```

## 常用命令

```bash
cargo build --workspace            # 构建
cargo test --workspace             # 全量测试
cargo fmt --all                    # 格式化
cargo clippy --workspace --all-targets -- -D warnings   # 静态检查（CI 以此为门禁）
cargo test --release --test perf_lexer -- --nocapture   # 词法吞吐率
```

CI（`.github/workflows/ci.yml`）在 Linux / Windows / macOS 上执行 `fmt --check`、`clippy -D warnings`、`build`、`test`（debug + release）与覆盖率采集。**提交前请确保本地上述命令全绿。**

## 代码规范

1. **格式化**：统一使用 `cargo fmt`（不要手工调整缩进）。
2. **Lint**：`clippy` 不允许 warning；确需豁免时用 `#[allow(...)]` 并写明原因。
3. **命名**：类型 / 枚举用 `UpperCamelCase`，函数 / 变量用 `snake_case`，常量用 `SCREAMING_SNAKE_CASE`。
4. **注释**：公共 API 使用 `///` 文档注释说明“做什么”与“为什么”，避免复述代码。代码注释可使用中文或英文，团队内统一风格即可。
5. **错误信息**：所有诊断必须携带 `Span`，消息面向用户（说明期望与实际），**统一使用英文**，例如
   `type mismatch: cannot initialize 'Int' with 'String'`。运行时错误（`Err(String)`）、日志输出、CLI 提示信息同样使用英文。
6. **模块职责**：
   - `lexer.rs` 只做字符 → Token，不构造 AST
   - `parser.rs` 只做 Token → AST，不做类型判断
   - `sema/` 负责类型推断、空安全与诊断

## 测试要求

每个改动都应附带测试，按层次选择：

| 层次 | 位置 | 说明 |
|------|------|------|
| 单元测试 | 各模块内 `#[cfg(test)] mod tests` | 词法 / 语法 / 源码映射等细粒度行为 |
| 集成测试 | `compiler/tests/sema_tests.rs` | 语义检查：正确程序无诊断，错误程序报预期诊断 |
| 快照测试 | `compiler/tests/snapshots.rs` | Token 流 / AST / 诊断输出的整体形态 |
| 性能基准 | `compiler/tests/perf_lexer.rs` | 扫描吞吐率（release 下 > 10 MB/s） |

### 快照测试工作流

```bash
INSTA_UPDATE=always cargo test      # 生成 / 更新 .snap
cargo insta review                  # 逐条审查差异并接受
```

`compiler/tests/snapshots/*.snap` **必须提交**；CI 会以只读方式校验。AST 结构发生有意变更时，请连同快照一起更新，并在 PR 描述中说明。

## 提交流程

1. 从 `master` 切分支：`feat/xxx`、`fix/xxx`、`docs/xxx`
2. 小步提交，提交信息说明“改了什么 + 为什么”
3. 提交前运行完整检查（fmt + clippy + test）
4. 更新 `开发规划与实现进度.md`：把完成任务由 `⬜` 改为 `✅`，并同步“第六部分：当前进度汇总”
5. 发起 PR，描述变更范围、测试情况与对既有快照的影响

## 语言特性实现须知

- 新增语法：先改 `token.rs`（如需新 Token）→ `lexer.rs` → `ast.rs` → `parser.rs` → `sema/checker.rs`，并补齐单元测试 + 快照。
- 语法设计以 `技术方案.md` 为准；与 Kotlin 冲突时优先对齐 Kotlin（项目目标为 100% Kotlin 语法兼容）。
- 语义分析（智能转换、可见性、泛型约束、when 穷举、sealed/接口继承、重载解析等）已基本完成；新增语义检查请先在 `开发规划与实现进度.md` 的 P3 清单中认领，并在 `compiler/tests/sema_tests.rs` 与 `tests/snapshots.rs` 补充测试。

## 行为准则

- 讨论聚焦技术本身，review 对事不对人
- 不确定的设计先在 issue 中讨论，再动手实现
- 性能敏感路径（lexer 热循环、类型比较）避免不必要的分配
