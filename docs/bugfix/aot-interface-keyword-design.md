# AOT 直调接口设计：`extern interface` 关键字方案

> **⚠️ 已废弃**：`extern interface` 已统一为 `extern object` + `@aot` 注解。
> 请参考 `docs/aura_cffi_impl/design.md` 中的最新规范。
> 本文件保留作为历史参考。

## 设计动机

当前 `demo_aot_direct` 使用命令式调用：

```aura
// 问题：魔法数字、无类型安全、无函数名绑定
val module_id = load_shared_library("utils.dll")
val sum = call_func(module_id, 0, listOf(3, 4))
```

本方案提出 `extern interface` 关键字，将"声明式绑定外部函数签名"从 `extern` 中剥离，形成清晰的语法分层：

| 关键字 | 语义 | 运行时路径 |
|--------|------|-----------|
| `extern "C"` | 外部 C 函数声明 | C ABI 调用（`GetProcAddress` / `dlsym`） |
| `extern "Rust"` | 外部 Rust 函数声明 | C ABI 调用（兼容 Rust `extern "C"`） |
| `extern interface` | 跨模块 AOT 接口绑定 | JitValue ABI 直调（DLL 内嵌 Aura 机器码） |

---

## 语法设计

### 基本形式

```aura
// 库路径通过 default fun loadLibrary() 声明
extern interface Utils {
    default fun loadLibrary(): String = "utils"
    fun add(a: Int, b: Int): Int
    fun multiply(a: Int, b: Int): Int
    fun factorial(n: Int): Int
    fun power(base: Int, exp: Int): Int
}
```

### 调用方式

```aura
fun main() = {
    val sum = Utils.add(3, 4)
    val prod = Utils.multiply(3, 4)
    val fact = Utils.factorial(5)
    val pw = Utils.power(2, 10)
    println("add = " + toString(sum))
}
```

### 方法顺序

**接口方法顺序无固定要求**，可以任意调整。运行时按**名称**查找函数，非索引：

```aura
// 以下两种写法等价
extern interface Utils {
    default fun loadLibrary(): String = "utils"
    fun add(a: Int, b: Int): Int
    fun multiply(a: Int, b: Int): Int
}

extern interface Utils {
    default fun loadLibrary(): String = "utils"
    fun multiply(a: Int, b: Int): Int
    fun add(a: Int, b: Int): Int
}
```

---

## 独立接口文件 + import 引入

`extern interface` 声明可以放在独立 `.aura` 文件中，通过 `import` 引入：

### utils_interface.aura（独立声明文件）

```aura
// utils_interface.aura — extern interface 声明
extern interface Utils {
    default fun loadLibrary(): String = "utils"
    fun add(a: Int, b: Int): Int
    fun multiply(a: Int, b: Int): Int
    fun factorial(n: Int): Int
    fun power(base: Int, exp: Int): Int
}
```

### main.aura（导入使用）

```aura
// 从独立文件导入 extern interface 声明
import "utils_interface.aura"

fun main() = {
    val sum = Utils.add(3, 4)
    println("add = " + toString(sum))
}
```

**预处理机制**：编译前扫描 `import "xxx.aura"` 语句，读取文件内容内联到源码中。

---

## 库查找机制

### 库名声明

`loadLibrary()` 返回**库名**（base name），非完整路径，VM 自动补全扩展名和搜索路径：

```aura
default fun loadLibrary(): String = "utils"  // 库名，非 "libs/utils.dll"
```

### 查找顺序（平台感知）

**Windows：**

| 优先级 | 路径 |
|--------|------|
| 1 | `libs/<name>.dll` |
| 2 | `<name>.dll` |
| 3 | `target/build/libs/<name>/<name>.dll` |

**Unix (Linux/macOS)：**

| 优先级 | 路径 |
|--------|------|
| 1 | `libs/lib<name>.so` |
| 2 | `lib<name>.so` |
| 3 | `libs/lib<name>.dylib` |
| 4 | `lib<name>.dylib` |
| 5 | `target/build/libs/<name>/lib<name>.so` |

### 约定目录

`libs/` 为默认库目录，所有动态库应放置于此：

```
project/
├── libs/
│   └── utils.dll          ← Windows
│   └── libutils.so        ← Linux
│   └── libutils.dylib     ← macOS
├── src/
│   └── main.aura
```

---

## 与 `extern "C"` 的对比

| 维度 | `extern "C"` | `extern interface` |
|------|-------------|-------------------|
| **关键字** | `extern "C"` | `extern interface` |
| **库名声明** | `extern "C" "utils"` | `default fun loadLibrary(): String = "utils"` |
| **函数声明** | `fun add(...)`（无返回值类型绑定） | `fun add(...): Int`（带返回类型） |
| **调用方式** | `add(3, 4)` | `Utils.add(3, 4)` |
| **导出符号** | `aura_c_add` | `aura_aot_add!2!0!0!0` |
| **调用约定** | C ABI | JitValue ABI |
| **类型安全** | 手动映射 C 类型 | 自动（JitValue 标签） |
| **编译参数** | `--aot --shared --cabi` | `--aot --shared` |
| **接口文件** | `export-header` → `.h` | 独立 `.aura` 文件 + `import` |
| **函数解析** | `GetProcAddress` / `dlsym` | `func_name_map` 名称→索引映射 |

---

## 语法层设计

### AST 节点

```rust
pub struct ExternInterfaceDecl {
    pub name: String,              // 接口名，如 "Utils"
    pub lib_path: Option<String>,  // 从 loadLibrary() 提取，None 时按模块名自动查找
    pub functions: Vec<FnDecl>,    // 函数声明列表（包含 loadLibrary 本身）
    pub span: Span,
}
```

### FnModifier 扩展

```rust
pub enum FnModifier {
    // ... 现有变体
    Default,  // 默认实现（extern interface 内 loadLibrary 使用）
}
```

### 解析规则

```
ExternInterfaceDecl := 'extern' 'interface' IDENT '{' FunctionDecl* '}'

FunctionDecl := ('default'?) 'fun' IDENT '(' ParamList? ')' ':' Type ('=' Expr)?
ParamList    := Param (',' Param)*
Param        := IDENT (':' Type)?

// loadLibrary 特殊处理：
//   default fun loadLibrary(): String = "libname"
//   → 提取 libname 作为 lib_path
//   → 不注册为接口函数（内部方法）
```

### 编译期校验

`extern interface` **必须**包含 `default fun loadLibrary(): String = "..."` 方法，否则编译报错：

```
error: extern interface `Utils` 必须包含 `default fun loadLibrary(): String = "..."` 方法
```

---

## 运行时调用链路

```
Aura 源码: Utils.add(3, 4)
    │
    ▼
AST: MemberAccess { object: Ident("Utils"), name: "add", args: [3, 4] }
    │
    ▼
HIR: Call { callee: "Utils.add", args: [...] }
    │
    ▼
Bytecode: CallNativeArgs(idx, argc)
    │
    ▼
VM: do_call_native_args(idx, argc)
    │
    ├─ natives[idx].ffi_abi == FfiAbi::Aura
    │
    ▼
call_aot_ffi(native, args)
    │
    ├─ 1. lib_name = native.ffi_lib          // "utils"
    ├─ 2. ensure_aot_lib_loaded("utils")     // 查找 libs/utils.dll
    ├─ 3. AotRuntime::load_shared_library()  // 解析 aura_aot_* 导出符号
    ├─ 4. func_name_map[module_id]["add"]    // 名称→索引映射
    ├─ 5. AotRuntime::call_func(module_id, func_idx, jit_args)
    └─ 6. 返回 JitValue → Value
```

---

## HIR 层设计

### FfiAbi 扩展

```rust
pub enum FfiAbi {
    None,   // 非 FFI 函数
    C,      // C ABI
    Rust,   // Rust 库（语法标记，调用约定同 C）
    Aura,   // AOT 直调（JitValue ABI）
}
```

### HIR Builder 处理

```rust
Decl::ExternInterface(e) => {
    // 跳过 loadLibrary（内部方法）
    for f in &e.functions {
        if f.name == "loadLibrary" { continue; }
        natives.push(HirFunction {
            name: format!("{}.{}", e.name, f.name),
            ffi_abi: FfiAbi::Aura,
            ffi_lib: e.lib_path.clone(),  // 从 loadLibrary() 提取
            // ...
        });
    }
}
```

---

## VM 层设计

### 关键方法

| 方法 | 职责 |
|------|------|
| `ensure_aot_lib_loaded(lib_name)` | 查找并加载 AOT 动态库，构建 `func_name_map` |
| `call_aot_ffi(native, args)` | 按名称查找函数索引，JitValue ABI 直调 |
| `candidate_lib_paths(lib_name)` | 平台感知的库候选路径列表 |

### 函数解析（按名称，非索引）

```rust
// aot_runtime.rs
func_name_map: HashMap<u32, HashMap<String, usize>>

// load_shared_library: 解析 aura_aot_* 导出符号
for (func_idx, (name, ...)) in symbols.iter().enumerate() {
    if let Some((func_name, ...)) = parse_aot_symbol_name(name) {
        self.func_name_map.entry(module_id).or_default().insert(func_name, func_idx);
    }
}

// lookup_func_idx: 按名称查找
pub fn lookup_func_idx(&self, module_id: u32, func_name: &str) -> Option<usize> {
    self.func_name_map.get(&module_id)?.get(func_name).copied()
}
```

---

## 构建命令

### 编译 AOT 动态库

```bash
# 生成 JitValue ABI 导出（供 extern interface 调用）
aura build libs/utils/src/lib.aura --aot --shared --output utils.dll

# 库文件放置到 libs/ 目录
cp utils.dll libs/utils.dll
```

### 运行调用方

```bash
# 源码中 import "utils_interface.aura" 引用接口声明
aura run src/main.aura
```

---

## 构建配置（aura.toml）

库的 FFI 导出方式通过 `aura.toml` 配置，默认 AOT Aura 直连模式：

```toml
[build]
ffi-mode = "aot"  # 默认：AOT Aura 直连（JitValue ABI）
# ffi-mode = "cabi"  # 可选：C ABI 导出（供外部 C/Rust 调用）
```

### FFI 模式对比

| 模式 | 导出符号 | 调用方式 | 使用场景 |
|------|---------|---------|---------|
| `aot`（默认） | `aura_aot_add!2!0!0!0` | `extern interface` + JitValue ABI | Aura 内部模块调用 |
| `cabi` | `aura_c_add` | `extern "C"` + C ABI | 外部 C/Rust 调用 |

### 示例：utils 库

```toml
# examples/ext_ffi_demo/libs/utils/aura.toml
[build]
opt-level = 2
debug = true
out-dir = "target/build"
cache-dir = "target/cache"
parallel = true
ffi-mode = "aot"  # AOT Aura 直连
```

### 构建命令

```bash
# 方式 1：使用 aura CLI 直接构建
aura build libs/utils/src/lib.aura --aot --shared --output libs/utils.dll

# 方式 2：使用 loom 构建系统（需启用 llvm feature）
loom build --member utils

# 库文件输出到 libs/ 目录，VM 自动查找
```

---

## 实现清单

| 文件 | 修改内容 | 状态 |
|------|---------|------|
| `compiler/src/ast.rs` | `FnModifier::Default` + `ExternInterfaceDecl` | ✅ |
| `compiler/src/parser.rs` | `parse_extern_interface`（`loadLibrary` 提取） | ✅ |
| `compiler/src/sema/checker.rs` | 校验 `loadLibrary` 必须存在 | ✅ |
| `compiler/src/codegen/hir.rs` | 跳过 `loadLibrary`，设置 `FfiAbi::Aura` | ✅ |
| `compiler/src/codegen/opcode.rs` | `FfiAbi::Aura` 变体 | ✅ |
| `compiler/src/codegen/serialize.rs` | 序列化 `FfiAbi::Aura`（byte 3） | ✅ |
| `compiler/src/codegen/emit.rs` | 生成 `BytecodeNative` 条目 | ✅ |
| `compiler/src/codegen/mod.rs` | `resolve_aura_imports` 预处理 | ✅ |
| `compiler/src/vm/aot_runtime.rs` | `func_name_map` + `lookup_func_idx` | ✅ |
| `compiler/src/vm/interp.rs` | `call_aot_ffi` + `ensure_aot_lib_loaded` + `candidate_lib_paths` | ✅ |
| `compiler/src/vm/interp.rs` | `do_call_native_args` 增加 `FfiAbi::Aura` 分支 | ✅ |
| `cli/src/main.rs` | `cmd_run` / `cmd_build` 集成预处理 | ✅ |
| `examples/.../utils_interface.aura` | 独立接口声明文件 | ✅ |
| `examples/.../main.aura` | `import` 引入 + 调用 | ✅ |

---

## 关键决策记录

### 1. `loadLibrary()` 而非 `= "path"`

**原因**：与 `extern "C" "utils"` 保持一致，库名是 base name 而非完整路径。VM 统一处理路径补全。

### 2. `default fun` 而非 `default loadLibrary =`

**原因**：`default fun` 遵循现有函数语法，有返回类型和函数体，语义清晰。

### 3. 按名称查找而非索引

**原因**：DLL 导出表符号顺序由链接器决定（通常按字母序），与接口声明顺序无关。按名称查找保证接口方法顺序可任意调整。

### 4. `libs/` 目录约定

**原因**：提供统一的库放置位置，避免硬编码路径。C FFI 和 AOT FFI 共享同一查找逻辑。

### 5. `import "xxx.aura"` 而非 `include`

**原因**：`import` 是语言级关键字，编译器预处理时读取文件内联，支持独立接口文件。

---

## 与现有 FFI 的关系

```
                    ┌─────────────────────────────────────────┐
                    │            Aura 源码                     │
                    │                                         │
                    │  extern "C" "utils" { ... }             │
                    │  extern interface Utils { ... }         │
                    │                                         │
                    └────────────┬────────────────────────────┘
                                 │
                    ┌────────────▼────────────────────────────┐
                    │           VM 运行时                      │
                    │                                         │
                    │  FfiAbi::C  → ensure_lib_loaded         │
                    │                  → LoadLibraryW          │
                    │                  → GetProcAddress        │
                    │                  → C ABI 调用            │
                    │                                         │
                    │  FfiAbi::Aura → ensure_aot_lib_loaded   │
                    │                  → candidate_lib_paths   │
                    │                  → AotRuntime::load      │
                    │                  → func_name_map 查找    │
                    │                  → JitValue ABI 直调     │
                    └─────────────────────────────────────────┘
```

| | `extern "C"` | `extern interface` |
|---|---|---|
| **库查找** | `ensure_lib_loaded` | `ensure_aot_lib_loaded` |
| **加载方式** | `LoadLibraryW` / `dlopen` | `AotRuntime::load_shared_library` |
| **符号解析** | `GetProcAddress` / `dlsym` | `aura_aot_*` 符号名解析 |
| **调用约定** | C ABI（手动类型映射） | JitValue ABI（自动类型标签） |
| **共享查找逻辑** | `libs/` 目录 + 平台扩展名 | ✅ 一致 |
