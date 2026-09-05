# aura-loom

Aura 语言构建系统 — 多文件编译、增量构建、任务 DAG、插件扩展。

> **设计理念**：纯 TOML 声明式配置，对标 Cargo.toml / Gradle / Bazel。
> 构建系统是 Aura 语言的一部分，集成在 AuraLang workspace 中。
> 详见 `docs/构建系统设计.md`。

## 特性

- ✅ **单一配置文件**：`aura.toml`，纯 TOML 声明式，无代码配置
- ✅ **构建生命周期**：clean → resolve → compile → test → package → verify → install → deploy
- ✅ **任务 DAG 引擎**：拓扑排序 + 并行调度 + 循环检测
- ✅ **源码集**：main / test / bench，支持自定义
- ✅ **增量构建**：fingerprint 驱动的本地 + 远程两级缓存
- ✅ **依赖管理**：5 种配置分组 + BOM 版本锁定 + 冲突解析
- ✅ **插件系统**：约定插件 + 显式插件 + 外部 `.so` 插件
- ✅ **构建包装器**：`aura-wrapper` 可重复构建
- ✅ **多项目管理**：Workspace 模式，共享缓存
- ✅ **仓库管理**：REST API 注册表 + 本地注册表
- ✅ **CI/CD 集成**：`aura ci` + `.aura-ci.yml` + GitHub Actions
- ✅ **IDE 集成**：`aura-project.json` 导出 + VS Code 任务
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
# 创建项目
loom new my-app

# 构建
loom build

# 运行测试
loom test

# 运行应用
loom run

# 监听模式（开发期）
loom build --watch

# 发布
loom publish
```

## 项目结构

```text
aura-loom/
├── src/
│   ├── main.rs          # CLI 入口
│   ├── lib.rs           # 公共 API（供插件调用）
│   ├── cli/             # CLI 层
│   ├── manifest/        # 配置层（aura.toml 解析）
│   ├── task/            # 任务引擎
│   ├── lifecycle/       # 生命周期
│   ├── sourceset/       # 源码集
│   ├── dep/             # 依赖管理
│   ├── cache/           # 构建缓存
│   ├── plugin/          # 插件系统
│   ├── workspace/       # 多项目
│   ├── wrapper/         # 构建包装器
│   ├── registry/        # 仓库管理
│   ├── ci/              # CI/CD
│   ├── ide/             # IDE 集成
│   ├── watch/           # 监听模式
│   └── error.rs         # 统一错误类型
├── docs/                # 设计文档
├── tests/               # 测试
├── examples/            # 示例项目
└── Cargo.toml
```

## 开发路线图

| 阶段 | 周期 | 产出 |
|------|------|------|
| L1 | 1 周 | Manifest + CLI 骨架 |
| L2 | 1.5 周 | 任务引擎 + 生命周期 |
| L3 | 1 周 | 构建缓存 |
| L4 | 1.5 周 | 插件系统 |
| L5 | 1 周 | Wrapper + Workspace |
| L6 | 1 周 | 仓库 + CI/CD |
| L7 | 1 周 | IDE + 开发体验 |

## 依赖

- `compiler`：Aura 编译器 SDK（`../AuraLang/compiler`）

## 许可证

MIT
