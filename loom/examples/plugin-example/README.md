# 插件示例

演示自定义外部插件开发。

## 目录结构

```text
plugin-example/
├── aura.toml                     # 项目配置（含 [plugins.external]）
├── plugins/
│   ├── Cargo.toml                # Rust cdylib 配置
│   ├── src/
│   │   └── lib.rs                # C ABI 实现（aura_plugin_info/configure/execute）
│   └── target/
│       └── release/
│           └── custom_plugin.dll # 编译产物
├── src/
│   └── main.aura                 # 入口文件
├── docs/
│   └── plugin-api.md             # 插件 API 文档
├── build-plugin.ps1              # PowerShell 编译脚本
└── build-plugin.sh               # Shell 编译脚本
```

## 外部插件 C ABI

插件必须导出三个 C ABI 符号：

| 符号 | 签名 | 说明 |
|------|------|------|
| `aura_plugin_info` | `() -> AuraPluginInfo` | 返回插件元信息（name/version/description） |
| `aura_plugin_configure` | `() -> i32` | 配置阶段回调，注册任务 |
| `aura_plugin_execute` | `(task_name, output_buf, buf_size) -> i32` | 任务执行回调 |

## 使用

```bash
# 1. 编译插件
cd plugins && cargo build --release
# Windows: plugins/target/release/custom_plugin.dll
# Linux:   plugins/target/release/libcustom_plugin.so
# macOS:   plugins/target/release/libcustom_plugin.dylib

# 2. 构建项目（自动加载插件）
loom build --dir .
```

## 验证结果

```text
$ loom build --dir .
项目: plugin-example v0.1.0
插件: 3 个 (aura-stdlib, aura-test-harness, custom-plugin)
[custom-plugin] configure: plugin configured
[custom-plugin]   registered custom task: greet
[custom-plugin]   usage: loom build --task greet

═══ 构建结果 ═══
  ✓ clean — ✓ 无需清理
  ✓ resolve — ✓ 依赖解析完成
  ✓ compile-main — ✓ 编译 main 源码集: 1 个文件
═══ 3 执行, 0 跳过 ═══
```

## 插件 API 文档

详见 [docs/plugin-api.md](docs/plugin-api.md)。
