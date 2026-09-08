# ext_ffi_demo — 三个 FFI Demo 技术方案

## 概述

三个 demo 均使用 **Aura 作为上层调用方**，区别在于下层 Aura AOT 编译产物的导出 ABI 格式不同。

| Demo | AOT 编译参数 | 导出 ABI | 调用机制 |
|---|---|---|---|
| **C FFI** | `--cabi --shared` | `aura_c_add` (C ABI) | `extern "C"` block → `libloading` |
| **Rust FFI** | `--cabi --shared` | `aura_c_add` (C ABI) | 同上（与 C FFI 相同机制） |
| **AOT 直调** | `--shared` | `aura_aot_add!2!2!2!2` (JitValue ABI) | `load_shared_library` → `call_func` |

---

## Workspace 结构

所有 demo 组织为一个 **loom workspace**，共享的 utils 库作为 workspace 成员：

```
examples/ext_ffi_demo/
├── aura.toml                 # Workspace 根配置
├── README.md                 # 本文件
├── libs/
│   └── utils/                # 共享函数库（三个 demo 共用）
│       ├── aura.toml         # library = true
│       └── src/
│           └── lib.aura      # add/multiply/factorial/power
├── demo_cffi/                # Demo 1: C FFI
│   ├── aura.toml             # 应用项目
│   ├── src/
│   │   └── main.aura         # 调用方（extern "C"）
│   ├── utils.h               # C 头文件（export-header 生成）
│   └── README.md
├── demo_rustffi/             # Demo 2: Rust FFI
│   ├── aura.toml             # 应用项目
│   ├── src/
│   │   └── main.aura         # 调用方（同 Demo 1）
│   ├── utils.rs              # Rust 绑定（展示兼容性）
│   └── README.md
└── demo_aot_direct/          # Demo 3: AOT 直调
    ├── aura.toml             # 应用项目
    ├── src/
    │   └── main.aura         # 调用方（load_shared_library）
    └── README.md
```

---

## Workspace 配置（aura.toml）

```toml
schema-version = "2.0"
name = "ext-ffi-demo"
version = "0.1.0"
description = "FFI Demo Workspace：C FFI / Rust FFI / AOT 直调"
authors = []
license = "MIT"

# 根项目不作为构建目标
library = false
entry = ""
exports = []

# ── Workspace 配置 ──
[workspace]
members = [
    "libs/utils",
    "demo_cffi",
    "demo_rustffi",
    "demo_aot_direct",
]
default-members = [
    "libs/utils",
    "demo_cffi",
    "demo_rustffi",
    "demo_aot_direct",
]
resolver = "2"

[workspace.build]
opt-level = 2
debug = true
out-dir = "target/build"
cache-dir = "target/cache"
parallel = true

# 根项目不需要这些
dependencies = []
compile-dependencies = []
runtime-dependencies = []
dev-dependencies = []
build-dependencies = []
tasks = []

[build]
opt-level = 2
debug = true
out-dir = "target/build"
cache-dir = "target/cache"
parallel = true

[build.source-sets.main]
source-dirs = []
include = []
exclude = []
depends-on = []

[build.alias]

[plugins]
aura-stdlib = true
aura-test-harness = false
aura-doc-gen = false
aura-format = false
aura-aot = true
aura-watch = false
aura-ci = false

[plugins.external]

[profiles.release]
activate = false

[profiles.debug]
activate = true

[repositories]
central = "https://registry.aura-lang.dev"

[repositories.custom]

[resources]
include = []
exclude = []

[package]
format = "auz"
include-sources = false
include-docs = false
include-native = true
native-targets = []
aot-opt-level = 2
```

---

## 共享库配置（libs/utils/aura.toml）

```toml
schema-version = "2.0"
name = "utils"
version = "0.1.0"
description = "三个 FFI Demo 共用的函数库"
authors = []
license = "MIT"
entry = "src/lib.aura"
exports = ["add", "multiply", "factorial", "power"]
library = true

dependencies = []
compile-dependencies = []
runtime-dependencies = []
dev-dependencies = []
build-dependencies = []
tasks = []

[build]
opt-level = 2
debug = true
out-dir = "target/build"
cache-dir = "target/cache"
parallel = true

[build.source-sets.main]
source-dirs = ["src"]
resource-dirs = []
include = ["**/*.aura"]
exclude = ["**/*.test.aura", "vendor/**", "target/**"]
depends-on = []

[build.alias]

[plugins]
aura-stdlib = true
aura-test-harness = false
aura-doc-gen = false
aura-format = false
aura-aot = true
aura-watch = false
aura-ci = false

[plugins.external]

[profiles.release]
activate = false

[profiles.debug]
activate = true

[repositories]
central = "https://registry.aura-lang.dev"

[repositories.custom]

[resources]
include = []
exclude = []

[package]
format = "auz"
include-sources = false
include-docs = false
include-native = true
native-targets = []
aot-opt-level = 2
```

---

## 应用项目配置示例（demo_cffi/aura.toml）

```toml
schema-version = "2.0"
name = "demo-cffi"
version = "0.1.0"
description = "Demo 1: C FFI — Aura 调用 Aura AOT 的 C ABI 产物"
authors = []
license = "MIT"
entry = "src/main.aura"
exports = ["main"]
library = false

# 依赖共享库（用于 AOT 编译时被调用的函数）
dependencies = ["utils"]
compile-dependencies = []
runtime-dependencies = []
dev-dependencies = []
build-dependencies = []
tasks = []

[build]
opt-level = 2
debug = true
out-dir = "target/build"
cache-dir = "target/cache"
parallel = true

[build.source-sets.main]
source-dirs = ["src"]
resource-dirs = []
include = ["**/*.aura"]
exclude = ["**/*.test.aura", "vendor/**", "target/**"]
depends-on = []

[build.alias]

[plugins]
aura-stdlib = true
aura-test-harness = false
aura-doc-gen = false
aura-format = false
aura-aot = true
aura-watch = false
aura-ci = false

[plugins.external]

[profiles.release]
activate = false

[profiles.debug]
activate = true

[repositories]
central = "https://registry.aura-lang.dev"

[repositories.custom]

[resources]
include = []
exclude = []

[package]
format = "auz"
include-sources = false
include-docs = false
include-native = true
native-targets = []
aot-opt-level = 2
```

> `demo_rustffi/aura.toml` 和 `demo_aot_direct/aura.toml` 结构相同，仅 `name` 和 `description` 不同。

---

## 共享库源码（libs/utils/src/lib.aura）

```aura
// 三个 demo 共用的函数库，无 main 函数

fun add(a: Int, b: Int): Int = a + b

fun multiply(a: Int, b: Int): Int = a * b

fun factorial(n: Int): Int = if n <= 1 then 1 else n * factorial(n - 1)

fun power(base: Int, exp: Int): Int = if exp <= 0 then 1 else base * power(base, exp - 1)
```

---

## Demo 1：C FFI（Aura 调用 Aura AOT 的 C ABI 产物）

### 架构

```
demo_cffi/src/main.aura ──extern"C"──→ utils.dll (Aura AOT + --cabi --shared)
   │                              │
   └── JIT ─── libloading ────────┘
```

### loom 构建命令

```bash
cd examples/ext_ffi_demo

# 1. 生成 C 头文件（供文档参考）
aura export-header libs/utils/src/lib.aura --out demo_cffi/utils.h

# 2. 构建共享库（AOT + C ABI）
loom build --member utils --aot --shared --cabi

# 3. 构建调用方
loom build --member demo-cffi

# 4. 运行
loom run --member demo-cffi
```

### 调用方 Aura 源码（demo_cffi/src/main.aura）

```aura
extern "C" {
    fun add(a: Int, b: Int): Int;
    fun multiply(a: Int, b: Int): Int;
    fun factorial(n: Int): Int;
    fun power(base: Int, exp: Int): Int;
}

fun main() = {
    let sum = add(3, 4)
    println("add(3, 4) = " + str(sum))
    
    let prod = multiply(3, 4)
    println("multiply(3, 4) = " + str(prod))
    
    let fact = factorial(5)
    println("factorial(5) = " + str(fact))
    
    let pw = power(2, 10)
    println("power(2, 10) = " + str(pw))
}
```

### 验证标准

- `add(3, 4)` 返回 7
- `multiply(3, 4)` 返回 12
- `factorial(5)` 返回 120
- `power(2, 10)` 返回 1024

---

## Demo 2：Rust FFI（与 Demo 1 相同机制，展示 Rust 兼容性）

### 架构

```
demo_rustffi/src/main.aura ──extern"C"──→ utils.dll (Aura AOT + --cabi --shared)
   │                              │
   └── JIT ─── libloading ────────┘ (与 Demo 1 完全相同)
```

### loom 构建命令

```bash
cd examples/ext_ffi_demo

# 与 Demo 1 完全相同
loom build --member utils --aot --shared --cabi
loom build --member demo-rustffi
loom run --member demo-rustffi
```

### Rust 绑定文件（demo_rustffi/utils.rs）

```rust
// 展示同一 Aura AOT DLL 可被 Rust 直接调用
// 无需额外编译步骤，Rust 通过 extern "C" 直接消费 aura_c_* 导出

#[link(name = "utils", kind = "dylib")]
extern "C" {
    pub fn aura_c_add(a: i32, b: i32) -> i32;
    pub fn aura_c_multiply(a: i32, b: i32) -> i32;
    pub fn aura_c_factorial(n: i32) -> i32;
    pub fn aura_c_power(base: i32, exp: i32) -> i32;
}

fn main() {
    let sum = aura_c_add(3, 4);
    println!("Rust calls aura_c_add(3, 4) = {}", sum);
    
    let fact = aura_c_factorial(5);
    println!("Rust calls aura_c_factorial(5) = {}", fact);
}
```

### 说明

Demo 2 与 Demo 1 的运行时路径完全相同（都是 `libloading` + `dlsym`）。区别在于：
- **Demo 1** 强调 C ABI 头文件生成（`export-header`）和 C 调用约定
- **Demo 2** 强调 Rust 兼容性——同一 DLL 可被 Rust 直接通过 `extern "C"` 调用

两者使用相同的 `utils.dll`，只是消费角度不同。

### 验证标准

- 与 Demo 1 相同

---

## Demo 3：AOT 直调（Aura 调用 Aura AOT 的 JitValue ABI 产物）

### 架构

```
demo_aot_direct/src/main.aura ──load_shared_library──→ utils.dll (Aura AOT + --shared)
   │                                                │
   └── call_func(module_id, 0, [3,4]) ──→ aura_aot_add!2!2!2!2
```

### loom 构建命令

```bash
cd examples/ext_ffi_demo

# 1. 构建共享库（AOT，不带 --cabi，导出 JitValue ABI）
loom build --member utils --aot --shared

# 2. 构建调用方
loom build --member demo-aot-direct

# 3. 运行
loom run --member demo-aot-direct
```

### 调用方 Aura 源码（demo_aot_direct/src/main.aura）

```aura
fun main() = {
    // 通过 load_shared_library 加载 Aura AOT 编译的共享库
    let result = load_shared_library("target/build/libs/utils/utils.dll")
    let module_id = result.module_id
    
    // 通过 call_func 调用函数（func_idx 由运行时符号枚举确定）
    // func_idx=0: add, func_idx=1: multiply, func_idx=2: factorial, func_idx=3: power
    
    let sum = call_func(module_id, 0, [3, 4])
    println("add(3, 4) = " + str(sum))
    
    let prod = call_func(module_id, 1, [3, 4])
    println("multiply(3, 4) = " + str(prod))
    
    let fact = call_func(module_id, 2, [5])
    println("factorial(5) = " + str(fact))
    
    let pw = call_func(module_id, 3, [2, 10])
    println("power(2, 10) = " + str(pw))
    
    // 清理
    unload_shared_library(module_id)
}
```

### 关键差异（对比 Demo 1/2）

| 特性 | Demo 1/2 (C FFI) | Demo 3 (AOT 直调) |
|---|---|---|
| 编译参数 | `--cabi --shared` | `--shared` |
| 导出符号 | `aura_c_add` | `aura_aot_add!2!2!2!2` |
| 调用声明 | 需要 `extern "C" { ... }` | 不需要，运行时自动发现 |
| 类型系统 | 手动映射到 C 类型 | Aura 自动处理 |
| 函数索引 | 静态已知 | 运行时枚举 |
| 额外依赖 | 无 | 无 |

### 验证标准

- 加载成功，枚举到 2+ 个导出函数
- `call_func(module_id, 0, [3, 4])` → 7
- `call_func(module_id, 1, [3, 4])` → 12
- `call_func(module_id, 2, [5])` → 120
- `call_func(module_id, 3, [2, 10])` → 1024

---

## 完整构建流程

```bash
cd examples/ext_ffi_demo

# 1. 构建所有成员（默认构建全部）
loom build

# 2. 仅构建特定 demo
loom build --member demo-cffi
loom build --member demo-rustffi
loom build --member demo-aot-direct

# 3. 运行特定 demo
loom run --member demo-cffi
loom run --member demo-rustffi
loom run --member demo-aot-direct

# 4. 清理构建产物
loom clean

# 5. 运行测试（如有）
loom test
```

---

## 实施顺序

1. **Demo 3（AOT 直调）** — 验证逻辑 `aot_plugin_load.rs` 已存在，最快跑通
2. **Demo 1（C FFI）** — 复用 `--cabi` + `export-header`，需要写 `main.aura`
3. **Demo 2（Rust FFI）** — 与 Demo 1 相同，补充 Rust 绑定文件

---

## 依赖关系

| Demo | 编译器改动 | 外部依赖 | 已有验证 |
|---|---|---|---|
| C FFI | ❌ 无 | `clang`（已有） | `aot_c_abi.rs` example |
| Rust FFI | ❌ 无 | `cargo`（已有） | `aot_c_abi.rs` example |
| AOT 直调 | ❌ 无 | `clang` + `llc`（已有） | `aot_plugin_load.rs` example |

三个 demo 全部不需要改动编译器，复用已完成的 `--cabi`（Demo 1/2）和 `--shared`（Demo 3）基础设施。
