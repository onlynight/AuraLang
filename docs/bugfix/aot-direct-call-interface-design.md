# AOT 直调接口设计方案评估

## 现状问题

当前 `demo_aot_direct` 使用 `load_shared_library` + `call_func` 调用 AOT 编译产物：

```aura
val module_id = load_shared_library("target/build/libs/utils/utils.dll")
val sum = call_func(module_id, 0, listOf(3, 4))  // 魔法数字 0 = add
val fact = call_func(module_id, 1, listOf(5))     // 魔法数字 1 = factorial
val prod = call_func(module_id, 2, listOf(3, 4))  // 魔法数字 2 = multiply
val pw = call_func(module_id, 3, listOf(2, 10))   // 魔法数字 3 = power
```

**核心问题**：

| 问题 | 说明 |
|------|------|
| 函数索引是魔法数字 | `0, 1, 2, 3` 的含义依赖 PE 导出表顺序，不可读、不可维护 |
| 无类型安全 | `listOf(3, 4)` 不检查参数类型/个数 |
| 无函数名绑定 | 调用者必须知道导出顺序，无法通过名字引用函数 |
| 与 C FFI 接口不一致 | C FFI 用 `extern "C" { fun add(...) }` 声明式调用，AOT 直调用命令式调用 |

---

## 设计方案

### 方案 A：`extern "aura"` 声明式接口（推荐）

```aura
// 声明式：与 extern "C" 语法一致，语义为 AOT 直调
extern "aura" "utils" {
    fun add(a: Int, b: Int): Int
    fun multiply(a: Int, b: Int): Int
    fun factorial(n: Int): Int
    fun power(base: Int, exp: Int): Int
}

fun main() = {
    val sum = add(3, 4)          // 自动解析为 AOT 直调
    val prod = multiply(3, 4)
    val fact = factorial(5)
    val pw = power(2, 10)
}
```

### 方案 B：`module` + 方法调用

```aura
module utils = load_shared_library("target/build/libs/utils/utils.dll")

fun main() = {
    val sum = utils.add(3, 4)
    val prod = utils.multiply(3, 4)
}
```

### 方案 C：`@aot` 注解

```aura
@aot("target/build/libs/utils/utils.dll")
fun add(a: Int, b: Int): Int

fun main() = {
    val sum = add(3, 4)
}
```

---

## 方案对比

| 维度 | 方案 A: `extern "aura"` | 方案 B: `module` | 方案 C: `@aot` |
|------|------------------------|-----------------|----------------|
| 语法一致性 | ✅ 与 `extern "C"` 完全一致 | ❌ 新语法 | ⚠️ 新注解语法 |
| 实现复杂度 | 中（复用 extern 解析链路） | 高（新 AST + 方法分派） | 中（注解解析 + 函数提升） |
| 类型检查 | ✅ HIR 阶段可做 | ⚠️ 需动态类型检查 | ✅ 编译期检查 |
| 运行时开销 | 低（一次符号扫描，之后直接调用） | 中（每次调用需查模块） | 低 |
| 多库支持 | ✅ `extern "aura" "lib1" { ... }` | ✅ `module lib1 = ...` | ⚠️ 每函数一个注解 |
| 与现有代码兼容 | ✅ 不影响 `extern "C"` | ❌ 需新运行时支持 | ⚠️ 需编译器改造 |
| 调试体验 | ✅ 函数名在源码中可见 | ✅ 函数名在源码中可见 | ✅ 函数名在源码中可见 |

**结论：推荐方案 A** — 与现有 `extern "C"` 语法完全一致，实现复杂度最低，用户体验最佳。

---

## 方案 A 详细设计

### 1. 语法层

```
extern "aura" "utils" {
    fun add(a: Int, b: Int): Int
}
```

- `"aura"` 作为 ABI 标记字符串，与 `"c"`、`"rust"` 并列
- `"utils"` 作为库名，对应 `utils.dll` / `utils.so` / `utils.dylib`
- 块内函数声明与 `extern "C"` 完全一致

### 2. HIR 层

新增 `FfiAbi::Aura` 变体：

```rust
pub enum FfiAbi {
    None,
    C,
    Rust,
    Aura,  // 新增：AOT 直调
}
```

HIR 函数标记：

```rust
HirFunction {
    name: "add",
    params: [HirParam("a", Int), HirParam("b", Int)],
    ret: Some(Int),
    is_native: true,
    ffi_abi: FfiAbi::Aura,
    ffi_lib: Some("utils"),
    ...
}
```

### 3. VM 层调用链路

```
Aura 源码: add(3, 4)
    │
    ▼
Bytecode: CallNative(idx=0)   // idx = natives 中 add 的索引
    │
    ▼
VM: do_call_native(idx)
    │
    ├─ 检查 natives[idx].ffi_abi
    │
    ├─ FfiAbi::C    → static_call_c_with_lib()      // 现有路径
    ├─ FfiAbi::Rust → static_call_c_with_lib()      // 现有路径（同 C）
    └─ FfiAbi::Aura → call_aot_ffi()                // 新增路径
                        │
                        ▼
                   1. ensure_lib_loaded("utils")     // 加载 DLL
                   2. 扫描 aura_aot_* 导出符号
                   3. 匹配函数名 "add" → func_idx
                   4. AotRuntime::call_func(module_id, func_idx, args)
                   5. 返回结果
```

### 4. 函数名 → func_idx 映射

**核心挑战**：`call_func` 需要 `func_idx`，但 Aura 源码中只有函数名。

**解决方案**：在 `AotRuntime` 中维护 `module_id → (func_name → func_idx)` 映射表。

```rust
// AotRuntime 新增字段
pub func_name_map: HashMap<u32, HashMap<String, usize>>,
```

**填充时机**：`load_shared_library` 时，解析每个 `aura_aot_*` 符号名，提取函数名，建立映射。

```rust
// parse_aot_symbol_name 已有：
// "aura_aot_add!2!0!0!0" → ("add", 2, 0, [0, 0])

// 在 load_shared_library 中：
let (func_name, nargs, rettag, arg_tags) = parse_aot_symbol_name(&symbol_name)?;
self.func_name_map
    .entry(module_id)
    .or_default()
    .insert(func_name, func_idx);
```

### 5. 新增 VM 方法

```rust
// Vm 结构体新增方法
fn call_aot_ffi(&mut self, native: &BytecodeNative, args: &[Value]) -> Option<Value> {
    // 1. 确保库已加载
    self.ensure_aot_lib_loaded(native.ffi_lib.as_deref())?;
    
    // 2. 查找模块 ID
    let module_id = self.loaded_aot_libs.get(native.ffi_lib.as_deref()?)?;
    
    // 3. 查找函数索引
    let func_idx = self.aot_runtime
        .func_name_map
        .get(module_id)?
        .get(&native.name)?;
    
    // 4. 调用
    let jit_args: Vec<JitValue> = args.iter().map(JitValue::from_value).collect();
    unsafe { self.aot_runtime.call_func(*module_id, *func_idx, &jit_args).ok() }
        .map(JitValue::to_value)
}
```

### 6. 调用约定与类型

| 维度 | C FFI (`extern "C"`) | AOT 直调 (`extern "aura"`) |
|------|---------------------|---------------------------|
| 调用约定 | C ABI (ccc) | JitValue ABI |
| 参数传递 | `i64` 数组（最多 8 个） | `JitValue` 数组 |
| 返回值 | `i64` → `Value` | `JitValue` → `Value` |
| 类型检查 | VM 层手动映射 | VM 层自动（JitValue 带类型标签） |
| 函数指针 | `GetProcAddress`/`dlsym` | `dlsym` + 符号名解析 |
| 库加载 | `LoadLibraryW`/`dlopen` | `libloading::Library::new` |

### 7. 错误处理

| 错误场景 | 处理方式 |
|----------|----------|
| 库文件不存在 | 打印错误，返回 `Value::Null` |
| 库中无 `aura_aot_*` 导出 | 打印错误，返回 `Value::Null` |
| 函数名未找到 | 打印错误，返回 `Value::Null` |
| 调用失败（异常码） | 打印错误，返回 `Value::Null` |
| 参数类型不匹配 | 打印警告，尝试调用（JitValue ABI 容忍类型不匹配） |

---

## 实现清单

| 文件 | 修改内容 | 复杂度 |
|------|---------|--------|
| `codegen/opcode.rs` | `FfiAbi` 新增 `Aura` 变体 | 低 |
| `codegen/hir.rs` | HIR builder 处理 `extern "aura"` | 低 |
| `codegen/serialize.rs` | 序列化 `FfiAbi::Aura` | 低 |
| `vm/aot_runtime.rs` | 新增 `func_name_map` 字段 + `load_shared_library` 填充 | 中 |
| `vm/aot_runtime.rs` | 新增 `lookup_func_idx(module_id, name)` 方法 | 低 |
| `vm/interp.rs` | `do_call_native` 增加 `FfiAbi::Aura` 分支 | 中 |
| `vm/interp.rs` | 新增 `call_aot_ffi` 方法 | 中 |
| `vm/interp.rs` | 新增 `ensure_aot_lib_loaded` 方法 | 低 |
| `examples/ext_ffi_demo/demo_aot_direct/src/main.aura` | 改写为 `extern "aura"` 语法 | 低 |

**总复杂度：中**（约 5-8 小时实现）

---

## 可行性评估

### ✅ 可行

1. **语法层**：`extern "aura"` 与 `extern "C"` 语法完全一致，解析器无需修改
2. **HIR 层**：`FfiAbi::Aura` 新增变体，HIR builder 已有 extern 块处理逻辑
3. **VM 层**：`AotRuntime` 已有 `load_shared_library` + `call_func`，只需增加名称→索引映射
4. **符号解析**：`parse_aot_symbol_name` 已能解析 `aura_aot_<name>!<nargs>!<rettag>!<tag0>!...` 格式
5. **类型安全**：JitValue ABI 自带类型标签，比 C FFI 的 `i64` 数组更安全

### ⚠️ 风险

1. **导出顺序不稳定**：PE 导出表顺序可能因编译器版本变化，但名称映射不受影响
2. **多版本兼容**：旧版 `.auc` 文件无 `FfiAbi::Aura` 字段，需向后兼容处理
3. **跨平台**：Linux/macOS 的符号枚举逻辑不同，需测试

### ❌ 不可行

无明显不可行因素。所有依赖组件（`libloading`、`object` crate、`AotRuntime`）均已存在。

---

## 与 C FFI 的统一视角

```
extern "C" "utils"    →  C ABI 包装函数 (aura_c_*)    →  GetProcAddress  →  i64 数组
extern "aura" "utils" →  JitValue ABI 包装函数 (aura_aot_*)  →  dlsym  →  JitValue 数组
```

两者在源码层完全对称，仅在运行时解析路径不同：

| 阶段 | C FFI | AOT 直调 |
|------|-------|----------|
| 编译 | `--aot --shared --cabi` | `--aot --shared` |
| 导出符号 | `aura_c_add` | `aura_aot_add!2!0!0!0` |
| 符号解析 | `GetProcAddress("aura_c_add")` | `dlsym("aura_aot_add!...")` |
| 调用约定 | C ABI | JitValue ABI |
| 参数类型 | 手动映射 `i32`/`i64` | 自动（JitValue 标签） |
| 返回值 | 手动 `unpack` | 自动（JitValue → Value） |

---

## 结论

**方案 A（`extern "aura"`）可行且推荐**：

1. 语法与 `extern "C"` 完全一致，学习成本为零
2. 实现复杂度中等，核心依赖组件均已存在
3. 比 `call_func(module_id, 0, args)` 的魔法数字方式安全、可读、可维护
4. 与 C FFI 形成对称设计：`extern "C"` → C ABI，`extern "aura"` → JitValue ABI

建议按 `docs/bugfix/demo1-demo2-cabi-implementation.md` 的实现顺序推进，在完成 C FFI 修复后再实施此方案。
