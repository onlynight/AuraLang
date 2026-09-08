# Demo1/Demo2 C ABI FFI 实现方案

## 现状分析

### 问题 1：`--cabi` 是死标志（Dead Flag）

`AotOptions.c_abi` 字段在 `mod.rs:68` 定义、在 CLI 中通过 `--cabi` 设置，但**整个代码库中没有任何代码读取这个字段**。搜索 `c_abi` 仅命中定义和赋值，零处消费。

结果：`loom build --member utils --aot --shared --cabi` 与不带 `--cabi` 完全相同——只生成 JitValue ABI 包装函数（`aura_aot_add!2!0!0!0`），不生成 C ABI 包装函数（`aura_c_add`）。

### 问题 2：Demo 侧无库名绑定

Demo1/2 的 `extern "C" { fun add(...) }` 声明中**没有库名参数**：

```aura
// 当前写法 — 无库名
extern "C" {
    fun add(a: Int, b: Int): Int
}

// 语法支持但 Demo 未使用
extern "C" "utils" {
    fun add(a: Int, b: Int): Int
}
```

解析器 `parse_extern()` 在 `parser.rs:1879` 支持第二个字符串字面量作为库名，但 Demo 文件未提供。

结果：`ffi_lib = None` → VM 的 `do_call_native` 跳过 `ensure_lib_loaded`，无法加载 `utils.dll`。

### 问题 3：函数名不匹配

即使 DLL 被加载，`static_call_c_with_lib` 用 `GetProcAddress` 查找符号名 `add`。但 DLL 导出的符号是 `aura_aot_add!2!0!0!0`（JitValue ABI 包装函数），不是 `add` 也不是 `aura_c_add`。

结果：`GetProcAddress` 返回 `NULL` → `static_call_c_with_lib` 返回 `None` → VM 回退为 `Value::Int(0)`。

### 问题 4：无 C ABI 包装函数生成

AOT 代码生成器（`emit.rs`）的 `emit_function` 只生成 JitValue ABI 包装函数（`emit_wrapper`），不生成裸 C ABI 函数（`define c i32 @aura_c_add(i32, i32)`）。`FfiGenerator` 只生成 `declare` 声明，不生成 `define` 定义。

### 问题 5：`static_call_c_with_lib` 无泛型签名推断

`static_call_c_with_lib` 在 `interp.rs:877` 通过函数名硬编码匹配参数类型（`sqlura_*` 系列），其他函数统一用 `i64` 参数 + `i64` 返回。但 `add(a: Int, b: Int): Int` 在 LLVM 中是 `i32`，用 `i64` 调用会导致 ABI 不匹配。

---

## 总体架构

```
┌─────────────────────────────────────────────────────────────────────┐
│ 编译端：utils 库                                                   │
│                                                                     │
│ loom build --member utils --aot --shared --cabi                    │
│                                                                     │
│ HIR ──→ emit_program ──→ LLVM IR ──→ utils.dll                     │
│                              │                                      │
│                     ┌────────┴────────┐                             │
│                     │ c_abi = true    │                             │
│                     │                 │                             │
│                     ├─ emit_wrapper   │ (现有) JitValue ABI 包装   │
│                     │  aura_aot_add   │                             │
│                     │  !2!0!0!0       │                             │
│                     │                 │                             │
│                     ├─ emit_c_wrapper │ (新增) C ABI 裸函数        │
│                     │  aura_c_add     │                             │
│                     │  (i32, i32)→i32 │                             │
│                     └─────────────────┘                             │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│ 运行端：demo1/demo2 调用方                                          │
│                                                                     │
│ extern "C" "utils" {                                                │
│     fun add(a: Int, b: Int): Int                                    │
│ }                                                                   │
│                                                                     │
│ VM ──→ do_call_native ──→ resolve_symbol_in_lib ──→ 直接调用       │
│        (ffi_lib="utils")   GetProcAddress("aura_c_add")            │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 实现步骤

### 步骤 1：AOT 端 — C ABI 包装函数生成

**目标**：当 `c_abi = true` 时，为每个非 native 函数生成一个裸 C ABI 包装函数。

**修改文件**：`compiler/src/codegen/aot/emit.rs`

**C ABI 包装函数格式**：

```llvm
; 裸 C ABI 函数 — 直接暴露 Aura 函数的 C 调用约定
define dso_local dllexport i32 @"aura_c_add"(i32 %a, i32 %b) {
  %ret = call i32 @add(i32 %a, i32 %b)
  ret i32 %ret
}

; 带递归的函数
define dso_local dllexport i32 @"aura_c_factorial"(i32 %n) {
  %ret = call i32 @factorial(i32 %n)
  ret i32 %ret
}
```

**类型映射规则**：

| Aura 类型 | LLVM 类型 | C 类型（头文件） |
|-----------|-----------|-----------------|
| `Int` | `i32` | `int` |
| `Float` | `double` | `double` |
| `Boolean` | `i32` | `int` |
| `String` | `{ i8*, i64 }` | `const char *`（简化：只传指针） |
| 无返回 | `void` | `void` |

**Linkage**：`dso_local dllexport`（Windows）/ `dso_local visibility("default")`（Unix）

### 步骤 2：CLI 端 — 将 `c_abi` 传入代码生成

**修改文件**：`compiler/src/codegen/aot/mod.rs`

在 `compile()` 函数中，将 `c_abi` 标志传入 `emit_program`。

### 步骤 3：Demo 端 — 添加库名

**修改文件**：`demo_cffi/src/main.aura` 和 `demo_rustffi/src/main.aura`

```aura
// 修改后
extern "C" "utils" {
    fun add(a: Int, b: Int): Int
    fun multiply(a: Int, b: Int): Int
    fun factorial(n: Int): Int
    fun power(base: Int, exp: Int): Int
}
```

### 步骤 4：VM 端 — 修复函数名解析

**方案**：VM 在 `static_call_c_with_lib` 中自动添加 `aura_c_` 前缀。

```rust
// interp.rs — static_call_c_with_lib
let name_with_prefix = format!("aura_c_{}", name);
let addr = if let Some(handle) = lib_handle {
    resolve_symbol_in_lib(handle, &name_with_prefix)
} else {
    resolve_static_symbol(&name_with_prefix)
}?;
```

### 步骤 5：VM 端 — 修复参数/返回类型映射

**方案**：在 `BytecodeNative` 中增加参数类型和返回类型字段，从 HIR 传递到字节码。

**修改文件**：

1. `opcode.rs` — `BytecodeNative` 增加 `param_types: Vec<u8>` 和 `ret_type: u8`
2. `serialize.rs` — 序列化/反序列化新字段
3. `emit.rs` — 从 HIR 类型映射到类型 ID
4. `interp.rs` — `static_call_c_with_lib` 使用类型信息

**类型 ID 定义**：

```rust
// 类型 ID（u8）
pub const TYPE_ID_I32: u8 = 0;
pub const TYPE_ID_I64: u8 = 1;
pub const TYPE_ID_DOUBLE: u8 = 2;
pub const TYPE_ID_BOOL: u8 = 3;
pub const TYPE_ID_CSTRING: u8 = 4;
pub const TYPE_ID_PTR: u8 = 5;
pub const TYPE_ID_VOID: u8 = 6;
```

### 步骤 6：`export-header` 命令 — 修复头文件生成

与步骤 5 的类型映射保持一致，增加 `Int32`, `Double` 等映射。

---

## 修改文件清单

| 文件 | 修改内容 | 复杂度 |
|------|---------|--------|
| `codegen/aot/emit.rs` | 新增 `emit_c_abi_wrappers` 方法；`emit_program` 中调用 | 中 |
| `codegen/aot/mod.rs` | `compile()` 传入 `c_abi` 标志 | 低 |
| `codegen/opcode.rs` | `BytecodeNative` 增加 `param_types`/`ret_type` 字段 | 低 |
| `codegen/serialize.rs` | 序列化新字段 | 低 |
| `codegen/emit.rs` | 从 HIR 类型映射到类型 ID | 中 |
| `vm/interp.rs` | `static_call_c_with_lib` 使用类型信息 + `aura_c_` 前缀 | 中 |
| `vm/ffi.rs` | `CType` 增加 `Int32`/`Double` 变体 | 低 |
| `demo_cffi/src/main.aura` | `extern "C" "utils"` 添加库名 | 低 |
| `demo_rustffi/src/main.aura` | 同上 | 低 |

---

## 实现顺序

1. **步骤 1 + 步骤 2**：AOT 端生成 C ABI 包装函数 → 验证 `aura_c_add` 符号存在
2. **步骤 3**：Demo 端添加库名
3. **步骤 4**：VM 端添加 `aura_c_` 前缀
4. **步骤 5**：传递参数类型信息 → 验证返回值正确
5. **步骤 6**：完善 `export-header`

---

## 验证标准

```
$ loom build --member utils --aot --shared --cabi
$ loom build --member demo-cffi
$ loom run --member demo-cffi

add(3, 4) = 7
multiply(3, 4) = 12
factorial(5) = 120
power(2, 10) = 1024
```
