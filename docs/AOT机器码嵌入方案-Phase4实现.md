# AOT 机器码嵌入方案 —— Phase 4 高级特性实现文档

## 概述

Phase 4 实现了 AOT 机器码嵌入方案的高级特性，包括动态库模式、编译期链接、跨模块调用、性能基准和插件系统。

**设计文档参考**：`docs/AOT机器码嵌入方案-详细设计.md` §9 Phase 4

---

## 4.1 Tier 2 动态库模式

### 功能描述

`OutputFormat::SharedLibrary` 允许将 AOT 编译的模块编译为动态库（`.so` / `.dylib` / `.dll`），用于插件系统和第三方模块扩展。

### API

```rust
// 输出格式
pub enum OutputFormat {
    // ... 其他格式
    SharedLibrary,  // Tier 2: 动态库
}

// 编译产物
pub struct AotOutput {
    // ... 其他字段
    pub shared_library_path: Option<PathBuf>,  // 动态库路径
}

// 链接函数
pub fn link_to_shared_library(
    input_path: &Path,
    lib_path: &Path,
    options: &AotOptions,
) -> Result<(), AotError>;
```

### 跨平台支持

| 平台 | 标志 | 输出格式 |
|------|------|----------|
| Windows | `-shared -Wl,/DLL` | `.dll` |
| Linux | `-shared` | `.so` |
| macOS | `-dynamiclib` | `.dylib` |

### 文件扩展名检测

`.auc_compile` 函数自动从输出文件扩展名检测格式：
- `.so` / `.dylib` / `.dll` → `OutputFormat::SharedLibrary`

---

## 4.2 Tier 4 编译期链接

### 功能描述

`OutputFormat::RustHost` 将 AOT 目标文件与 Rust 宿主二进制链接，生成单一可执行文件。适用于部署场景，将编译产物嵌入宿主程序，无需运行时动态加载。

### API

```rust
pub enum OutputFormat {
    RustHost,  // Tier 4: 编译期链接
}

pub struct AotOutput {
    pub rust_host_path: Option<PathBuf>,  // 编译期链接产物路径
}

pub fn link_to_rust_host(
    input_path: &Path,
    host_path: &Path,
    options: &AotOptions,
) -> Result<(), AotError>;
```

### 实现策略

使用 `clang` 直接链接目标文件为可执行文件，并注入 `aura_rust_host_entry` 符号作为入口点。支持 std C FFI 链接。

---

## 4.3 跨模块调用

### 功能描述

多模块依赖解析支持 AOT 模块之间的函数调用。当一个模块需要调用另一个模块的函数时，通过依赖解析机制自动查找和绑定。

### 数据结构

```rust
/// 模块依赖描述
pub struct ModuleDependency {
    pub name: String,                    // 依赖的模块名称
    pub imports: Vec<String>,            // 需要导入的函数名列表
    pub resolved_module_id: Option<u32>, // 依赖模块已加载时的 ID
}

/// 跨模块符号条目
pub struct CrossModuleSymbol {
    pub name: String,         // 函数名
    pub module_id: u32,       // 所属模块 ID
    pub func_idx: usize,      // 函数在分发表中的索引
}
```

### API

```rust
impl AotRuntime {
    /// 注册模块依赖
    pub fn register_module_dependency(
        &mut self,
        module_id: u32,
        dependency: ModuleDependency,
    );

    /// 解析所有模块依赖
    /// 返回 (已解析数量, 未解析数量)
    pub fn resolve_dependencies(&mut self) -> (usize, usize);

    /// 按函数名查找跨模块符号
    pub fn lookup_cross_module_symbol(&self, func_name: &str) -> Option<&CrossModuleSymbol>;

    /// 获取模块的依赖列表
    pub fn get_module_dependencies(&self, module_id: u32) -> Option<&[ModuleDependency]>;

    /// 获取已解析的跨模块符号数量
    pub fn cross_module_symbol_count(&self) -> usize;
}
```

### 使用流程

1. 加载模块 A（包含函数 `add`）
2. 加载模块 B（依赖模块 A 的 `add` 函数）
3. 注册模块 B 对模块 A 的依赖
4. 调用 `resolve_dependencies()` 解析依赖
5. 通过 `lookup_cross_module_symbol()` 查找跨模块函数

---

## 4.4 性能基准

### 测试文件

`compiler/tests/aot_embed_perf_tests.rs`

### 基准用例

| 测试 | 描述 | 迭代次数 |
|------|------|----------|
| `bench_function_call` | 简单函数调用 | 10,000 |
| `bench_accumulate_loop` | 累加循环（仅验证编译） | - |

### 性能结果（示例）

```
=== 函数调用 性能基准 (10000 次迭代) ===
  VM  解释执行: 3405.10 ms
  AOT 机器码:   3619.86 ms
  加速比:       0.94x
```

**注**：对于简单函数调用，VM 解释执行可能比 AOT 更快，因为 AOT 的模块加载和分发表查找开销在小程序中占主导。对于复杂程序，AOT 应展现优势。

---

## 4.5 插件系统

### 功能描述

`PluginManager` 管理第三方 AOT 模块的加载、卸载和发现。支持通过 `.auc` 文件格式加载插件。

### 数据结构

```rust
/// 插件信息
pub struct PluginInfo {
    pub name: String,       // 插件名称
    pub path: String,       // 插件路径
    pub version: String,    // 插件版本
    pub func_count: usize,  // 导出函数数量
}
```

### API

```rust
impl PluginManager {
    /// 创建新的插件管理器
    pub fn new() -> Self;

    /// 添加插件搜索路径
    pub fn add_search_path(&mut self, path: String);

    /// 加载插件
    pub fn load_plugin(&mut self, path: &str) -> Result<u32, String>;

    /// 卸载插件
    pub fn unload_plugin(&mut self, plugin_name: &str) -> bool;

    /// 列出所有已加载插件
    pub fn list_plugins(&self) -> &[PluginInfo];

    /// 获取插件数量
    pub fn plugin_count(&self) -> usize;

    /// 获取关联的 AOT 运行时
    pub fn runtime(&self) -> &AotRuntime;
    pub fn runtime_mut(&mut self) -> &mut AotRuntime;

    /// 发现插件（扫描搜索路径）
    pub fn discover_plugins(&self) -> Vec<String>;

    /// 加载所有发现的插件
    pub fn load_all_discovered(&mut self) -> Vec<Result<u32, String>>;
}
```

### 使用示例

```rust
let mut pm = PluginManager::new();
pm.add_search_path("/path/to/plugins".to_string());

// 发现插件
let plugins = pm.discover_plugins();

// 加载插件
for plugin_path in plugins {
    let module_id = pm.load_plugin(&plugin_path)?;
}

// 通过运行时调用插件函数
let runtime = pm.runtime();
// ... 调用函数
```

---

## 文件改动清单

### 新增文件

| 文件 | 说明 |
|------|------|
| `compiler/tests/aot_phase4_tests.rs` | Phase 4 集成测试（11 个测试） |
| `compiler/tests/aot_embed_perf_tests.rs` | 性能基准测试 |
| `docs/AOT机器码嵌入方案-Phase4实现.md` | 本文档 |

### 修改文件

| 文件 | 改动 |
|------|------|
| `compiler/src/codegen/aot/mod.rs` | 新增 `SharedLibrary` / `RustHost` 输出格式，`shared_library_path` / `rust_host_path` 字段 |
| `compiler/src/codegen/aot/linker.rs` | 新增 `link_to_shared_library` / `link_to_rust_host` 函数 |
| `compiler/src/vm/aot_runtime.rs` | 新增 `ModuleDependency` / `CrossModuleSymbol` / `PluginManager` |

---

## 运行测试

```bash
# Phase 4 集成测试
cargo test --features llvm --test aot_phase4_tests

# 性能基准
cargo test --features llvm --test aot_embed_perf_tests -- --no-capture

# 全部 AOT 测试
cargo test --features llvm aot

# 完整测试套件
cargo test --features llvm
```