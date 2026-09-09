# Aura × DSH 集成方案（v4.0）

> **核心发现**：`D:\Code\AuraLang\` 已包含完整 Aura 源码（编译器、标准库、LSP、示例）。
> **核心目标**：让 DSH 作为 AI 编码助手，能直接生成正确的 Aura 代码，实现 vibe coding。
>
> **v4.0 更新**：修复 SKILL.md 三大问题 — 加入行为指令、修正 import 语法、增加验证流程。
> 核心改动：模型不再创建"探查语法"任务，改为直接写代码 + 自动 `aura check` 验证。

---

## 1. 资产盘点

| 资产 | 路径 | 状态 | 对 DSH 的价值 |
|------|------|------|---------------|
| 编译器源码 | `compiler/` | ✅ 完整 | 可构建 `aura` CLI 做代码验证 |
| CLI 工具 | `cli/` | ✅ 3 个二进制 | `aura check/fmt/lsp/debug` |
| 语言教程 | `book/chapter-*.md` | ✅ 4 章 | 精确的语法文档 |
| API 文档 | `docs/api/index.md` | ✅ 19 模块 80 函数 | 精确的 stdlib 签名 |
| 示例代码 | `examples/` | ✅ 36 个 .aura 文件 | Few-shot 参考 |
| VS Code 扩展 | `vscode-extension/` | ✅ 完整 | TextMate 语法、LSP 客户端 |
| 设计文档 | `docs/*.md` | ✅ 20+ 篇 | 架构理解 |
| 构建系统 | `loom/` | ✅ 完整 | 项目构建 |
| DSH 集成文档 | `docs/00-*.md` | ✅ 本文档 | 方案总览 |

---

## 2. 方案架构

```
用户自然语言需求
      │
      ▼
┌─────────────────────────────────────────────┐
│  Pillar 1: 语言卡片 (Language Card)           │
│  docs/01-aura-language-card.md (~3K tokens)   │
│  → 注入 DSH 系统提示词                         │
│  → 大模型"知道"语法怎么写                       │
└──────────────────────┬──────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────┐
│  Pillar 2: 验证工具 (Validation Tool)          │
│  构建 aura CLI → 调用 aura check/fmt           │
│  → 生成代码后自动验证 + 修正                     │
│  → 形成自校验闭环                              │
└──────────────────────┬──────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────┐
│  Pillar 3: 示例库 (Example Library)            │
│  examples/ (36 个真实 Aura 文件)               │
│  book/ (4 章教程)                              │
│  → Few-shot / RAG 检索源                      │
│  → 覆盖常见模式                               │
└─────────────────────────────────────────────┘
```

---

## 3. 实施步骤

### Phase 1：语言卡片注入（立即可用）

**目标**：大模型在系统提示词中直接获得 Aura 语法知识。

**已完成**：
- ✅ `docs/01-aura-language-card.md` — 精简语法参考（~3K tokens）
- ✅ `docs/02-aura-stdlib-reference.md` — 标准库完整 API
- ✅ `docs/04-aura-style-guide.md` — 代码风格指南

**待完成**：
- [ ] 将语言卡片注入 DSH 系统提示词（方式取决于 DSH 架构）
- [ ] 创建 `aura-lang` DSH skill（按需加载完整参考）

### Phase 2：验证工具闭环（需构建编译器）

**目标**：agent 可以验证生成的 Aura 代码并自动修正。

```bash
# 步骤 1：构建 Aura 编译器
cd D:\Code\AuraLang
cargo build --release --features "llvm,jit,std-all"

# 步骤 2：aura 二进制出现在 target/release/aura
# 步骤 3：创建 aura-check 工具

# 工具使用：
aura check main.aura        # 语法+语义检查
aura fmt --check main.aura  # 格式检查
aura run main.aura          # 编译运行
```

**aura-check 工具规格**：详见 `tools/aura-check-spec.md`

### Phase 3：示例库增强

**目标**：提供丰富的 Few-shot 参考和 RAG 检索源。

**已有**：36 个示例文件覆盖基础语法、并发、FFI、游戏等。

**待完成**：
- [ ] 按场景分类整理示例（basics/concurrency/ffi/app）
- [ ] 创建示例索引（关键词 → 文件映射）
- [ ] 可选：示例向量化用于 RAG

---

## 4. 关键决策

### 决策 1：语言卡片 vs Skill vs RAG

| 方案 | 优点 | 缺点 | 推荐场景 |
|------|------|------|---------|
| **系统提示词** | 立即可用，零延迟 | 占用 tokens，所有对话携带 | 个人使用、快速验证 ⭐ |
| **DSH Skill** | 按需加载，省 tokens | 首次延迟，需用户触发 | 多语言场景 |
| **RAG 检索** | 知识可动态更新 | 需向量数据库 | 生产环境、大型项目 |

**当前推荐**：Phase 1 用系统提示词（快速验证），Phase 3 用 Skill + RAG（长期方案）

### 决策 2：编译器构建 vs 轻量验证

| 方案 | 优点 | 缺点 |
|------|------|------|
| **构建完整编译器** | 最精确，支持完整类型检查 | 需 Rust 工具链 + LLVM |
| **轻量语法验证** | 零依赖，纯 JS 实现 | 只能检查基本结构 |

**当前推荐**：如果机器有 Rust 1.75+ 和 LLVM 17+，构建完整编译器；否则用轻量验证器。

---

## 5. 效果评估

| 指标 | 无集成 | Phase 1（语言卡片） | Phase 2（+验证工具） |
|------|--------|-------------------|---------------------|
| 语法正确率 | ~10% | ≥80% | ≥95% |
| 关键字正确率 | ~5% | ≥70% | ≥90% |
| stdlib 调用正确率 | ~0% | ≥60% | ≥85% |
| 自校验闭环 | 无 | 0 次 | ≤2 次 |
| 首次成功率 | <15% | ≥50% | ≥80% |

---

## 6. 文件结构

```
D:\Code\AuraLang\
├── compiler/               ← 编译器源码（Rust）
├── cli/                    ← CLI 工具
├── book/                   ← 用户文档（mdbook）
├── docs/
│   ├── 00-Aura-DSH-Integration-Plan.md  ← 本文档
│   ├── 01-aura-language-card.md           ← ⭐ 语言卡片（核心！）
│   ├── 02-aura-stdlib-reference.md        ← 标准库 API
│   ├── 04-aura-style-guide.md             ← 代码风格
│   └── api/index.md                        ← 自动生成的 API 文档
├── examples/               ← 36 个示例文件
├── skills/
│   └── aura-lang/SKILL.md  ← DSH skill 包
├── tools/
│   └── aura-check-spec.md  ← 验证工具规格
└── vscode-extension/       ← VS Code 扩展
```

---

## 7. 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| 语言卡片过长 | 中 | 中 | 控制在 3K tokens，stdlib 用独立文件 |
| 大模型混淆 Kotlin | 中 | 中 | 强调独有特性 + 负面示例 |
| 编译器构建失败 | 中 | 高 | 准备轻量验证器备选方案 |
| stdlib API 不稳定 | 低 | 中 | 版本化文档，标记稳定/实验 |
| 示例代码不够覆盖 | 中 | 低 | 36 个示例已覆盖主要模式 |

---

## 8. v3.0：DSH 原生插件方案（已实施）

### 8.1 方案对比

| 方式 | 手动注入 | 自定义预设+Skill |
|------|---------|-----------------|
| 设置复杂度 | 每次手动粘贴 | 一次配置，自动加载 |
| Token 开销 | 所有会话 | 仅选择预设时 |
| 维护性 | 差（手动同步） | 好（文件即配置） |
| DSH 原生 | ❌ | ✅ Skill + Preset |

### 8.2 已创建的文件

```
~/.dsh/.agent-presets/aura-coding/
├── preset.yml                    ← 预设元数据（名称、描述、顺序）
├── agent.cordis.yml              ← 基于 standard 的完整配置
└── skills/aura-lang/
    └── SKILL.md (13KB)           ← Aura 语言知识（行为指令 + 语法 + 验证 + 示例）
```

### 8.3 使用方法

1. **DSH Web GUI** → 新建会话 → 选择预设 **"Aura 编码模式"**
2. 模型自动获得 Aura 语言知识，直接生成 `.aura` 代码
3. 不需要手动注入任何提示词

### 8.4 Skill 触发条件

Skill 的 `description` 字段定义了触发条件：
- 用户提到 Aura、NovaOS、data struct、actor、comptime 等关键词
- 涉及 `.aura` 文件的创建或编辑
- 涉及 FFI、Result<T,E>、concurrent 等 Aura 独有概念

### 8.5 与 v2.0 的演进

| 版本 | 方案 | 状态 |
|------|------|------|
| v1.0 | 纯系统提示词注入 | 已废弃 |
| v2.0 | 语言卡片 + 验证工具 + 示例库 | 文档完成 |
| v3.0 | DSH 原生预设 + Skill | 已实施 |
| **v4.0** | **修复 SKILL.md 三大问题** | **已实施** ✅ |

v4.0 核心修复：

| 问题 | v3.0 表现 | v4.0 修复 |
|------|----------|----------|
| 创建探查任务 | 模型创建"探查语法边界"任务 | 加入行为指令：直接写代码，不探查 |
| 不检查语法 | 无验证流程 | 强制 `aura check` 验证步骤 |
| 不知道能力 | 缺少实际示例 | 增加 7 个完整可运行示例 |
| Import 语法错 | `import std.io as io` | 修正为 `import aura.lang.std.IO.*` |
| API 调用方式错 | `io.println()` | 修正为 `println()` (prelude) 和 `aura.lang.std.IO.println()` (模块) |

v2.0 的语言卡片（`docs/01-aura-language-card.md`）仍是核心知识源，
v4.0 将其封装为 DSH 原生 Skill，并修正所有语法错误。
