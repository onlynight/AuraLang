# Workspace 示例

演示多项目管理（monorepo）。

## 目录结构

```text
workspace/
├── aura.toml           # Workspace 根配置
├── libs/
│   ├── core/
│   │   ├── aura.toml   # 库项目
│   │   └── src/
│   └── utils/
│       ├── aura.toml
│       └── src/
├── apps/
│   └── server/
│       ├── aura.toml   # 应用项目
│       └── src/
└── target/
```
