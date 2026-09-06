# Workspace 示例

演示多项目管理（monorepo）。

## 目录结构

```text
workspace/
├── aura.toml               # Workspace 根配置（含 [workspace] 表）
├── libs/
│   ├── core/
│   │   ├── aura.toml       # 库项目（library = true）
│   │   └── src/
│   │       └── lib.aura    # 核心数学运算
│   └── utils/
│       ├── aura.toml       # 库项目（library = true）
│       └── src/
│           └── lib.aura    # 字符串和集合工具
└── apps/
    └── server/
        ├── aura.toml       # 应用项目（依赖 core + utils）
        └── src/
            └── main.aura   # 入口（import core.* + utils.*）
```

## Workspace 配置

根 `aura.toml` 中定义 `[workspace]` 表：

```toml
[workspace]
members = ["libs/core", "libs/utils", "apps/server"]
default-members = ["libs/core", "libs/utils", "apps/server"]
resolver = "2"

[workspace.dependency-management]
aura-math = "1.0.0"
aura-io = "1.0.0"
```

## 成员依赖图

```text
apps/server
  ├── core    (libs/core)
  └── utils   (libs/utils)
```

## 使用

```bash
cd workspace
loom build                        # 构建所有默认成员
loom build --member core          # 仅构建 core 成员
loom build --member server        # 仅构建 server 成员
loom build --member server --with-deps  # 构建 server + 其依赖
```

> 注：`--member` 接受成员名称（aura.toml 中的 `name` 字段），不是路径。

## 验证结果

```text
$ loom build --dir .
Workspace: 3 个成员, 选择 3 个
  → core v0.1.0    libs/core
  → utils v0.1.0   libs/utils
  → server v0.1.0  apps/server

$ loom build --dir . --member core
Workspace: 3 个成员, 选择 1 个
  → core v0.1.0    libs/core

$ loom build --dir . --member server
Workspace: 3 个成员, 选择 1 个
  → server v0.1.0  apps/server
```
