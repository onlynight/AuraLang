# AOT 直调接口设计：`interface` 关键字方案

## 设计动机

当前 `demo_aot_direct` 使用命令式调用：

```aura
// 问题：魔法数字、无类型安全、无函数名绑定
val module_id = load_shared_library("utils.dll")
val sum = call_func(module_id, 0, listOf(3, 4))
```

上一版方案用 `extern "aura"` 复用 extern 语法，但语义不够清晰——`extern` 的本意是"外部链接"，而 AOT 直调是"跨模块接口调用"，概念不同。

本方案提出独立关键字 `interface`，将"声明式绑定外部函数签名"从 `extern` 中剥离，形成清晰的语法分层：

| 关键字 | 语义 | 运行时路径 |
|--------|------|-----------|
| `extern "C"` | 外部 C 函数声明 | C ABI 调用 |
| `interface` | 跨模块接口绑定 | AOT JitValue ABI 调用 |

---

## 语法设计

### 基本形式

```aura
// 声明一个接口，绑定到 utils.dll 的 AOT 导出
interface Utils {
    fun add(a: Int, b: Int): Int
    fun multiply(a: Int, b: Int): Int
    fun factorial(n: Int): Int
    fun power(base: Int, exp: Int): Int
}
```

### 带库名的形式

```aura
// 显式指定库文件路径
interface Utils = "target/build/libs/utils/utils.dll" {
    fun add(a: Int, b: Int): Int
    fun multiply(a: Int, b: Int): Int
    fun factorial(n: Int): Int
    fun power(base: Int, exp: Int): Int
}
```

### 调用方式

**方式一：接口名.函数名（推荐）**

```aura
fun main() = {
    val sum = Utils.add(3, 4)
    val prod = Utils.multiply(3, 4)
    val fact = Utils.factorial(5)
    val pw = Utils.power(2, 10)
    println("add = " + toString(sum))
}
```

**方式二：直接调用（如果接口函数提升为全局）**

```aura
fun main() = {
    val sum = add(3, 4)  // 直接调用，编译器自动路由到 Utils 模块
}
```

推荐方式一，因为：
- 明确知道调用目标
- 支持多接口同名函数
- 与 Aura 的 class 方法调用风格一致（`ClassName.method()`）

---

## 语法层设计

### AST 节点

```rust
// ast.rs 新增
pub struct InterfaceDecl {
    pub name: String,              // 接口名，如 "Utils"
    pub lib_path: Option<String>,  // 库路径，None 时按模块名自动查找
    pub functions: Vec<FnDecl>,    // 函数声明列表
    pub span: Span,
}
```

### 解析规则

```
InterfaceDecl := 'interface' IDENT ['=' STRING] '{' FunctionDecl* '}'

FunctionDecl := 'fun' IDENT '(' ParamList? ')' ':' Type
ParamList    := Param (',' Param)*
Param        := IDENT (':' Type)?
```

### 与 extern 的区分

| 语法 | 解析器分支 | 语义 |
|------|-----------|------|
| `extern "C" { ... }` | `parse_extern_decl` | C ABI 外部函数 |
| `extern "rust" { ... }` | `parse_extern_decl` | Rust ABI 外部函数 |
| `interface Foo { ... }` | `parse_interface_decl`（新增） | AOT 模块接口绑定 |

---

## HIR 层设计

### 新增 HIR 节点

```rust
// hir.rs 新增
pub struct HirInterface {
    pub name: String,
    pub lib_path: Option<String>,
    pub functions: Vec<HirFunction>,  // 复用 HirFunction
}

// HirProgram 新增字段
pub struct HirProgram {
    // ... 现有字段 ...
    pub interfaces: Vec<HirInterface>,  // 新增
}
```

### 函数标记

接口内的函数在 HIR 中标记为：

```rust
HirFunction {
    name: "add",
    params: [HirParam("a", Int), HirParam("b", Int)],
    ret: Some(Int),
    is_native: true,
    ffi_abi: FfiAbi::Aura,        // 新增变体
    ffi_lib: Some("utils"),       // 库名（从 lib_path 提取）
    interface_name: Some("Utils"), // 新增：所属接口名
    ...
}
```

### FfiAbi 扩展

```rust
pub enum FfiAbi {
    None,
    C,
    Rust,
    Aura,  // 新增：AOT 直调
}
```

---

## VM 层设计

### 调用链路

```
Aura 源码: Utils.add(3, 4)
    │
    ▼
AST: MemberAccess { object: Var("Utils"), name: "add", args: [3, 4] }
    │
    ▼
HIR: Call { callee: "Utils.add", args: [...] }
    │
    ▼
Bytecode: CallNative(idx)   // idx = natives 中 "Utils.add" 的索引
    │
    ▼
VM: do_call_native(idx)
    │
    ├─ natives[idx].ffi_abi == FfiAbi::Aura
    │
    ▼
call_aot_ffi(native, args)
    │
    ├─ 1. ensure_lib_loaded("utils")          // 加载 DLL
    ├─ 2. load_shared_library("utils.dll")     // AotRuntime 加载模块
    ├─ 3. func_name_map[module_id]["add"]      // 名称→索引映射
    ├─ 4. AotRuntime::call_func(module_id, func_idx, jit_args)
    └─ 5. 返回 JitValue → Value
```

### AotRuntime 扩展

```rust
// aot_runtime.rs 新增
impl AotRuntime {
    /// 函数名 → func_idx 映射表（module_id → {func_name → func_idx}）
    pub func_name_map: HashMap<u32, HashMap<String, usize>>,
    
    /// 按名称查找函数索引
    pub fn lookup_func_idx(&self, module_id: u32, func_name: &str) -> Option<usize> {
        self.func_name_map.get(&module_id)?.get(func_name).copied()
    }
}
```

### Vm 扩展

```rust
// interp.rs 新增
impl Vm {
    /// 已加载的 AOT 模块映射（库名 → module_id）
    aot_module_map: HashMap<String, u32>,
    
    /// 确保 AOT 库已加载
    fn ensure_aot_lib_loaded(&mut self, lib_name: &str) -> Result<u32, String> {
        if let Some(&id) = self.aot_module_map.get(lib_name) {
            return Ok(id);
        }
        let lib_path = self.resolve_lib_path(lib_name)?;
        let module_id = self.aot_runtime.load_shared_library(&lib_path)?;
        self.aot_module_map.insert(lib_name.to_string(), module_id);
        Ok(module_id)
    }
    
    /// AOT 接口调用
    fn call_aot_ffi(&mut self, native: &BytecodeNative, args: &[Value]) -> Option<Value> {
        let lib_name = native.ffi_lib.as_deref()?;
        let module_id = self.ensure_aot_lib_loaded(lib_name).ok()?;
        let func_name = &native.name;
        let func_idx = self.aot_runtime.lookup_func_idx(module_id, func_name)?;
        let jit_args: Vec<JitValue> = args.iter().map(JitValue::from_value).collect();
        unsafe { self.aot_runtime.call_func(module_id, func_idx, &jit_args).ok() }
            .map(JitValue::to_value)
    }
}
```

### do_call_native 扩展

```rust
// interp.rs 修改
fn do_call_native(&mut self, top: usize, idx: usize) -> Result<(), VmError> {
    // ... 现有代码 ...
    
    let result = if let Some(f) = self.natives.get(&native.name) {
        f(&args)
    } else if let Some(f) = self.natives.resolve_c_function(&native.name) {
        f(&args)
    } else {
        match native.ffi_abi {
            FfiAbi::C | FfiAbi::Rust => {
                static_call_c_with_lib(&native.name, &args, lib_handle, &native.param_types, native.ret_type)
                    .unwrap_or_else(|| {
                        eprintln!("[vm] 未链接的外部函数 `{}`", native.name);
                        Value::Int(0)
                    })
            }
            FfiAbi::Aura => {
                self.call_aot_ffi(&native, &args).unwrap_or_else(|| {
                    eprintln!("[vm] AOT 接口调用失败: `{}`", native.name);
                    Value::Int(0)
                })
            }
            FfiAbi::None => {
                eprintln!("[vm] 未定义的函数 `{}`", native.name);
                Value::Int(0)
            }
        }
    };
    self.frames[top].stack.push(result);
    Ok(())
}
```

---

## 函数名 → func_idx 映射

### 符号名解析

`AotRuntime::load_shared_library` 已能扫描 `aura_aot_*` 导出符号，并用 `parse_aot_symbol_name` 解析：

```
"aura_aot_add!2!0!0!0" → ("add", nargs=2, rettag=0, arg_tags=[0,0])
```

### 映射表填充

```rust
// aot_runtime.rs: load_shared_library 中
for (func_idx, (symbol_name, entry)) in symbol_entries.iter().enumerate() {
    if let Some((func_name, nargs, rettag, arg_tags)) = parse_aot_symbol_name(symbol_name) {
        self.func_name_map
            .entry(module_id)
            .or_default()
            .insert(func_name, func_idx);
    }
}
```

### 类型兼容性检查（可选增强）

可以在调用时检查 Aura 源码声明的参数类型是否与 DLL 导出的 JitValue 类型标签匹配：

```rust
fn check_type_compatibility(
    declared_params: &[HirType],
    exported_arg_tags: &[u8],
) -> bool {
    // 比较声明类型和导出类型标签
    // 不匹配时打印警告但不阻止调用
}
```

---

## 错误处理

| 错误场景 | 处理方式 | 用户可见输出 |
|----------|----------|-------------|
| 库文件不存在 | 返回 `Value::Null` | `[vm] AOT 库加载失败: utils.dll not found` |
| 库中无 `aura_aot_*` 导出 | 返回 `Value::Null` | `[vm] AOT 库中无有效导出: utils.dll` |
| 函数名未找到 | 返回 `Value::Null` | `[vm] 接口函数未找到: Utils.add` |
| 调用异常 | 返回 `Value::Null` | `[vm] AOT 调用异常: Utils.add (code=42)` |
| 参数类型不匹配 | 警告但继续 | `[vm] 警告: Utils.add 参数类型不匹配` |

---

## 实现清单

| 文件 | 修改内容 | 复杂度 |
|------|---------|--------|
| `ast.rs` | 新增 `InterfaceDecl` 结构体 | 低 |
| `parser.rs` | 新增 `parse_interface_decl` 方法 | 低 |
| `hir.rs` | 新增 `HirInterface` + HIR builder 处理 interface | 中 |
| `codegen/opcode.rs` | `FfiAbi` 新增 `Aura` 变体 | 低 |
| `codegen/serialize.rs` | 序列化 `FfiAbi::Aura` | 低 |
| `codegen/emit.rs` | 从 interface 生成 `BytecodeNative` 条目 | 中 |
| `vm/aot_runtime.rs` | 新增 `func_name_map` + `lookup_func_idx` | 低 |
| `vm/interp.rs` | `do_call_native` 增加 `FfiAbi::Aura` 分支 | 中 |
| `vm/interp.rs` | 新增 `call_aot_ffi` + `ensure_aot_lib_loaded` | 中 |
| `examples/demo_aot_direct/src/main.aura` | 改写为 `interface` 语法 | 低 |

**总复杂度：中**（约 8-12 小时实现）

---

## 与 `extern "C"` 的统一视角

```
extern "C" "utils" { ... }    →  C ABI 包装函数 (aura_c_*)    →  GetProcAddress  →  i64 数组
interface Utils { ... }        →  JitValue ABI 包装函数 (aura_aot_*)  →  dlsym  →  JitValue 数组
```

| 维度 | `extern "C"` | `interface` |
|------|-------------|------------|
| 关键字 | `extern` | `interface` |
| 语义 | 外部链接声明 | 模块接口绑定 |
| 编译参数 | `--aot --shared --cabi` | `--aot --shared` |
| 导出符号 | `aura_c_add` | `aura_aot_add!2!0!0!0` |
| 调用约定 | C ABI | JitValue ABI |
| 类型安全 | 手动映射 | 自动（JitValue 标签） |
| 函数命名 | 直接调用 `add(3, 4)` | 接口调用 `Utils.add(3, 4)` |
| 多模块支持 | 多个 `extern` 块 | 多个 `interface` 块 |

---

## 扩展性

### 未来可能的扩展

1. **多库接口**：

```aura
interface Utils = "utils.dll" {
    fun add(a: Int, b: Int): Int
}

interface Math = "math.dll" {
    fun sin(x: Float): Float
}
```

2. **接口继承**：

```aura
interface BaseUtils {
    fun add(a: Int, b: Int): Int
}

interface ExtendedUtils extends BaseUtils = "utils_v2.dll" {
    fun multiply(a: Int, b: Int): Int
}
```

3. **接口默认实现**：

```aura
interface Utils {
    fun add(a: Int, b: Int): Int
    fun add10(a: Int): Int = add(a, 10)  // 默认实现，引用接口内其他函数
}
```

4. **C FFI 接口化**（统一 extern 和 interface）：

```aura
// 未来可能：用 interface 统一 C FFI
interface CUtils = "utils.dll" [abi="C"] {
    fun add(a: Int, b: Int): Int
}
```

---

## 可行性评估

### ✅ 可行

1. **语法层**：`interface` 是独立关键字，不影响现有 `extern` 解析
2. **AST/HIR 层**：新增结构体，与现有 `ExternDecl` 并列
3. **VM 层**：复用 `AotRuntime` 现有基础设施，只需增加名称→索引映射
4. **符号解析**：`parse_aot_symbol_name` 已能提取函数名
5. **类型安全**：JitValue ABI 自带类型标签

### ⚠️ 风险

1. **接口名冲突**：多个 interface 声明同名函数时，需通过 `Utils.add` vs `Math.add` 区分
2. **库路径解析**：`lib_path` 为 `None` 时，按模块名自动查找的逻辑需定义
3. **热重载**：运行中 DLL 被替换时的行为需定义（建议不支持热重载）

### ❌ 不可行

无明显不可行因素。

---

## 结论

**`interface` 关键字方案可行且优于 `extern "aura"`**：

1. **语义更清晰**：`interface` = "声明跨模块接口"，`extern` = "外部链接声明"，概念分离
2. **调用更直观**：`Utils.add(3, 4)` 比 `add(3, 4)` 明确知道调用目标
3. **扩展性更好**：未来可支持接口继承、默认实现、多库绑定
4. **实现复杂度可控**：约 8-12 小时，核心依赖组件均已存在
5. **与 C FFI 形成对称设计**：`extern "C"` → C ABI，`interface` → JitValue ABI
