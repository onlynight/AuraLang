# 插件 API 文档

Aura 构建系统支持三类插件：约定插件、显式插件、外部插件。本文档描述外部插件的 C ABI 接口。

## 插件类型

| 类型 | 说明 | 示例 |
|------|------|------|
| 约定插件（Convention） | 自动激活，提供默认行为 | `aura-stdlib`, `aura-test-harness`, `aura-watch` |
| 显式插件（Explicit） | 需手动启用（`[plugins]` 中设为 `true`） | `aura-doc-gen`, `aura-format`, `aura-aot`, `aura-ci` |
| 外部插件（External） | 编译为 `.so`/`.dll`/`.dylib` 的动态库 | 用户自定义 |

## 外部插件 C ABI

外部插件必须导出以下三个 C ABI 符号：

### 1. `aura_plugin_info`

返回插件元信息。

```c
struct AuraPluginInfo {
    const char* name;        // 插件名称（不能为 null）
    const char* version;     // 插件版本（不能为 null）
    const char* description; // 插件描述（可以为 null）
};

struct AuraPluginInfo aura_plugin_info(void);
```

### 2. `aura_plugin_configure`

配置阶段回调。在构建开始前调用。

```c
int aura_plugin_configure(void);
```

返回值：`0` 表示成功，非零表示失败。

插件可在此阶段：
- 注册自定义任务
- 修改源码集配置
- 设置默认构建选项

### 3. `aura_plugin_execute`

任务执行回调。当任务调度器执行到本插件注册的任务时调用。

```c
int aura_plugin_execute(
    const char* task_name,  // 任务名称
    char* output_buf,       // 输出缓冲区（可写）
    size_t buf_size         // 缓冲区大小
);
```

返回值：`0` 表示成功，非零表示失败。

输出写入 `output_buf`，以 null 结尾。如果缓冲区不足，输出会被截断。

## 配置

在 `aura.toml` 中配置外部插件：

```toml
[plugins.external]
# 插件名 = { 路径, 版本约束, 配置 }
my-plugin = { path = "plugins/target/release/my_plugin.dll", version = "1.0.0" }
```

- `path`：插件文件路径（相对于项目根目录），支持 `.so`/`.dll`/`.dylib`
- `version`：版本约束（可选）

## 编译

### Rust 实现

```toml
# Cargo.toml
[lib]
name = "my_plugin"
crate-type = ["cdylib"]
```

```rust
// src/lib.rs
use std::os::raw::c_char;

#[repr(C)]
pub struct AuraPluginInfo {
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
}

#[no_mangle]
pub extern "C" fn aura_plugin_info() -> AuraPluginInfo {
    // ...
}

#[no_mangle]
pub extern "C" fn aura_plugin_configure() -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn aura_plugin_execute(
    task_name: *const c_char,
    output_buf: *mut c_char,
    buf_size: usize,
) -> i32 {
    0
}
```

### 编译命令

```bash
cd plugins
cargo build --release
```

输出：
- Windows: `target/release/my_plugin.dll`
- Linux: `target/release/libmy_plugin.so`
- macOS: `target/release/libmy_plugin.dylib`

## 本项目结构

```text
plugin-example/
├── aura.toml                        # 项目配置（含 [plugins.external]）
├── plugins/
│   ├── Cargo.toml                   # Rust cdylib 配置
│   ├── src/
│   │   └── lib.rs                   # C ABI 实现
│   └── target/
│       └── release/
│           └── custom_plugin.dll    # 编译产物
├── src/
│   └── main.aura                    # 入口文件
└── docs/
    └── plugin-api.md               # 本文档
```

## 内置插件清单

### 约定插件（自动激活）

| 插件 | 名称 | 功能 |
|------|------|------|
| 标准库 | `aura-stdlib` | 自动注册 `io`/`math`/`string`/`json` 等标准模块 |
| 测试框架 | `aura-test-harness` | 注册 `run-tests` 任务，注入测试框架 |
| Watch 模式 | `aura-watch` | 文件监听 + 增量重编 |

### 显式插件（手动启用）

| 插件 | 名称 | 功能 |
|------|------|------|
| 文档生成 | `aura-doc-gen` | 生成 HTML/Markdown 文档 |
| 格式化 | `aura-format` | 源码格式化 |
| AOT 编译 | `aura-aot` | 提前编译为原生代码 |
| CI 集成 | `aura-ci` | CI 流水线集成 |
