# aura-loom

Aura 语言构建系统 — 多文件编译、增量构建、任务 DAG、插件扩展。

> **设计理念**：纯 TOML 声明式配置，对标 Gradle / Bazel（而非 Cargo，Cargo 同时承担生态层）。
> 构建系统是 Aura 语言的一部分，集成在 AuraLang workspace 中。
> 包生态（install / publish / deps）由 `aura` 承担。
> 详见 `docs/构建系统设计.md` 与 `docs/多进程与CLI架构分析报告.md` §4。

## 特性

- ✅ **单一配置文件**：`aura.toml`，纯 TOML 声明式，无代码配置
- ✅ **构建生命周期**：clean → resolve → compile → test → run
- ✅ **任务 DAG 引擎**：拓扑排序 + 并行调度 + 循环检测
- ✅ **源码集**：main / test / bench，支持自定义
- ✅ **增量构建**：fingerprint 驱动的本地 + 远程两级缓存
- ✅ **依赖解析**：5 种配置分组 + BOM 版本锁定 + 本地路径解析
- ✅ **插件系统**：约定插件 + 显式插件 + 外部 `.so` 插件
- ✅ **构建包装器**：`aura-wrapper` 可重复构建
- ✅ **多项目管理**：Workspace 模式，共享缓存
- ✅ **CI/CD 集成**：`loom ci` + `.loom/.aura-ci.yml` + GitHub Actions
- ✅ **IDE 集成**：`.loom/aura-project.json` 导出 + VS Code 任务
- ✅ **Watch 模式**：文件监听 + 增量重编

## 安装

```bash
# 从源码构建
cd aura-loom
cargo build --release

# 使用构建包装器
aura-wrapper install
```

## 使用

```bash
# 创建项目（等价于 aura new）
loom new my-app

# 构建
loom build

# 运行测试
loom test

# 运行应用
loom run

# 监听模式（开发期）
loom watch

# 构建（开发期）
loom build --watch

# 注意：包生态操作请使用 aura 命令
aura install my-package
aura publish
aura deps
```

## 项目结构

```text
aura-loom/
├── src/
│   ├── main.rs          # CLI 入口
│   ├── lib.rs           # 公共 API（供插件调用）
│   ├── cli/             # CLI 层
│   ├── manifest/        # 配置层（aura.toml 解析）
│   │   ├── mod.rs       # LoomManifest 结构体 + serde 默认
│   │   ├── default.toml # 内置默认配置（include_str! 嵌入）
│   │   ├── default.rs   # 默认配置 + TOML 深度合并
│   │   ├── parse.rs     # 解析（合并逻辑）
│   │   ├── priority.rs  # CLI/Profile 覆盖
│   │   └── validate.rs  # 配置校验
│   ├── task/            # 任务引擎
│   ├── lifecycle/       # 生命周期
│   ├── sourceset/       # 源码集
│   ├── dep/             # 依赖解析（本地路径）
│   ├── cache/           # 构建缓存
│   ├── plugin/          # 插件系统
│   ├── workspace/       # 多项目
│   ├── wrapper/         # 构建包装器
│   ├── ci/              # CI/CD
│   ├── ide/             # IDE 集成
│   ├── watch/           # 监听模式
│   └── error.rs         # 统一错误类型
├── docs/                # 设计文档
├── tests/               # 测试
├── examples/            # 示例项目
└── Cargo.toml
```

## 配置优先级

`aura.toml` 支持"内置默认 + 项目覆盖"的合并语义。项目文件只需写"与默认不同的字段"。

### 优先级链（高 → 低）

```
CLI 参数  >  激活 Profile  >  项目 aura.toml  >  内置默认 aura.toml  >  serde 字段级默认
```

- **CLI**：`--opt-level`、`--debug`、`--profile` 等
- **Profile**：`--profile release` 激活后覆盖 build 字段
- **项目**：用户编写的 `aura.toml`
- **内置默认**：编译时嵌入的 `src/manifest/default.toml`
- **serde**：`#[serde(default)]` 字段级兜底

### 最小项目配置

```toml
# 仅 name/version/description，其余字段全部继承内置默认
name = "my-app"
version = "0.1.0"
description = "My App"
```

### 部分覆盖示例

```toml
name = "my-app"
version = "1.0.0"

# 只改 exclude，其他字段继承默认
[build.source-sets.main]
exclude = ["foo"]

# 只改 opt-level，其他字段继承默认
[profiles.release.build]
opt-level = 1
```

### 合并规则

| base | overlay | 结果 |
|---|---|---|
| Table | Table | 递归合并，overlay 优先 |
| Table | 非 Table | overlay 整体替换 |
| Array | Array | overlay 整体替换（不做元素级合并） |
| 标量 | 任意 | overlay 整体替换 |

详见 `docs/默认配置合并设计.md`。

## 开发路线图

| 阶段 | 周期 | 产出 |
|------|------|------|
| L1 | 1 周 | Manifest + CLI 骨架 |
| L2 | 1.5 周 | 任务引擎 + 生命周期 |
| L3 | 1 周 | 构建缓存 |
| L4 | 1.5 周 | 插件系统 |
| L5 | 1 周 | Wrapper + Workspace |
| L6 | 1 周 | CI/CD |
| L7 | 1 周 | IDE + 开发体验 |

## 依赖

- `compiler`：Aura 编译器 SDK（`../AuraLang/compiler`）

## 许可证

Apache-2.0
