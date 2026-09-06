# Rust FFI 设计方案

> **目标**：为 Aura 增加对 Rust 库的**原生亲和** FFI 支持，在 AOT 模式下**保持与 C FFI 同等的 0 开销**，且**不依赖未稳定的 Rust 默认 ABI**。
>
> **范围**：本方案仅覆盖**设计**，不修改现有代码。所有变更点用"建议修改"标记，实施前需评审。
>
> **版本历史**：
> - v1.0：初版，提出 `extern "rust"` 使用 Rust 默认调用约定
> - **v1.1（当前）**：修正"Rust 默认 ABI 官方未稳定"风险，`extern "rust"` **默认走 C ABI**，"原生亲和"重新定义为语法/工具/库发现/文档层面

---

## 1. 设计目标与"0 开销"边界

### 1.1 目标

| 目标 | 含义 |
|------|------|
| **原生亲和 Rust** | Rust 作为**一等公民**：独立语法、工具链识别、库发现、独立文档。**不等于**使用不稳定的 Rust 默认 ABI |
| **AOT 0 开销** | 与 C FFI 一样：LLVM IR `declare @foo` 直接调用，无中间层、无参数装箱 |
| **VM 兼容** | VM 后端走既有 i64 装箱调度，不引入新的性能模型 |
| **语法清晰** | `extern "c"` 与 `extern "rust"` 语法上区分，语义上对称 |
| **向后兼容** | 现有 `extern "c"` 代码、C 库绑定零改动 |
| **跨版本稳定** | **不依赖 Rust 默认 ABI**（官方未稳定），全程走 C ABI（官方稳定） |

### 1.2 "0 开销"的严格定义

| 后端 | 0 开销含义 | C FFI 现状 | Rust FFI 目标 |
|------|-----------|-----------|-------------|
| **AOT** | LLVM IR 直接外部调用，寄存器传参，无中间层 | ✅ `declare ... @foo(ccc)` | ✅ `declare ... @foo(ccc)` |
| **VM** | 参数装箱为 i64 后通过 `CFuncPtr` 调用（VM 固有开销，非 FFI 开销） | ⚠️ 走 i64 装箱（已接受） | ⚠️ 同左，**不引入额外开销** |
| **JIT** | 编译为机器码后走 AOT 等价路径 | — | — |

> **重要前提**：Rust FFI 的"0 开销"等价于 C FFI 的"0 开销"，指的是**调用层无中间转换**。Rust 库本身的执行效率由 rustc 决定，与本设计无关。

### 1.3 "原生亲和"的重新定义

**v1.0 误解**：把"原生亲和"等同于"使用 Rust 默认调用约定"——这是错的，因为 Rust 默认 ABI 官方未稳定。

**v1.1 正确定义**：

| 亲和维度 | 具体表现 |
|---------|---------|
| **语法清晰** | `extern "rust"` 独立关键字，一目了然区别于 `extern "c"` |
| **工具链识别** | IDE/LSP 看到 `extern "rust"` 提示用户补 `#[no_mangle] extern "C"` |
| **库发现** | 自动识别 Rust dylib（`libfoo.so` / `.rlib` / `.dll`） |
| **文档独立** | Rust 用户对接文档与 C 用户文档分离 |
| **错误预防** | 编译器可校验：`extern "rust"` 块的函数在 Rust 侧必须 `#[no_mangle] extern "C"` |
| **未来预留** | 若 Rust 官方稳定默认 ABI，可无破坏性切换 |

**核心立场**：`extern "rust"` **使用 C ABI**（官方稳定），**不使用** Rust 默认 ABI（官方未稳定）。

---

## 2. 现状回顾：C FFI 为何 0 开销

### 2.1 完整调用链路

```
Aura 源码 (extern "c" "raylib" { fun DrawCircle(...) })
    ↓
AST: ExternDecl { abi: "c", library: Some("raylib"), functions: [...] }
    ↓
HIR: HirFunction { is_native: true, name: "DrawCircle", ... }
    ↓
AOT: FfiGenerator → LLVM IR "declare void @DrawCircle(i32, i32, float, i32)"
    ↓
llc/lld: 链接 raylib 动态库 → 机器码直接调用
```

### 2.2 0 开销的关键

- **无装箱**：AOT 下参数直接以寄存器/栈传递，不经过 `CType::pack/unpack`
- **无蹦床**：调用者→被调用者是直接 `call @foo` 指令
- **符号解析**：由链接器（lld/ld）静态解析，零运行时开销

### 2.3 现状缺陷

| 缺陷 | 说明 |
|------|------|
| `abi` 字段未区分 | `ExternDecl.abi` 存字符串但未在 HIR/AOT 层使用，目前所有 extern 一律走 C ABI |
| `library` 字段未使用 | `ExternDecl.library` 存在但 AOT 层未利用（链接由外部 lld 处理） |
| 无 Rust 亲和 | Rust 库必须用 `#[no_mangle] extern "C"` 才能被调用，但 Aura 无独立语法提示 |

---

## 3. 设计权衡：Rust ABI 的稳定性边界

### 3.1 Rust ABI 现状（截至 Rust 1.8x）

| 维度 | 默认（无 `extern "C"`） | `extern "C"` + `#[no_mangle]` |
|------|------------------------|------------------------------|
| **符号名** | v0 mangling，`_RNvC3foo3bar...` | 与源码名一致 |
| **调用约定** | 平台本地（x86-64: Rust `x86_64-unknown`） | C ABI（`ccc`） |
| **结构体布局** | 未稳定，rustc 可优化 | `#[repr(C)]` 可显式稳定 |
| **字符串/Vec** | `String`/`Vec<T>`/`&str` 不可 FFI | 用 `*const c_char` 等 C 类型 |
| **官方稳定性** | ⚠️ **未稳定**（Rust Reference 明确说会变化） | ✅ **稳定**（Rust 1.0 起未变，跨版本承诺） |
| **实际稳定性** | 10+ 年未变，但**无官方保证** | ✅ 官方保证 |

> **关键事实**：Rust Reference 原话——"The ABI of Rust functions without an explicit `extern` block is subject to change"。这是**官方承诺**，设计不能依赖"实际多年未变"。

### 3.2 设计决策（v1.1 修正）

**核心立场**：`extern "rust"` 默认使用 **C ABI**（官方稳定），**不使用** Rust 默认 ABI（官方未稳定）。

| 决策 | 选择 | 理由 |
|------|------|------|
| ✅ 支持 `extern "rust"` 关键字 | 保留 | 语法清晰、工具链识别、库发现 |
| ✅ `extern "rust"` 默认走 C ABI | 采用 | 官方稳定，跨 Rust 版本不失效 |
| ✅ Rust 侧**强制** `#[no_mangle] extern "C"` | 文档明确 + 编译器警告 | 唯一官方稳定的路径 |
| ❌ 不使用 Rust 默认调用约定 | 删除 | 官方未稳定，风险不可接受 |
| ❌ 不实现 Rust v0 mangling 解码 | 不实施 | 收益低、风险高、Rust 侧未承诺稳定 |
| ❌ 不支持 `String`/`Vec`/`Box` 跨边界 | 文档禁止 | 类型不安全 |
| ❌ 不支持 Rust 默认布局的 `struct` | 文档禁止 | 布局不稳定，需 `#[repr(C)]` |

### 3.3 与"0 开销"的关系

| Rust 侧写法 | 调用约定 | 是否 0 开销 | 是否稳定 |
|------------|---------|------------|---------|
| `#[no_mangle] extern "C" pub fn foo()` | C ABI | ✅ 是 | ✅ 官方稳定 |
| `#[no_mangle] pub fn foo()`（无 `extern "C"`） | Rust 默认 | ✅ 是 | ⚠️ 官方未稳定 |

**结论**：
- 两种写法都是 0 开销（AOT 下直接调用，无中间层）
- 但**只有 `extern "C"` 是官方稳定的**
- Aura 强制走 C ABI 路径，确保跨 Rust 版本不失效

### 3.4 为什么不直接用 `extern "c"`？

如果 `extern "rust"` 走 C ABI，与 `extern "c"` 完全等价，为何还要 `extern "rust"`？

| 维度 | `extern "c"` | `extern "rust"` |
|------|--------------|-----------------|
| 调用约定 | C ABI | C ABI（相同） |
| 符号解析 | 通用 | 同左 |
| **语义提示** | "这是 C 库" | "这是 Rust 库" |
| **工具链提示** | 无 Rust 特定提示 | IDE 提示补 `#[no_mangle] extern "C"` |
| **库发现** | 通用 dylib | 自动识别 Rust dylib（`.rlib`/`.so`） |
| **错误预防** | 可能误用 Rust 默认 ABI | 编译器可校验 Rust 侧标注 |
| **文档独立** | 混在 C FFI 文档 | 独立文档，Rust 用户对接路径清晰 |
| **未来兼容** | 无 | Rust 若稳定默认 ABI 可无破坏切换 |

**结论**：`extern "rust"` 保留，作为**语法标记 + 工具链钩子**，而非 ABI 切换。

---

## 4. 语言语法设计

### 4.1 基本形式

```aura
// 调用 Rust 库（Rust 侧必须 #[no_mangle] extern "C"）
extern "rust" "my_rust_lib" {
    fun add(a: Int, b: Int): Int
    fun create(): Handle
    fun process(handle: Handle, data: Pointer<Byte>, len: Long)
    
    // 常量（Rust 侧需 #[no_mangle]）
    val VERSION: Int
}
```

### 4.2 与 C FFI 的对称

| 特性 | `extern "c"` | `extern "rust"` |
|------|--------------|-----------------|
| **调用约定** | C ABI (`ccc`) | **C ABI (`ccc`)**（相同） |
| **符号名** | 源码名 | 源码名（要求 `#[no_mangle]`） |
| **Rust 侧要求** | — | `#[no_mangle] extern "C"` |
| **类型映射** | `Int` → `i32` 等 | 同左 |
| **库加载** | `libloading` 加载 C dylib | `libloading` 加载 Rust dylib |
| **VM 后端** | i64 装箱调度 | 同左 |
| **AOT 后端** | `declare ... (ccc)` | `declare ... (ccc)`（相同） |
| **工具链语义** | 通用 C 库 | Rust 库专用（提示/校验/文档） |

> **关键差异**：两者在**调用约定层完全相同**（都是 C ABI），差异仅在**语法标记**与**工具链语义**。

### 4.3 符号修饰（可选，阶段 2）

```aura
// 显式指定修饰名（应对无法修改 Rust 源码的场景）
extern "rust" "my_rust_lib" {
    fun add: "_RNvC3foo3bar9rust_lib4add"  // 显式 v0 mangling
}
```

> **注**：此功能**阶段 2 实现**，阶段 1 仅支持"符号名 = 源码名"（要求 `#[no_mangle]`）。

### 4.4 不支持的写法

```aura
// ❌ 不支持：Rust String/Vec/Box 不能跨 FFI 边界
extern "rust" "lib" {
    fun foo(s: RustString)  // RustString 非 Aura 类型
}

// ❌ 不支持：Rust 默认布局结构体
extern "rust" "lib" {
    fun bar(p: Pointer<MyStruct>)  // 要求 MyStruct 两侧均为 #[repr(C)]
}

// ❌ 不支持：Rust trait object / dyn
extern "rust" "lib" {
    fun baz(o: Pointer<dyn Trait>)  // dyn 不可 FFI
}

// ❌ 不支持：Rust 侧忘写 extern "C"（编译器应给出警告）
extern "rust" "lib" { fun foo(): Int }
// 编译警告：Rust 侧必须使用 #[no_mangle] extern "C"，否则 ABI 不稳定
```

---

## 5. AST 与 HIR 扩展

### 5.1 AST（无需改动）

`ExternDecl.abi: String` 已是通用字段，`"c"` / `"rust"` 只是值域扩展。

```rust
// ast.rs（无变化）
pub struct ExternDecl {
    pub abi: String,           // "c" | "rust" | "stdcall" | ...
    pub library: Option<String>,
    pub functions: Vec<FnDecl>,
    pub constants: Vec<Stmt>,
    pub span: Span,
}
```

### 5.2 HIR（建议修改）

**建议修改 1**：新增 `FfiAbi` 枚举

```rust
// codegen/hir.rs
/// FFI ABI 类型（新增）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FfiAbi {
    #[default]
    None,   // 非 FFI 函数
    C,      // C ABI（现状）
    Rust,   // Rust 库（语法标记，调用约定同 C）
}
```

> **关键**：`FfiAbi::Rust` **不代表** Rust 默认调用约定，仅作为**语法标记**。代码生成时 `C` 与 `Rust` 走相同路径（都用 `ccc`）。

**建议修改 2**：扩展 `HirFunction`

```rust
// codegen/hir.rs
pub struct HirFunction {
    pub name: String,
    pub params: Vec<HirParam>,
    pub ret: Option<HirType>,
    pub body: HirBlock,
    pub is_native: bool,
    pub type_params: Vec<String>,
    // ── 新增 ──
    /// FFI ABI 标记（仅 `is_native == true` 时有意义）
    pub ffi_abi: FfiAbi,
    /// FFI 库名（对应 `extern "<abi>" "<lib>"`）
    pub ffi_lib: Option<String>,
}
```

**建议修改 3**：HIR 降级填充新字段

```rust
// codegen/hir.rs - desugar_program
Decl::Extern(e) => {
    let abi = match e.abi.as_str() {
        "rust" | "Rust" => FfiAbi::Rust,
        _ => FfiAbi::C,  // 默认 C（包括 "c"、空字符串、未知值）
    };
    for f in &e.functions {
        natives.push(HirFunction {
            // ... 现有字段 ...
            is_native: true,
            ffi_abi: abi,                              // 新增
            ffi_lib: e.library.clone(),                // 新增
        });
    }
    // 常量同理
}
```

### 5.3 编译器警告（建议修改）

`extern "rust"` 块中的函数应产生**编译期提示**（不阻塞编译）：

```
warning: `extern "rust"` 块的函数 `foo` 需在 Rust 侧使用 `#[no_mangle] extern "C"`
  ┌─ main.aura:3:5
  │
3 │     fun foo(): Int
  │     ^^^^^^^^^^^^^^ 提示：Rust 侧必须使用 #[no_mangle] extern "C"
```

### 5.4 序列化（建议修改）

`codegen/serialize.rs` 需扩展序列化新字段：

```rust
// codegen/serialize.rs
fn write_function(buf: &mut Vec<u8>, f: &HirFunction) {
    // ... 现有字段 ...
    buf.push(f.ffi_abi as u8);                  // 新增
    buf.push(f.ffi_lib.is_some() as u8);        // 新增
    if let Some(lib) = &f.ffi_lib {
        write_string(buf, lib);
    }
}
```

---

## 6. AOT 后端设计

### 6.1 核心结论：C 与 Rust 走相同路径

| ABI | LLVM IR 调用约定 | 生成示例 |
|-----|-----------------|----------|
| C | `ccc` | `declare i32 @foo(i32, i32, cc ccc)` |
| Rust | **`ccc`**（相同） | `declare i32 @foo(i32, i32, cc ccc)` |

> **v1.1 修正**：原 v1.0 方案中"Rust 使用平台默认调用约定"**已删除**，因为 Rust 默认 ABI 官方未稳定。两种 ABI 在代码生成层完全相同。

### 6.2 建议修改：`FfiGenerator`

```rust
// codegen/aot/ffi.rs
pub struct FfiGenerator<'a> {
    type_mapper: &'a TypeMapper,
}

impl FfiGenerator {
    fn generate_extern_function(&self, func: &HirFunction) -> String {
        let ret_ty = /* ... */;
        let params_str = /* ... */;
        
        // C 与 Rust 都使用 C ABI（ccc），无差异
        // FfiAbi::Rust 仅作为语义标记，代码生成时等价于 C
        let _ = func.ffi_abi;  // 预留未来扩展
        let cc_suffix = ", cc ccc";  // 统一 C ABI
        
        format!(
            "declare {} @{}({}){}\n",
            ret_ty, func.name, params_str, cc_suffix
        )
    }
}
```

### 6.3 符号修饰（阶段 2）

阶段 1：符号名 = `func.name`（要求 Rust 侧 `#[no_mangle]`）。

阶段 2：支持 `fun foo: "mangled_name"` 语法：

```rust
// parser.rs
fun foo: "_RNvC3foo3bar9rust_lib4add"  // 显式修饰名
```

AST 扩展：

```rust
pub struct FnDecl {
    // ... 现有字段 ...
    pub mangled_name: Option<String>,  // 新增：显式修饰名
}
```

### 6.4 AOT 0 开销验证

**测试用例**：

```rust
// Rust 侧
#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 { a + b }
```

```aura
// Aura 侧
extern "rust" "libfoo" { fun add(a: Int, b: Int): Int }
fun main(): Int { return add(1, 2) }
```

**AOT 生成 LLVM IR**：

```llvm
declare i32 @add(i32, i32, cc ccc)   ; 与 extern "c" 完全一致
define i32 @main() {
    %r = call i32 @add(i32 1, i32 2, cc ccc)   ; 直接调用，无中间层
    ret i32 %r
}
```

**链接**：`lld -o main main.o -lfoo`，0 开销等价于 C FFI，**且官方稳定**。

---

## 7. VM 后端设计

### 7.1 现状回顾

VM 后端统一走 `CFuncPtr`（8 参数 i64 版本），通过 `CType::pack/unpack` 装箱。

### 7.2 Rust FFI 的 VM 行为

**核心原则**：VM 端不区分 C ABI 与 Rust ABI，统一走 i64 装箱。

> 这是 VM 后端的**固有设计**，与 AOT 的 0 开销路径无关。VM 用户应理解：VM 模式有装箱开销，AOT 模式才有真正的 0 开销。

### 7.3 符号解析差异

| 后端 | C FFI 符号解析 | Rust FFI 符号解析 |
|------|---------------|------------------|
| **静态链接** | `dlsym(NULL, "foo")` | `dlsym(NULL, "foo")`（同左） |
| **动态加载** | `libloading` 加载 C dylib | `libloading` 加载 Rust dylib（`.so`/`.dll`） |

**建议修改**：扩展 `DynamicLoader`

```rust
// vm/dynamic_ffi.rs
pub struct DynamicLoader {
    #[cfg(feature = "dynamic-ffi")]
    libs: Vec<LoadedLib>,
    fns: HashMap<String, fn(&[Value]) -> Value>,
    // 新增：库类型元数据
    #[cfg(feature = "dynamic-ffi")]
    lib_abi: HashMap<String, FfiAbi>,
}
```

### 7.4 VM 端的 `CType` 复用

`CType` 枚举与 `pack/unpack` 完全复用，无需新增 `RustType`。Rust FFI 的参数类型本质仍是 `i32`/`i64`/`f32`/`f64` 等，装箱格式与 C 完全一致。

---

## 8. 类型映射

### 8.1 完整映射表

| Aura 类型 | Rust 类型（推荐） | Rust 类型（等价） | 备注 |
|-----------|------------------|------------------|------|
| `Int` | `i32` | `i32` | 32 位有符号 |
| `Long` | `i64` | `i64` | 64 位有符号 |
| `Float` | `f32` | `f32` | 32 位浮点 |
| `Double` | `f64` | `f64` | 64 位浮点 |
| `Boolean` | `bool` | `bool` | 1 字节 |
| `Char` | `u8` | `u8` | UTF-8 字节 |
| `CString` | `*const c_char` | `*const u8` | 字符串指针 |
| `Pointer<T>` | `*mut T` / `*const T` | 同左 | 原始指针 |
| `Handle` | `*mut c_void` | `*mut u8` | 不透明指针 |
| `Unit` | `()` | `()` | 无返回 |

> **前提**：Rust 侧所有 FFI 函数必须 `extern "C"`，否则类型映射不适用。

### 8.2 边界类型限制

| Rust 类型 | 是否支持 FFI | 替代方案 |
|-----------|-------------|---------|
| `String` / `&str` | ❌ | `*const c_char` + 长度 |
| `Vec<T>` | ❌ | `*mut T` + 长度 + 容量 |
| `Box<T>` | ❌ | `*mut T`（手动管理内存） |
| `Option<T>`（nonzero） | ⚠️ 部分 | 用 `Pointer<T>`（null = None） |
| `Result<T, E>` | ❌ | 返回错误码 + out-param |
| `dyn Trait` | ❌ | 用 vtable 指针 + data 指针 |
| `&T` / `&mut T` | ❌ | 用 `*const T` / `*mut T` |
| `struct S`（默认布局） | ❌ | 要求 `#[repr(C)]` |

### 8.3 结构体跨边界

**Aura 侧**：

```aura
extern "rust" "lib" {
    fun create_point(): Handle  // 不透明句柄
    fun get_x(p: Handle): Int
    fun get_y(p: Handle): Int
}
```

**Rust 侧**：

```rust
#[repr(C)]
struct Point { x: i32, y: i32 }

#[no_mangle]
pub extern "C" fn create_point() -> *mut c_void {
    Box::into_raw(Box::new(Point { x: 0, y: 0 }))
}
```

> **设计选择**：阶段 1 **不直接支持**跨边界的 `struct` 字段访问，仅支持 `Handle` 模式。阶段 2 考虑 `#[repr(C)]` 双向映射。

---

## 9. 符号解析策略

### 9.1 阶段 1：`#[no_mangle] extern "C"` 约定

**Rust 侧**：

```rust
#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 { a + b }
```

**Aura 侧**：

```aura
extern "rust" "libfoo" { fun add(a: Int, b: Int): Int }
```

**符号解析**：`libloading::Library::new("libfoo.so").get(b"add")`

> **强制要求**：Rust 侧必须 `#[no_mangle] extern "C"`，否则 Aura 编译器给出警告。

### 9.2 阶段 2：显式修饰名

**Aura 侧**：

```aura
extern "rust" "libfoo" {
    fun add: "_RNvC3foo3bar9rust_lib4add"  // 显式修饰名
}
```

**实现**：AST 扩展 `FnDecl.mangled_name: Option<String>`，AOT 生成时用修饰名。

### 9.3 阶段 3（远期）：元数据探测

读取 Rust rlib 元数据，自动推导 ABI 与修饰名。**当前不实施**，原因：
- Rust rlib 元数据格式未稳定
- 实现复杂度高，收益有限

### 9.4 库发现策略

**Rust dylib 命名约定**：

| 平台 | 静态库 | 动态库 |
|------|-------|-------|
| Linux | `libfoo.a` | `libfoo.so` |
| macOS | `libfoo.a` | `libfoo.dylib` |
| Windows | `foo.lib` | `foo.dll` |

**Aura 解析规则**：

```
extern "rust" "foo" { ... }
    ↓
libfoo.{so,dylib,dll}        // 动态库（VM/动态加载）
libfoo.a / foo.lib           // 静态库（AOT 链接）
```

---

## 10. 回调方向（Rust → Aura）

### 10.1 现状

`extern "c"` 已支持 C → Aura 回调（通过 `aura_callback_trampoline` 蹦床）。

### 10.2 Rust FFI 的回调

**Rust 侧接收 Aura 回调**：

```rust
type Callback = extern "C" fn(*mut c_void, i64, i64, i64, i64, i64, i64, i64, i64) -> i64;

#[no_mangle]
pub extern "C" fn register_callback(cb: *mut c_void) {
    // cb 是 Aura 蹦床指针，直接调用即可
    // 蹦床已按 C ABI 暴露，Rust 侧可用 unsafe 包装
    let cb: Callback = std::mem::transmute(cb);
    cb(/* context */, 1, 2, 3, 4, 5, 6, 7, 8);
}
```

**Aura 侧注册**：

```aura
extern "rust" "callbacklib" {
    fun registerCallback(cb: Pointer<Int>)
}
fun handler(a: Int, b: Int, c: Int, d: Int): Int {
    return a + b + c + d
}
fun main(): Int {
    val cb = makeCallback(handler)
    registerCallback(cb)
    return 0
}
```

### 10.3 设计决策

**阶段 1**：复用现有蹦床机制，Rust 侧用 `unsafe` 包装。**不新增** Rust-specific 蹦床。

**阶段 2**：考虑 Rust-safe 蹦床包装器（基于 `unsafe extern "C"` trait）。

---

## 11. 与现有 FFI 的关系与兼容性

### 11.1 兼容性矩阵

| 现有代码 | 行为 | 变化 |
|---------|------|------|
| `extern "c" "lib" { ... }` | 调用 C ABI | ✅ 无变化 |
| `extern "c" { ... }`（无库名） | 调用静态链接符号 | ✅ 无变化 |
| `extern "c" { fun puts(...) }` | 调用 libc 符号 | ✅ 无变化 |
| `extern "rust" "lib" { ... }` | **新增**：语法标记 Rust 库，调用约定同 C | ✅ 新功能 |

### 11.2 代码复用

| 组件 | 复用策略 |
|------|---------|
| `CType` 枚举 | ✅ 完全复用（参数装箱格式一致） |
| `CFuncPtr` | ✅ 完全复用 |
| `CallbackRegistry` | ✅ 完全复用 |
| `aura_callback_trampoline` | ✅ 完全复用 |
| `DynamicLoader` | ⚠️ 扩展：支持 Rust dylib 加载 |
| `FfiGenerator` | ⚠️ 扩展：`FfiAbi::Rust` 与 `C` 走相同路径，预留未来 |
| `HirFunction` | ⚠️ 扩展：新增 `ffi_abi` / `ffi_lib` |
| `serialize.rs` | ⚠️ 扩展：序列化新字段 |
| `sema/checker.rs` | ⚠️ 扩展：`extern "rust"` 块给出编译警告 |

---

## 12. 实现计划

### 12.1 阶段划分

| 阶段 | 内容 | 工作量 | 依赖 |
|------|------|--------|------|
| **P1 语法** | parser 接受 `extern "rust"`；AST 无变化 | 0.5d | — |
| **P2 HIR** | 新增 `FfiAbi` 枚举 + `ffi_abi`/`ffi_lib` 字段 | 0.5d | P1 |
| **P3 序列化** | 扩展 `serialize.rs` | 0.5d | P2 |
| **P4 警告** | `sema` 校验：`extern "rust"` 块给出 Rust 侧标注提示 | 0.5d | P2 |
| **P5 AOT** | `FfiGenerator` 识别 `FfiAbi::Rust`（路径同 C，预留未来） | 0.5d | P2 |
| **P6 VM** | `DynamicLoader` 支持 Rust dylib | 1d | P2 |
| **P7 测试** | Rust dylib 集成测试 + AOT 等价性验证 | 1.5d | P5+P6 |
| **P8 文档** | 标准库文档 + 示例 | 0.5d | P1-P7 |

**总计**：约 **6 人日**。

### 12.2 里程碑

- **M1（P1-P3）**：语法与 HIR 就绪，可通过 `aura compile` 生成 IR
- **M2（P4-P5）**：AOT 0 开销路径打通 + 编译警告
- **M3（P6）**：VM 动态加载 Rust dylib
- **M4（P7）**：测试覆盖与回归验证
- **M5（P8）**：文档与示例完备

### 12.3 实施优先级

```
高优先级（阶段 1 实施）：
├── P1 语法（parser）
├── P2 HIR（FfiAbi + ffi_lib）
├── P3 序列化
├── P4 警告（sema 校验）
├── P5 AOT（FfiGenerator 识别，路径同 C）
└── P7 测试（基础用例）

中优先级（阶段 2）：
├── P6 VM（DynamicLoader 扩展）
├── 阶段 2 符号修饰语法
└── 阶段 2 结构体跨边界

低优先级（阶段 3）：
├── 元数据探测
├── Rust-safe 蹦床
└── trait object 映射
```

---

## 13. 风险与限制

### 13.1 Rust ABI 稳定性

| 风险 | 影响 | 缓解 |
|------|------|------|
| Rust 默认 ABI 未稳定 | **已规避**：`extern "rust"` 走 C ABI，不依赖 Rust 默认 ABI | ✅ 设计层面消除 |
| Rust 侧忘写 `extern "C"` | 调用失败（UB） | ✅ P4 编译警告 + 文档强调 |
| Rust v0 mangling 可能变化 | 阶段 2 修饰名方案可能失效 | 阶段 2 推迟，先做 `#[no_mangle]` 路径 |
| Rust struct 默认布局变化 | `#[repr(C)]` 之外的结构体不可用 | 文档明确限制 |

### 13.2 工程风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| LLVM IR `ccc` 跨平台一致性 | 调用约定可能不一致 | 阶段 1 仅 Linux/x86-64 验证 |
| Rust dylib 跨平台链接 | libstd 静态链接差异 | 文档说明平台支持矩阵 |
| 回调内存管理 | Aura 回调生命周期与 Rust 所有权 | 阶段 1 不处理，文档警告 |

### 13.3 限制

1. **VM 后端不"0 开销"**：与 C FFI 一致，VM 模式有装箱开销
2. **不支持 Rust 集合类型跨边界**：`String`/`Vec`/`Box` 必须用 C 等价类型
3. **不支持 Rust 默认布局结构体**：需 `#[repr(C)]`
4. **不支持 trait object**：需用 vtable 指针模式
5. **阶段 1 不支持修饰名**：Rust 侧必须 `#[no_mangle]`
6. **Rust 侧强制 `#[no_mangle] extern "C"`**：编译器给出警告，但不阻塞编译

### 13.4 与 v1.0 方案的对比

| 维度 | v1.0（已废弃） | v1.1（当前） |
|------|---------------|-------------|
| `extern "rust"` 调用约定 | Rust 默认（平台本地） | **C ABI（`ccc`）** |
| Rust 侧要求 | `#[no_mangle]`（可选 `extern "C"`） | **强制 `#[no_mangle] extern "C"`** |
| 跨 Rust 版本稳定性 | ⚠️ 官方未稳定，可能失效 | ✅ 官方稳定，永不失效 |
| AOT 生成 IR | `declare @foo(...)`（无 `ccc`） | `declare @foo(..., cc ccc)`（与 C 一致） |
| 风险 | 高（依赖未稳定 ABI） | 低（走官方稳定路径） |
| "原生亲和"含义 | ABI 层（错误） | 语法/工具/库发现/文档（正确） |

---

## 14. 完整示例

### 14.1 基础调用

**Rust 侧（`src/lib.rs`）**：

```rust
use std::ffi::c_void;

#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 { a + b }

#[no_mangle]
pub extern "C" fn create_point(x: i32, y: i32) -> *mut c_void {
    Box::into_raw(Box::new(Point { x, y }))
}

#[no_mangle]
pub extern "C" fn destroy_point(p: *mut c_void) {
    unsafe { drop(Box::from_raw(p as *mut Point)); }
}
```

**Aura 侧（`main.aura`）**：

```aura
extern "rust" "mypointlib" {
    fun add(a: Int, b: Int): Int
    fun createPoint(x: Int, y: Int): Handle
    fun destroyPoint(p: Handle)
}

fun main(): Int {
    val p = createPoint(3, 4)
    destroyPoint(p)
    return add(1, 2)  // → 3
}
```

**编译**：

```bash
aura build main.aura -L ./target/release --extern libmypointlib.so
```

### 14.2 回调

**Rust 侧**：

```rust
type Callback = extern "C" fn(*mut c_void, i64, i64, i64, i64, i64, i64, i64, i64) -> i64;

#[no_mangle]
pub extern "C" fn register_and_call(cb: *mut c_void) -> i64 {
    let cb: Callback = unsafe { std::mem::transmute(cb) };
    cb(std::ptr::null_mut(), 1, 2, 3, 4, 5, 6, 7, 8)
}
```

**Aura 侧**：

```aura
extern "rust" "callbacklib" {
    fun registerAndCall(cb: Pointer<Int>): Int
}

fun handler(a: Int, b: Int, c: Int, d: Int): Int {
    return a + b + c + d
}

fun main(): Int {
    val cb = makeCallback(handler)
    return registerAndCall(cb)  // → 10
}
```

### 14.3 混合 C + Rust

```aura
extern "c" "raylib" {
    fun InitWindow(w: Int, h: Int, title: CString)
}

extern "rust" "game_engine" {
    fun createGame(): Handle
    fun gameUpdate(g: Handle, dt: Float)
    fun gameDestroy(g: Handle)
}

fun main() {
    InitWindow(800, 600, CString("Game"))
    val game = createGame()
    while (true) {
        gameUpdate(game, 0.016)
    }
    gameDestroy(game)
}
```

### 14.4 错误示例（Rust 侧漏写 `extern "C"`）

**Rust 侧（错误）**：

```rust
// ❌ 错误：未使用 extern "C"
#[no_mangle]
pub fn add(a: i32, b: i32) -> i32 { a + b }
```

**Aura 编译时警告**：

```
warning: `extern "rust"` 块的函数 `add` 需在 Rust 侧使用 `#[no_mangle] extern "C"`
  ┌─ main.aura:3:5
  │
3 │     fun add(a: Int, b: Int): Int
  │     ^^^^^^^^^^^^^^^^^^^^^^^^^^ 提示：Rust 侧必须使用 #[no_mangle] extern "C"
```

**运行时行为**：未定义（UB），可能崩溃或返回错误结果。

---

## 15. 附录

### A. 关键设计决策汇总

| 决策 | 选择 | 理由 |
|------|------|------|
| 是否支持 `extern "rust"` 关键字 | ✅ 支持 | 语法清晰、工具链钩子、库发现 |
| `extern "rust"` 使用何种调用约定 | ✅ **C ABI** | 官方稳定，跨 Rust 版本不失效 |
| 是否使用 Rust 默认调用约定 | ❌ 不使用 | 官方未稳定，风险不可接受 |
| Rust 侧是否强制 `extern "C"` | ✅ 强制 | 唯一官方稳定的路径 |
| 是否支持修饰名 | ⚠️ 阶段 2 | 阶段 1 要求 `#[no_mangle]` |
| 是否支持 Rust 集合类型 | ❌ 不支持 | 用 C 等价类型 |
| 是否支持结构体跨边界 | ⚠️ 阶段 2 | 阶段 1 仅 `Handle` |
| VM 后端是否区分 ABI | ❌ 不区分 | 复用 i64 装箱 |
| AOT 是否区分 ABI | ⚠️ 区分标记但不区分行为 | `FfiAbi::Rust` 与 `C` 走相同路径，预留未来 |
| 是否实现 mangling 解码 | ❌ 不实现 | 收益低、风险高 |
| 是否实现元数据探测 | ❌ 阶段 3 | 复杂度高 |

### B. 与现有文档的关系

| 文档 | 关系 |
|------|------|
| `docs/技术方案.md §3.9` | 当前仅展示 Rust via C ABI，本方案补充原生亲和语法 |
| `docs/技术方案.md §9.3` | FFI 处理章节，需补充 Rust ABI 子节 |
| `docs/开发规划与实现进度.md` | P8 已完成 C FFI，本方案是 P8 扩展 |

### C. 验证清单

- [ ] parser 接受 `extern "rust"` 字符串字面量
- [ ] AST → HIR 正确传递 `ffi_abi` / `ffi_lib`
- [ ] 序列化/反序列化往返一致
- [ ] `sema` 对 `extern "rust"` 块给出 Rust 侧标注警告
- [ ] AOT 生成正确 LLVM IR（C 与 Rust 均使用 `ccc`）
- [ ] AOT 编译后链接 Rust dylib 成功
- [ ] AOT 执行结果正确
- [ ] VM 加载 Rust dylib 成功
- [ ] VM 调用结果正确
- [ ] 现有 `extern "c"` 测试全部通过
- [ ] 性能基准：AOT Rust FFI 与 C FFI 调用开销差异 < 5%
- [ ] 错误用例：Rust 侧漏写 `extern "C"` 时给出警告

---

## 16. 总结

本方案为 Aura 增加对 Rust 库的**原生亲和** FFI 支持：

- **语法层**：`extern "rust"` 与 `extern "c"` 对称，AST 无改动
- **ABI 层**：**走 C ABI**（官方稳定），**不使用** Rust 默认 ABI（官方未稳定）
- **HIR 层**：新增 `FfiAbi` 枚举 + `ffi_abi`/`ffi_lib` 字段，标记 ABI 类型
- **AOT 层**：`FfiAbi::Rust` 与 `C` 走相同路径（`ccc`），预留未来扩展
- **VM 层**：复用既有 i64 装箱调度，不引入新模型
- **类型映射**：复用 `CType`，明确限制不可跨边界的 Rust 类型
- **符号解析**：阶段 1 走 `#[no_mangle] extern "C"` 约定
- **错误预防**：编译器对 `extern "rust"` 块给出 Rust 侧标注警告

**核心承诺**：
1. AOT 模式下，Rust FFI 与 C FFI **同等 0 开销**——LLVM IR 直接调用，无中间层，无参数装箱
2. **跨 Rust 版本稳定**——全程走 C ABI（官方稳定），不依赖 Rust 默认 ABI（官方未稳定）
3. **"原生亲和"含义**——语法/工具/库发现/文档层面，**非** ABI 层

**预估工作量**：约 6 人日（阶段 1）。

**与 v1.0 的关键差异**：
- v1.0 错误地让 `extern "rust"` 使用 Rust 默认调用约定（官方未稳定）
- v1.1 修正为使用 C ABI（官方稳定），`extern "rust"` 仅作为语法标记

**下一步**：评审通过后，按 P1→P8 顺序实施。
