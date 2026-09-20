# std / 协程库「按需免import + 按需链接」改造方案

> 对应问题：std 与协程库无需 `import` 即可调用，但当前实现存在**编译期/运行期符号面分裂、用户抢名、MIR 降级歧义**三个 bug，以及**体积无法裁剪**的结构性代价。
>
> 设计原则：**按需免import**（仅 prelude 免import）+ **按需链接**（仅 imported 模块链接进二进制）

---

## 现状诊断

| 层 | 文件 | 注册了什么 | 问题 |
|----|------|-----------|------|
| 语义检查 | `sema/checker.rs:51-150` | 13 个名字（`println`/`print`/`listOf` + 11 个 `aura.concurrent.*`） | 其余 325 个 std 名字编译期不可见 |
| HIR 降糖 | `codegen/hir.rs:359-650` | `println` + `malloc` + `CString` + 并发 + `std_native_functions()` | 全量注册，无法按需 |
| MIR 降级 | `codegen/mir.rs:453` | 纯字符串匹配 `ctx.natives.contains(callee)` | 用户同名函数被静默抢成 native 调用 |
| 符号表 | `sema/symbol.rs:132-175` | 函数重签名为合法重载 | 用户可污染内置名且重载解析会选错 |
| 运行时 | `vm/native.rs:29-69` | 全部 338 个 native 一次性注册 | 体积无法裁剪（~3MB） |

---

## 核心设计

```
┌─────────────────────────────────────────────────────────┐
│                    源码层                                 │
│  println("hello")          ← 免import（prelude，17个）    │
│  import aura.lang.std.Math.*        ← 显式引入                     │
│  return sin(1.0)           ← import 后可用                │
└────────────────────┬────────────────────────────────────┘
                     │ 编译期分析
┌────────────────────▼────────────────────────────────────┐
│                 Import 解析器                             │
│  收集：prelude(17) + imported modules                     │
│  输出：{enabled_modules: ["math"], used_fns: ["sin"]}     │
└────────────────────┬────────────────────────────────────┘
                     │ 驱动
┌────────────────────▼────────────────────────────────────┐
│              HIR / Codegen 层                             │
│  只注册 enabled_modules 中的 native 函数                   │
│  未 import 的模块：不注册、不链接                           │
└────────────────────┬────────────────────────────────────┘
                     │ 链接
┌────────────────────▼────────────────────────────────────┐
│              NativeRegistry（运行时）                      │
│  预置：17 个 prelude（始终存在，~200KB）                    │
│  按需：imported modules 的函数（编译期确定）                  │
│  未 import 的 std 代码：不编译进二进制                       │
└─────────────────────────────────────────────────────────┘
```

### Prelude（免import，17 个）

| 函数 | 用途 |
|------|------|
| `println` | 输出（换行） |
| `print` | 输出（不换行） |
| `puts` | C 风格输出 |
| `abs` | 绝对值 |
| `sqrt` | 平方根 |
| `pow` | 幂运算 |
| `toInt` | 转整数 |
| `toFloat` | 转浮点 |
| `toStr` | 转字符串 |
| `clock` | 时钟 |
| `strlen` | 字符串长度 |
| `CString` | C 字符串 |
| `CStr` | C 字符串 |
| `ptrIsNull` | 指针判空 |
| `ptrToInt` | 指针转整数 |
| `intToPtr` | 整数转指针 |
| `makeCallback` | 创建回调 |

### Import 语法（需import，320+ 个）

```aura
// 通配：引入模块所有函数到当前作用域
import aura.lang.std.Math.*

// 模块引用：通过模块名调用
import aura.lang.std.Math
// 调用方式：aura.lang.std.Math.sin(1.0)

// 精确引入：只引入指定函数
import aura.lang.std.Math.sin

// 别名：避免命名冲突
import aura.lang.std.Math as m
// 调用方式：m.sin(1.0)

// 混合：prelude + 按需
import aura.lang.std.Math.*
fun main(): Float {
    println("test")  // prelude，免import
    return sin(1.0)  // 需import
}
```

---

## 体积对比

| 场景 | 当前 | 按需后 |
|------|------|--------|
| `println("hello")` | ~3 MB（全量 std） | ~200 KB（仅 prelude） |
| `import aura.lang.std.Math.*` | ~3 MB | ~500 KB |
| `import aura.lang.std.IO.*` + `import aura.lang.std.Math.*` | ~3 MB | ~800 KB |
| 全量 import 所有模块 | ~3 MB | ~3 MB（无变化） |

---

## Phase 1 — 按需免import 基础架构

### 1a. 新增 `compiler/src/std/prelu.rs`（已合并到 `decl.rs`）

**单一真相源**：区分 prelude（17 个）和命名空间库（320 个）。

```rust
// compiler/src/std/decl.rs
pub const PRELUDE_NAMES: &[&str] = &[
    "println", "print", "puts", "abs", "sqrt", "pow",
    "toInt", "toFloat", "toStr", "clock", "strlen",
    "CString", "CStr", "ptrIsNull", "ptrToInt", "intToPtr",
    "makeCallback",
];

pub fn is_prelude(name: &str) -> bool   // 17 个免import
pub fn is_namespaced(name: &str) -> bool  // 320 个需import
pub fn all_names() -> ...  // 全部 338 个
```

### 1b. 改 `checker.rs:check_call` 只兜底 prelu

```rust
// 修复前（全量兜底，错误）：
if is_builtin(&full_name) { return Ty::Any; }

// 修复后（只兜底 prelude）：
if is_prelude(&full_name) { return Ty::Any; }
// 命名空间函数必须通过 import 引入
```

### 1c. 改 `checker.rs:collect_declaration` 只阻止 prelu 抢名

```rust
// 修复前（阻止所有 std 名，错误）：
if is_builtin(&f.name) { report_error() }

// 修复后（只阻止 prelude 名）：
if is_prelude(&f.name) { report_error() }
// 用户可以自由定义 fun sin(x: Float)，只要不 import aura.lang.std.Math.*
```

### 1d. 改 `mir.rs:lower_expr` 降级前查用户作用域（保留）

```rust
// 用户函数优先于同名 native
if ctx.user_functions.contains(callee) {
    emit Call  // 用户函数
} else if ctx.natives.contains(callee) {
    emit CallNative  // native 函数
}
```

### 1e. 改 `hir.rs` 只自动注册 prelu

```rust
// 只自动注册 prelude 函数（17 个）
// 命名空间函数在 import 解析后才注册
// std_native_functions() 保留，但只在 import 时调用
```

### 验收

```bash
cargo test --workspace  # 全绿
```

新增 3 条回归测试（待 Phase 1b 完成后添加）：
1. `println("hello")` 免import 编译通过且运行正确
2. `fun println(x: Any)` 编译报错（不能重定义 prelude）
3. `fun sin(x: Float)` 不import 时编译通过（用户可自由定义）

---

## Phase 2 — Import 语法与解析

### 2a. 新增 `ImportDecl` AST 节点

```rust
// compiler/src/ast.rs
pub enum Decl {
    // ... 现有类型
    Import(ImportDecl),
}

pub struct ImportDecl {
    pub module_path: Vec<String>,  // ["aura", "math"]
    pub kind: ImportKind,           // Wildcard / Module / Function / Alias
    pub span: Span,
}

pub enum ImportKind {
    Wildcard,           // import aura.lang.std.Math.*
    Module,             // import aura.lang.std.Math
    Function(String),   // import aura.lang.std.Math.sin
    Alias(String),      // import aura.lang.std.Math as m
}
```

### 2b. 改 `parser.rs` 解析 `import` 声明

```rust
fn parse_import(&mut self) -> ImportDecl {
    self.expect("import");
    let path = self.parse_dotted_path();
    let kind = if self.check("*") {
        self.advance();
        ImportKind::Wildcard
    } else if self.check("as") {
        self.advance();
        let alias = self.parse_ident();
        ImportKind::Alias(alias)
    } else if path.len() > 2 {
        // import aura.lang.std.Math.sin → 函数引入
        ImportKind::Function(path.pop())
    } else {
        ImportKind::Module
    };
    ImportDecl { module_path: path, kind, span }
}
```

### 2c. 改 `checker.rs` 展开 import 到符号表

```rust
// 处理 ImportDecl
for imp in &program.imports {
    match &imp.kind {
        ImportKind::Wildcard => {
            // 把模块所有函数加到符号表
            for name in module_functions(&imp.module_path) {
                self.symbols.insert_function(name, ...);
            }
        }
        ImportKind::Module => {
            // 注册模块名到符号表（调用时用 aura.lang.std.Math.sin）
            self.symbols.insert_module(&imp.module_path);
        }
        ImportKind::Function(fn_name) => {
            // 只注册指定函数
            self.symbols.insert_function(fn_name, ...);
        }
        ImportKind::Alias(alias) => {
            // 注册别名
            self.symbols.insert_alias(alias, &imp.module_path);
        }
    }
}
```

### 验收

- `import aura.lang.std.Math.*` + `sin(1.0)` → 通过
- 不import + `sin(1.0)` → 报错 `unresolved identifier`
- `import aura.lang.std.Math` + `aura.lang.std.Math.sin(1.0)` → 通过
- `import aura.lang.std.Math.sin` + `sin(1.0)` → 通过
- `import aura.lang.std.Math as m` + `m.sin(1.0)` → 通过

---

## Phase 3 — 体积优化

### 3a. `NativeRegistry::with_modules`

```rust
impl NativeRegistry {
    /// 仅注册 prelude + 指定模块
    pub fn with_modules(modules: &[&str]) -> Self {
        let mut reg = Self::new_prelude_only();  // 17 个 prelude
        for module in modules {
            match module {
                "math" => std_math::register(&mut reg),
                "io" => std_io::register(&mut reg),
                "string" => std_string::register(&mut reg),
                // ...
                _ => {}
            }
        }
        reg
    }
    
    /// 仅注册 prelu（最小化）
    fn new_prelude_only() -> Self {
        let mut reg = Self::new();
        // 只注册 17 个 prelude 函数
        reg.register("println", native_println);
        reg.register("print", native_print);
        // ...
        reg
    }
}
```

### 3b. HIR 按 import 注册

```rust
pub fn lower_program(hir: &HirProgram, imports: &ImportSet) -> (Vec<MirFunction>, LowerCtx) {
    let mut ctx = LowerCtx::new();
    
    // 1. 注册 prelude（始终存在）
    for name in PRELUDE_NAMES {
        ctx.register_native(name, 1);
    }
    
    // 2. 注册 imported modules
    for module in &imports.enabled_modules {
        for (name, param_count) in module_functions(module) {
            ctx.register_native(name, param_count);
        }
    }
    
    // 3. 降级用户函数
    // ...
}
```

### 3c. Cargo feature flags

```toml
# compiler/Cargo.toml
[features]
default = []  # 默认不启用任何 std 模块（仅 prelude）
std-io = []
std-math = []
std-string = []
std-net = []
std-json = []
std-collections = []
std-fs = []
std-time = []
std-random = []
std-encoding = []
std-ascii = []
std-console = []
std-env = []
std-process = []
std-path = []
std-iter = []
std-assert = []
std-test = []
std-builtin = []
std-concurrent = []
```

```rust
// compiler/src/std/mod.rs
pub fn register_all(reg: &mut NativeRegistry) {
    #[cfg(feature = "std-io")]
    std_io::register(reg);
    
    #[cfg(feature = "std-math")]
    std_math::register(reg);
    
    // 未启用的模块：代码不编译进二进制
}
```

### 3d. AOT std 支持（C ABI 导出）

```rust
// compiler/src/std/aot_cffi.rs
#[no_mangle]
pub extern "C" fn aura_printf(fmt: *const c_char, ...) -> c_int { ... }
#[no_mangle]
pub extern "C" fn aura_sin(x: f64) -> f64 { x.sin() }
#[no_mangle]
pub extern "C" fn aura_sqrt(x: f64) -> f64 { x.sqrt() }
```

### 验收

- `cargo build --no-default-features --features "std-io,std-math"` → 二进制显著减小
- minimal features 编译并运行 `spawn(42) + println(...)` → 通过
- AOT 编译 `fun main() { println("hello") }` → 生成的 ELF 独立运行，不依赖 Aura VM

---

## 落地顺序

| Phase | 工期 | 改动行数 | 破坏兼容 | 优先级 |
|-------|------|---------|---------|--------|
| **1a** | 1 天 | ~100  | 是（命名空间函数需 import） | **必做** |
| **1b** | 3 天 | ~500  | 否 | 建议 |
| **1c** | 2 天 | ~200  | 否 | 建议 |
| **2** | 1 天 | ~100  | 否 | 建议 |
| **3** | 2 周 | ~500  | 否 | 视部署形态 |
| **4** | 1 周 | ~400  | 否 | 视 AOT 优先级 |

---

## 风险

1. **Phase 1a 会破坏现有代码**：命名空间函数（`aura.lang.std.Math.sin`）需要 `import`，未 import 的调用会报错。这是设计意图，但需要更新文档和示例。
2. **Phase 2 的 import 解析**：需要处理嵌套模块（`aura.lang.std.Math.sin`）、别名（`as m`）、通配（`*`）等多种语法，实现复杂度中等。
3. **Phase 3 的 feature flags**：编译期裁剪需要 `#[cfg(feature = "...")]` 门控每个 std 模块，修改面较大。
4. **Phase 4 的 C ABI 边界**：Aura `String` 是 `{ptr, len}` 结构，C ABI 侧约定 `const char*`（终止符），需要在转换层处理。

---

## 参考

- 现状分析：见对话中「std 与协程库免 import 改造方案」
- 涉及文件：`compiler/src/std/`、`compiler/src/sema/`、`compiler/src/codegen/`、`compiler/src/vm/`
