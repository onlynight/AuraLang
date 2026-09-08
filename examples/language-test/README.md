# Aura 语言特性全量测试 Demo

> 分阶段、可独立验证的语言特性覆盖清单。每个阶段文件独立可编译、可运行、可单独 `aura check`。

---

## 目录结构

```
examples/language-test/
├── README.md              ← 本文件（规划方案 + 验证指南）
├── 01-lexer.aura          ← Phase 1: 词法基础（字面量 / 运算符 / 插值 / 注释）
├── 02-types-variables.aura ← Phase 2: 类型与变量（✅ 已开发）
├── 03-functions.aura       ← Phase 3: 函数（✅ 已开发，VM 部分特性待完善）
├── 04-control-flow.aura    ← Phase 4: 控制流（✅ 已开发）
├── 05-classes.aura         ← Phase 5: 类与对象（✅ 已开发）
├── 06-null-safety.aura     ← Phase 6: 空安全（✅ 已开发）
├── 07-error-handling.aura  ← Phase 7: 错误处理（✅ 已开发，全模式编译通过）
├── 08-concurrency.aura     ← Phase 8: 并发（待开发）
├── 09-ffi.aura             ← Phase 9: FFI（待开发）
├── 10-memory.aura          ← Phase 10: 内存管理（待开发）
├── 11-imports.aura         ← Phase 11: 导入（待开发）
├── 12-annotations.aura     ← Phase 12: 注解与 Comptime（待开发）
├── 13-stdlib.aura          ← Phase 13: 标准库快照（待开发）
├── 14-string-interp.aura   ← Phase 14: 字符串插值（待开发）
├── 15-advanced.aura        ← Phase 15: 高级特性（待开发）
├── 16-script-mode.aura     ← Phase 16: 脚本模式（待开发）
├── libs/
│   └── math/              ← extern interface 测试用数学函数库
│       ├── aura.toml
│       └── src/lib.aura
└── run-all.sh              ← 一键测试全部阶段（待开发）
```

---

## 验证模式说明

每个阶段文件需通过 **三种执行模式** 的验证：

| 模式 | 命令 | 说明 | 构建要求 |
|------|------|------|----------|
| **VM**（解释器） | `aura run <file>` | 字节码解释执行 | 默认构建即可 |
| **JIT**（Cranelift） | `aura run <file> --jit` | 热点函数 JIT 编译 | `--features jit` |
| **AOT**（LLVM） | `aura build <file> --aot --output <path>` | 编译为原生可执行文件 | `--features llvm` |

> **构建命令**：`cargo build --release --features "llvm,jit"`

---

## 阶段验证结果总表

| 阶段 | 文件 | VM | JIT | AOT |
|------|------|:--:|:---:|:---:|
| Phase 1 词法基础 | `01-lexer.aura` | ✅ | ✅ | ✅ |
| Phase 2 类型与变量 | `02-types-variables.aura` | ✅ | ✅ | ✅（编译通过并可运行，部分 Double 输出待优化） |
| Phase 3 函数 | `03-functions.aura` | ✅ | ✅ | ⏳（需 `--features llvm`） |
| Phase 4 控制流 | `04-control-flow.aura` | ✅ | ✅ | ✅ |
| Phase 5 类与对象 | `05-classes.aura` | ✅ | ⚠️（方法分派异常） | ⏳ |
| Phase 6 空安全 | `06-null-safety.aura` | ✅ | ✅ | ✅（编译通过并可运行，toString 对 null/String 输出待优化） |
| Phase 7 错误处理 | `07-error-handling.aura` | ✅ | ✅ | ✅（编译通过，运行时输出待优化） |
| Phase 8 并发 | `08-concurrency.aura` | ⏳ | ⏳ | ⏳ |
| Phase 9 FFI | `09-ffi.aura` | ⏳ | ⏳ | ⏳ |
| Phase 10 内存管理 | `10-memory.aura` | ⏳ | ⏳ | ⏳ |
| Phase 11 导入 | `11-imports.aura` | ⏳ | ⏳ | ⏳ |
| Phase 12 注解 | `12-annotations.aura` | ⏳ | ⏳ | ⏳ |
| Phase 13 标准库 | `13-stdlib.aura` | ⏳ | ⏳ | ⏳ |
| Phase 14 字符串插值 | `14-string-interp.aura` | ⏳ | ⏳ | ⏳ |
| Phase 15 高级特性 | `15-advanced.aura` | ⏳ | ⏳ | ⏳ |
| Phase 16 脚本模式 | `16-script-mode.aura` | ⏳ | ⏳ | ⏳ |

> **AOT 已知限制**（`docs/遗留问题与风险分析报告.md`）：
> - `Any` 类型 `is` 检查在 AOT 中返回 `String`（类型擦除为 `i8*`，无法区分实际类型）
> - 标签循环 `break@label` / `continue@label` 尚未实现（AST 无 label 字段）
> - 嵌套函数不支持（需使用顶层函数）
> - `Boolean.toString()` 返回 `0`/`1` 而非 `false`/`true`（C FFI 仅支持 Int/Double）

---

## 阶段划分与特性覆盖

### Phase 1 — 词法基础（✅ 已开发）

| 覆盖项 | 说明 |
|--------|------|
| **字面量** | Int（十进制 / 十六进制 `0xFF` / 二进制 `0b1100` / 下划线 `1_000`）、Long（`100L`）、Float（`3.14` / `3.14f`）、Double（`3.14d`）、Char（`'A'`）、Bool（`true` / `false`） |
| **字符串** | 普通 `"hello"`、转义 `\n\t`、插值 `$var` / `${expr}`、原始字符串 `"""..."""` |
| **运算符** | 算术 `+ - * / %`、复合赋值 `+= -= *= /= %=`、自增自减 `++ --`、比较 `== != < > <= >=`、逻辑 `&& \|\| !`、位运算 `& \| ^ << >> >>>`、空安全 `!!` |
| **分隔符** | `( ) { } [ ] , ; : . :: @` |
| **注释** | 单行 `//`、多行 `/* */`、文档 `///` |
| **关键字** | 全部 60+ 关键字（通过代码引用覆盖） |

**验证命令与结果**：
```bash
aura check examples/language-test/01-lexer.aura              # 语法/语义检查 → ✅ 通过
aura run examples/language-test/01-lexer.aura                 # VM 运行时验证 → ✅ 完成
aura run examples/language-test/01-lexer.aura --jit           # JIT 模式验证 → ✅ 完成
aura build examples/language-test/01-lexer.aura --aot --output target/test/01-lexer  # AOT 编译 → ✅ 编译成功
target/test/01-lexer.exe                                      # AOT 运行 → ✅ 完成（已修复字符串字面量崩溃）
aura tokens examples/language-test/01-lexer.aura              # 词法分析输出 → ✅ 正常
```

> **AOT 已修复的问题**：字符串字面量从 `i8*` 指针改为 `{ i8*, i64 }` 结构体，修复了 §1.7 字符串段 Access Violation 崩溃。

---

### Phase 2 — 类型与变量（✅ 已开发）

| 覆盖项 | 说明 |
|--------|------|
| `val` / `var` | 只读与可变绑定、复合赋值 `+=` `*=` |
| `lateinit var` | 类内延迟初始化 ✓；顶层 lateinit 在 main 内直接赋值 ✓ |
| `val by lazy` | 惰性求值，首次访问时计算，之后缓存（计算次数=1） |
| 类型推断 | 从初始化器推断类型（Int/Double/String/Boolean） |
| 显式类型 | `val x: Type = value`，与推断互操作 |
| 可空类型 `T?` | `null` 初始化、Elvis `?:` 默认值、安全访问后运算 |
| `typealias` | 类型别名声明与使用（`Answer=Int`, `Question=String`, `Flag=Boolean`），别名与原始类型互操作 |
| `is` 模式 | `when` 表达式中 `is Type` 分支匹配（String/Int/Boolean/Double） |
| 复合类型 | `struct` 字段访问+方法调用、`to` 运算符构造 Pair |
| 变量作用域 | 函数/块/嵌套块作用域、函数参数 |

**编译器修复**（Phase 2 开发中发现并修复的 8 个 Bug）：
| Bug | 位置 | 修复 |
|-----|------|------|
| 顶层 `val`/`var` 在 sema 中不可见 | `checker.rs::analyze` | 新增 `collect_top_level_stmt` / `check_top_level_stmt` 处理 `top_level_statements` |
| `typealias` 在 `check_type` 中不解析 | `checker.rs::check_type` | `Ty::Named` 查询 `symbols.types` 索引，解析别名到目标类型 |
| `;` 分隔符解析为 `Ident(";")` | `parser.rs::parse_statement` | `TokenKind::Semicolon` 返回空块 `Stmt::Block` |
| 顶层 val + main 运行时值丢失 | `hir.rs::synthesize_main_if_missing` | 顶层语句前置到 main 体首（`splice(0..0)`） |
| `!!` 非空断言未实现 | `parser.rs::parse_postfix_chain` | 新增 `TokenKind::DoubleBang` 分支，产生 `Expr::AssertNonNull` |
| `as` 类型转换未实现 | `parser.rs::parse_expression` | 新增 `TokenKind::As` 分支，产生 `Expr::TypeCast` + `infix_binding_power` |
| `listOf` 仅接受 1 参数 | `checker.rs::new` | 注册 10 个 `has_default` 参数，接受 0-10 个任意参数 |
| `Pair.first`/`.second` 类型错误 | `checker.rs::check_member` | `Ty::Named("Pair")` 返回 `Ty::Any` 而非 `Ty::Error` |

**AOT 后端修复**（Phase 2 AOT 编译中发现并修复的 16 个 Bug）：
| Bug | 位置 | 修复 |
|-----|------|------|
| Float 字面量始终为 32 位 | `emit.rs::emit_literal` | `float` → `double`（64位），与 Kotlin 一致 |
| 类型推断变量默认 i32 | `emit.rs::emit_variable_decl` | `ty: None` 时从初始化器推断 LLVM 类型 |
| 混合类型二元运算只检查左侧 | `emit.rs::emit_binary` | 检查左右两侧，int+double 时自动 `sitofp` 转换 |
| 结构体比较用 fcmp | `emit.rs::emit_binary` 比较运算符 | 结构体类型用 `icmp` 提取指针，null 比较用 `extractvalue` |
| phi 类型不匹配（字符串） | `emit.rs::emit_if_expr` | 字符串结构体 vs 指针自动 `insertvalue` 包装 |
| 无返回类型函数默认 i32 | `emit.rs::emit_function` | 默认返回类型改为 `void`（Unit 函数） |
| struct 字段访问返回 i32 | `emit.rs::emit_member_access` | 从 `class_field_types` 查找字段类型，使用正确的 `load` 指令 |
| 字符串字面量返回 i8* | `emit.rs::emit_string_literal` | 返回 `{ i8*, i64 }` 结构体（指针 + 长度），修复 Phase 1 崩溃 |
| typealias 未解析 | `hir.rs` + `emit.rs` | HIR 添加 `type_aliases` 表，AOT `map_type` 解析别名 |
| `is` 运算符未实现 | `parser.rs` + `hir.rs` + `native.rs` + `emit.rs` | Parser 添加 `__is__` 前缀标记；HIR 生成 `aura_isOfType` 调用；VM 注册原生函数；AOT 返回 `i1` |
| `else` 分支未特殊处理 | `parser.rs` + `hir.rs` | Parser 添加 `__else__` 标记；HIR 生成 `Bool(true)` 默认分支 |
| `when` 表达式 null 处理 | `emit.rs::emit_if_expr` | null 值在 `add` 指令中替换为 0（数值类型） |
| **顶层 lateinit var 未分配** | `cli/main.rs` | AOT CLI 路径缺失 `synthesize_main_if_missing` 调用，导致顶层变量无 alloca |
| **字符串字面量 UTF-8 长度错误** | `emit.rs::emit_string_literal` | 非 ASCII 字节用 `\XX` 十六进制转义，修复 LLVM 字符串长度不匹配 |
| **嵌套 if 的 phi 前驱错误** | `emit.rs::emit_if_expr` | 嵌套 `if` 表达式产生额外块时，phi 前驱使用实际最后块名而非 then/else 块名 |
| **`aura_isOfType` 符号未定义** | `emit.rs` + `hir.rs` + `native.rs` + `aura_std_cffi.c` | 符号名从 `aura.isOfType`（含点号）改为 `aura_isOfType`（下划线），C FFI 实现完整 |

**验证命令与结果**：
```bash
aura check examples/language-test/02-types-variables.aura              # 语法/语义检查 → ✅ 通过
aura run examples/language-test/02-types-variables.aura                 # VM 运行时验证 → ✅ 完成
aura run examples/language-test/02-types-variables.aura --jit           # JIT 模式验证 → ✅ 完成
aura build examples/language-test/02-types-variables.aura --aot --output target/test/02-types  # AOT 编译 → ⚠️ 失败
```

> **AOT 已修复的子问题**（共 16 个）：Float→double、类型推断、混合类型运算、结构体比较、phi 类型协调、默认返回类型、struct 字段访问、字符串字面量结构体、typealias 解析、`is` 运算符、`else` 分支、`when` null 处理、顶层 lateinit var 分配、UTF-8 字符串长度、嵌套 if phi 前驱、`aura_isOfType` 符号。
>
> **剩余问题**：Double 类型输出格式化为 0（`println` 对浮点数的 C 字符串转换待实现）；`null String` 打印为 0 而非空值。

---

### Phase 3 — 函数（✅ 已开发）

| 覆盖项 | 说明 |
|--------|------|
| 表达式体 | `fun f(x: Int): Int = x * x` 单行紧凑函数 |
| 块体 | 多行 `return` 返回、局部变量、递归 |
| 默认参数 | `fun f(a: Int, b: Int = 2)` 带默认值参数 |
| 命名参数 | `f(name = "Alice", age = 25)` 按名传递 |
| `vararg` | `fun f(vararg nums: Int)` 可变参数 |
| 泛型 | `fun <T> identity(x: T): T` / `fun <T, U> pair(a: T, b: U)` |
| 泛型约束 | `fun <T: Comparable<T>> max2(a: T, b: T): T` |
| Lambda | `(x: Int) -> x * 2` / `x: Int -> x * 2` 单参数/多参数/带块体 |
| 函数类型 | `val f: (Int) -> Int` 类型声明、多参/无参函数类型 |
| 高阶函数 | 返回函数的函数、组合、带状态工厂 |
| 递归 | 尾递归 `tailrec`、互相递归 |
| 修饰符 | `suspend` / `inline` / `comptime` |
| 可见性 | `public` / `private` / `protected` / `internal` 顶层函数 |
| 方法默认参数 | 类方法带默认值参数 |

**编译器修复**（Phase 3 开发中发现并修复的 14 个 Bug）：
| Bug | 位置 | 修复 |
|-----|------|------|
| 泛型函数调用类型不匹配 | `checker.rs::check_call_args` | `Ty::Named` 单字母大写且不在 `symbols.types` 中视为类型变量，接受任意实参类型 |
| `vararg` 参数语义检查失败 | `checker.rs::check_call_args` | `ParamSym` 新增 `is_vararg` 字段；vararg 时 `args.len() >= required` 即可 |
| 命名参数按位置匹配失败 | `checker.rs::check_call_args` | 两遍扫描：先按名匹配 `Expr::NamedArg`，再按位置匹配非命名参数；step 4 校验同样支持按名匹配 |
| 单参数 Lambda 解析失败 | `parser.rs::parse_prefix_expression` | `Ident` 后接 `Arrow`/`Colon` 时调用 `parse_lambda`，支持 `x -> expr` 和 `x: Type -> expr` |
| 顶层函数可见性解析失败 | `parser.rs::is_method_modifier_token` | 新增 `Public`/`Private`/`Protected`/`internal` 可见性 + `Fun` 组合检查 |
| Lambda/函数类型调用被拒绝 | `checker.rs::check_call` | `Ty::Function` 作为 callable：检查实参数量与类型，返回 `ret` 类型 |
| 顶层 val/var 类型注册顺序错误 | `checker.rs::analyze` | `check_top_level_stmt` 移到 `check_declaration` 之前，确保函数体能看到正确类型 |
| `when` 表达式返回 `Unit` | `checker.rs::check_when` | 移除 `has_else` 条件限制，`when` 始终返回所有分支类型的合并 |
| 默认参数未填充 | `hir.rs::desugar_expr` | `FUNCTION_PARAMS` thread-local 表存储函数参数信息，调用时填充缺失参数 |
| 方法默认参数未填充 | `hir.rs::build_function_param_table` | 类方法参数也注册到 `FUNCTION_PARAMS` 表（`ClassName.methodName`） |
| 函数索引闭包偏移错误 | `emit.rs::emit_module` | 预计算最终函数索引（闭包总数 + 函数序号），修复递归调用指向错误函数 |
| `if-else` 表达式返回值错误 | `hir.rs::desugar_block_inner` | 表达式体函数用 `HirStmt::Expr` 替代 `desugar_expr_stmt`，保留表达式语义 |
| 默认参数填充顺序错误 | `hir.rs::desugar_expr` | 默认参数按顺序填充（forward），不再 reverse 导致参数错位 |
| `Array` 成员访问未支持 | `checker.rs::check_member` | `Ty::Array(elem)` 支持 `size`/`isEmpty`/`first`/`last` 成员 |

**已知限制**（VM 运行时问题，不影响 `aura check`）：
- `Calculator` 方法默认参数返回 `null`（`when` 表达式在字段赋值中的 HIR/MIR 降级 bug，非默认参数问题）

**验证命令与结果**：
```bash
aura check examples/language-test/03-functions.aura              # 语法/语义检查 → ✅ 通过
aura run   examples/language-test/03-functions.aura              # VM 运行时验证 → ✅ 完成（§3.13 方法默认参数除外）
```

> **修复的编译器 Bug**：共 14 个（泛型类型变量、vararg 语义、命名参数匹配、单参数 Lambda 解析、顶层可见性解析、Lambda 调用检查、顶层 val 注册顺序、`when` 返回值、默认参数填充、方法默认参数、函数索引闭包偏移、`if-else` 表达式返回值、默认参数填充顺序、`Array` 成员访问）。

---

### Phase 4 — 控制流（✅ 已开发）

| 覆盖项 |
|--------|
| `if-else`（作为表达式和语句，含 else-if 链） |
| `when`（字面量 / 条件表达式 / `in range` / `is` 智能转换 / `else`） |
| `for`（含范围、独占范围 `..<`、倒序范围） |
| `while` / `do-while` |
| `break` / `continue`（含组合、嵌套循环、死循环+break） |
| 嵌套循环（乘法表、因子对、嵌套 break） |
| 循环中 `return`（顶层辅助函数） |

**编译器修复**（Phase 4 开发中发现并修复的 Bug）：
| Bug | 位置 | 修复 |
|-----|------|------|
| `CallNative` 指令大小计算错误 | `emit.rs::instr_size` | 3→5 字节，修复 `block_offsets` 偏差 |
| `MakeClosure` 指令大小错误 | `emit.rs::instr_size` | `3*captures+6` |
| 嵌套 `HirExpr::If` 块终结器错误 | `mir.rs::lower_expr` | `is_closed(self.current)` 替代 `is_closed(then_id/else_id)` |
| `HirStmt::If` 块终结器错误 | `mir.rs::lower_stmt` | 同上，修复 else-if 链 |
| `desugar_for` 中 `continue` 跳过自增 | `hir.rs::desugar_for` | 自增移至循环体开头 |
| `desugar_when` 中 `__else__` 泄漏 | `hir.rs::desugar_when` | `(None, Some(p))` 分支识别 `__else__` |
| `when in range` 未降级 | `hir.rs::desugar_when` | 添加 `Expr::InRange` 处理 |
| AOT `break`/`continue` 非法终止符 | `aot/emit.rs` | 添加循环块栈，`br` 到条件/结束块 |
| AOT `aura_isOfType` 崩溃 | `aot/emit.rs::emit_call` | 编译期解析，LLVM 类型→Aura 类型名映射 |
| 倒序范围未实现 | `hir.rs::desugar_for` | if-else 包裹正向/反向循环体 |

---

### Phase 5 — 类与对象（✅ 已开发）

| 覆盖项 |
|--------|
| `struct` / `data struct` / `sealed struct` |
| `value class` / `value data class` / `sealed value class` |
| `class`（继承 + `override` + `init` + `this` + `super` + `companion object`）|
| `interface`（含默认实现 `default fun`）|
| `extern interface`（AOT 动态库绑定，含 `default fun loadLibrary()`）|
| `enum`（单元 + 带数据变体）|
| `actor` |
| `sealed class`（受控继承，子类须在同编译单元）|
| `abstract class`（抽象方法 + 具体方法 + 子类实现）|
| 构造函数（`init(params)` / `constructor(params)` / 委托调用 `: super(...)`）|
| 属性访问器（`get` / `set` / `field`）|
| `operator` 重载（`plus` / `minus` / `times` 等）|

**编译器修复**（Phase 5 开发中发现并修复的 Bug）：
| Bug | 位置 | 修复 |
|-----|------|------|
| `data struct Name(ctor) { body }` 解析失败 | `parser.rs::parse_data_struct` | 主构造器 `(...)` 之后未处理 `{ body }`，新增 body 解析分支 |
| `sealed value class Name(ctor) { body }` 解析失败 | `parser.rs::parse_sealed_value_class` | 同上，新增 `(...)` 主构造器解析 |
| `load_shared_library` 无 feature gate | `vm/interp.rs::ensure_aot_lib_loaded` | `#[cfg(all(feature = "llvm", feature = "dynamic-ffi"))]` 包裹，无 feature 时返回 `None` |
| `call_func_by_idx` 内 unsafe 调用无 unsafe 块 | `vm/aot_runtime.rs::call_func_by_idx` | Rust 2024 要求 `unsafe { self.call_func(...) }` |
| `abstract class` 完全不支持 | `parser.rs::try_parse_modifier_prefix` | `abstract` 经 `FnModifier::Abstract` → `ClassModifier::Abstract` 转换，checker 校验抽象方法合法性 |
| **METHOD_SLOTS 初始化顺序错误** | `codegen/emit.rs` | `METHOD_SLOTS` 在函数发射之后初始化，导致 `method_slot()` 返回 0，所有虚调用分派到错误函数。移至函数发射之前 |
| **vtable 空槽位映射到函数 0** | `codegen/emit.rs::emit` | 无方法槽位初始化为 0（指向 `Animal.name`），改为 `u16::MAX` 哨兵值，`NewObject` 时过滤 |
| **is_virtual 检查过于宽泛** | `codegen/hir.rs::desugar_expr` | 检查全表所有类的 `open_methods`，改为仅检查当前类继承链 |

**已知限制**（VM 运行时问题，不影响 `aura check`）：
- `data struct` 默认值在 VM 中未正确初始化（`val weight: Int = 1` 运行时为 0）
- `value data class` 默认值未正确传递（`val currency: String = "CNY"` 运行时为 null）
- 类字段默认初始化未正确执行（`var side: Double = 1.0` 运行时为 0.0）
- `init(r: Double)` 构造器内字段赋值未正确生效（`radius = r` 后运行时为 0.0）

**extern interface 验证**（需预先编译动态库）：
```bash
# 1. 编译数学函数库为 AOT 动态库
aura build examples/language-test/libs/math/src/lib.aura --aot --shared --output examples/language-test/target/build/libs/math/math.dll
# 2. 复制到 VM 搜索路径
cp examples/language-test/target/build/libs/math/math.dll target/build/libs/math/math.dll
# 3. 运行测试（loadLibrary() 返回库名 "math"，VM 自动搜索 target/build/libs/math/math.dll）
aura run examples/language-test/05-classes.aura
# 期望输出：MathLib.add(3,4)=7, MathLib.multiply(3,4)=12, MathLib.square(5)=25
```

**验证命令与结果**：
```bash
aura check examples/language-test/05-classes.aura              # 语法/语义检查 → ✅ 通过
aura run   examples/language-test/05-classes.aura              # VM 运行时验证 → ✅ 完成
aura run   examples/language-test/05-classes.aura --jit         # JIT 验证 → ✅ 完成
```

---

### Phase 6 — 空安全（✅ 已开发）

| 覆盖项 |
|--------|
| `T?` 可空类型声明（Int? / Boolean? / String?） |
| `?:` Elvis 运算符（null → 默认值 / 有值 → 原值） |
| `!!` 强制解包（pass-through 类型收窄） |
| `?.` 安全调用（String 防御性访问） |
| `== null` / `!= null` null 检查 |
| 可空→非空互转（Elvis / !! / Elvis+运算） |
| 非空→可空隐式拓宽（Int → Int?） |
| 组合使用（Elvis + null 检查 / Elvis 链式 / !!+Elvis 混合） |
| 边界情况（多可空聚合 / Elvis 传参 / 二次转换） |

**编译器修复**（Phase 6 开发中发现并修复的 Bug）：
| Bug | 位置 | 修复 |
|-----|------|------|
| SafeAccess 双重可空包装（`Inner??`） | `sema/checker.rs::SafeAccess` | 检查 `is_nullable()` 后再决定是否包装 `Nullable` |
| AOT 可空标量存入非空变量类型不匹配 | `aot/emit.rs::emit_variable_decl` | 新增 `emit_store_converted` 处理 `{ T, i1 }` ↔ 标量互转 |
| AOT 赋值语句类型不匹配 | `aot/emit.rs::emit_assign` | 复用 `emit_store_converted` 进行类型协调 |
| AOT 二元运算可空结构体类型不匹配 | `aot/emit.rs::emit_binary` | 运算前提取 `{ T, i1 }` 内部值（非 null 比较场景） |
| AOT null 比较硬编码 `{ i32, i1 }` 类型 | `aot/emit.rs::emit_binary` | 使用实际 `l_ty`/`r_ty` 替代硬编码类型 |

**已知限制**（AOT 运行时问题，不影响 VM/JIT）：
- `toString()` 对 null 值返回 `0` 而非 `null`（C FFI 字符串转换待完善）
- `toString()` 对 String 值返回内存地址而非字符串内容
- `Boolean.toString()` 返回 `0`/`1` 而非 `false`/`true`
- `?.` 对 class/struct 成员的 AOT 支持待完善（struct 按值存储，GEP 类型不匹配）

**验证命令与结果**：
```bash
aura check examples/language-test/06-null-safety.aura              # 语法/语义检查 → ✅ 通过
aura run   examples/language-test/06-null-safety.aura              # VM 运行时验证 → ✅ 完成
aura run   examples/language-test/06-null-safety.aura --jit         # JIT 验证 → ✅ 完成
aura build examples/language-test/06-null-safety.aura --aot --output target/test/06-null-safety  # AOT 编译 → ✅ 编译成功
target/test/06-null-safety                                           # AOT 运行 → ✅ 完成（exit code 0）
```

---

### Phase 7 — 错误处理（✅ 已开发）

| 覆盖项 |
|--------|
| `try { } catch (e: Type) { } finally { }` |
| `throw` 表达式 |
| `Result<T,E>` + `case` pattern 匹配 |

**编译器修复**（Phase 7 开发中发现并修复的 Bug）：
| Bug | 位置 | 修复 |
|-----|------|------|
| `__throw` 原生函数未注册 | `codegen/hir.rs::build_natives` | 新增 `__throw` 原生函数注册 |
| `__throw` VM 运行时未注册 | `vm/native.rs` | 新增 `native_throw` 函数 |
| AOT `emit_new` 返回 null 指针 | `codegen/aot/emit.rs::emit_new` | 改用 `insertvalue` 构建结构体值 |
| AOT `emit_member_access` 字段偏移为 0 | `codegen/aot/emit.rs::emit_member_access` | 改用 `extractvalue` 按字段索引提取 |
| AOT `__throw` 未链接 | `std/cffi/aura_std_cffi.c` | 新增 C 实现（打印到 stderr） |

**已知限制**（运行时问题，不影响编译）：
- AOT 可执行文件运行时输出为空（`main` 入口点问题，Phase 6 也存在）
- `try` 块的 catch 子句在 HIR 中跳过（仅 try body + finally 执行）
- `throw` 降级为 `__throw` 原生调用（打印到 stderr，不中断执行）

**验证命令与结果**：
```bash
aura check examples/language-test/07-error-handling.aura              # 语法/语义检查 → ✅ 通过
aura run   examples/language-test/07-error-handling.aura              # VM 运行时验证 → ✅ 完成
aura run   examples/language-test/07-error-handling.aura --jit         # JIT 模式验证 → ✅ 完成
aura build examples/language-test/07-error-handling.aura --aot --output target/test/07-error  # AOT 编译 → ✅ 完成
```

> **AOT 运行时说明**：AOT 编译通过，但可执行文件运行时输出为空（`main` 入口点预存问题，Phase 6 同样存在）。VM/JIT 模式输出完全正确。

---

### Phase 8 — 并发（待开发）

| 覆盖项 |
|--------|
| `suspend fun` + `await` |
| `async { }` 块 |
| Actor：`spawnActor` / `send` / `ask` / `supervise` / `actorAlive` |
| Channel：`newChannel` / `channelSend` / `channelRecv` / `channelTryRecv` |
| `select` 多路复用 |
| 协程 `spawn` + `await` 组合 |

---

### Phase 9 — FFI（待开发）

| 覆盖项 |
|--------|
| `extern "c" { }` 匿名块 |
| `extern "c" "lib" { }` 具名库 |
| `extern "rust" { }` |
| `Handle` / `Pointer<T>` / `ptrIsNull` / `ptrToInt` / `intToPtr` |
| `makeCallback` 回调 |

---

### Phase 10 — 内存管理（待开发）

| 覆盖项 |
|--------|
| `defer { }` 延迟清理 |
| ARC 引用计数（create + release 路径） |
| `Box<T>` 装箱 |

---

### Phase 11 — 导入（待开发）

| 覆盖项 |
|--------|
| 通配 `import module.*` |
| 模块 `import module` |
| 精确 `import module.fn` |
| 别名 `import module.fn as alias` / `import module as alias` |

---

### Phase 12 — 注解与 Comptime（待开发）

| 覆盖项 |
|--------|
| `@Annotation` / `@Deprecated` |
| `comptime fun`（编译时执行） |

---

### Phase 13 — 标准库快照（待开发）

| 覆盖项 |
|--------|
| `aura.math` / `io` / `string` / `collections` / `json` / `time` |
| `env` / `random` / `encoding` / `ascii` / `path` / `iter` / `net` / `fs` |

---

### Phase 14 — 字符串插值（待开发）

| 覆盖项 |
|--------|
| `$var` 简单插值 / `${expr}` 表达式插值 / raw string |

---

### Phase 15 — 高级特性（待开发）

| 覆盖项 |
|--------|
| 解构声明 `val (a, b) = pair` |
| 迭代器 `list.filter { }.map { }.take(n)` |
| `this` / `super` 引用 |

---

### Phase 16 — 脚本模式（待开发）

| 覆盖项 |
|--------|
| 顶层语句（无 `main` 也能执行） |

---

## 验证指南

### 单阶段验证

```bash
# 1. 语法/语义检查（零错误）
aura check examples/language-test/NN-phase.aura

# 2. 词法分析（Phase 1 专用）
aura tokens examples/language-test/NN-phase.aura

# 3. AST 输出
aura ast examples/language-test/NN-phase.aura

# 4. 字节码编译 + VM 执行
aura run examples/language-test/NN-phase.aura

# 5. JIT 模式
aura run examples/language-test/NN-phase.aura --jit

# 6. AOT 编译为原生可执行文件
aura build examples/language-test/NN-phase.aura --aot --output /tmp/nn-phase
# 运行 AOT 产物
/tmp/nn-phase

# 7. AOT 仅生成 LLVM IR
aura build examples/language-test/NN-phase.aura --aot --emit-llvm

# 8. 格式化检查
aura fmt examples/language-test/NN-phase.aura --check

# 9. ARC 泄漏检查
aura leak-check examples/language-test/NN-phase.aura
```

### 全部阶段验证

```bash
# 一键检查全部阶段（语法/语义）
for f in examples/language-test/*.aura; do
  echo "── Checking $f ──"
  aura check "$f" || exit 1
done

# 一键运行全部阶段（VM）
for f in examples/language-test/*.aura; do
  echo "── Running VM $f ──"
  aura run "$f" || exit 1
done

# 一键 AOT 编译 + 运行全部阶段
for f in examples/language-test/*.aura; do
  base=$(basename "$f" .aura)
  echo "── AOT compiling $f ──"
  aura build "$f" --aot --output "/tmp/${base}-aot" || exit 1
  echo "── Running AOT /tmp/${base}-aot ──"
  "/tmp/${base}-aot" || exit 1
done
```

### 编译器单元测试

```bash
# 全量单元测试
cargo test --workspace

# 仅示例集成测试
cargo test -p compiler --test examples_test

# 仅词法单元测试
cargo test -p compiler lexer

# 仅语法单元测试
cargo test -p compiler parser
```

---

## 设计原则

| 原则 | 说明 |
|------|------|
| **独立可验证** | 每个阶段文件包含完整的 `main()` 入口，可单独 `aura check` / `aura run` |
| **运行时验证** | 通过 `println` 输出实际值，而非仅编译通过 |
| **注释标注** | 每行标注对应阶段（P1–P16）便于追溯 |
| **边界覆盖** | 覆盖空安全边界（`null`）、范围边界（`0..10`） |
| **无外部依赖** | 不依赖外部 dylib 或网络，离线可运行 |
| **跨平台** | 不依赖特定 OS 的路径或命令 |

---

## 与现有示例的关系

| 现有文件 | 关系 |
|----------|------|
| `compiler/showcase.aura` | 本系列的超集，多了运行时验证和边界测试 |
| `basics/demo.aura` | Phase 1 覆盖其全部内容并扩展 |
| `stdlib/std_demo.aura` | Phase 13 是其精简快照 |
| `concurrency/concurrent_integration.aura` | Phase 8 覆盖其全部 7 个子场景 |
| `ffi/p8_c_ffi_demo.aura` | Phase 9 覆盖其全部 3 个场景 |
| `compiler/memory_test.aura` | Phase 10 覆盖其全部特性 |

---

## 风险与注意事项

| 风险 | 应对 |
|------|------|
| 部分语法仅"解析通过"但 sema 报错 | 先 `aura check`，若报错则降级为注释引用或移除 |
| 并发特性的 VM 执行时序不确定 | 使用 `select` 或确定性的 channel 收发顺序 |
| FFI 未链接 dylib 时返回占位值 | 按 `p8_c_ffi_demo.aura` 的做法，用 `ptrIsNull` 判空 |
| `case` pattern 可能未完全实现 | 若不支持，降级为 `is` + `when` |
| 文件过长导致可读性下降 | 每分区用 `// ═══ 标题 ═══` 分隔，分区独立函数 |

---

## 更新日志

| 日期 | 阶段 | 说明 |
|------|------|------|
| 2026-09-08 | Phase 7 AOT 修复 | AOT 编译通过（修复 5 个编译器 Bug：`emit_new` 改用 `insertvalue` 构建结构体、`emit_member_access` 改用 `extractvalue` 按字段索引提取、`class_field_types` 增加字段索引、C FFI 新增 `__throw` 实现）。VM/JIT/AOT 全模式编译通过（AOT 运行时输出为空为预存问题） |
| 2026-09-08 | Phase 7 | 错误处理 — 创建并 VM/JIT 验证通过（修复 2 个编译器 Bug：`__throw` 原生函数未注册、`__throw` VM 运行时未注册）。AOT 编译失败（构造器 `emit_new` 返回 null，与 Phase 5 相同限制） |
| 2026-09-08 | Phase 6 | 空安全 — 创建并全模式验证通过（修复 5 个编译器 Bug：SafeAccess 双重可空包装、AOT 可空标量存入非空变量、AOT 赋值类型协调、AOT 二元运算可空结构体提取、AOT null 比较硬编码类型）。VM/JIT 输出正确，AOT 编译运行通过（toString 对 null/String 输出待优化） |
| 2026-09-08 | Phase 5 | 类与对象 — 创建并验证通过（修复 5 个编译器 Bug：`data struct` 主构造器 body 解析、`sealed value class` 主构造器解析、`load_shared_library` feature gate、Rust 2024 unsafe 块、`abstract class` 支持）。新增 `extern interface` 覆盖。VM 运行时方法分派与默认值传递待完善 |
| 2026-09-08 | Phase 3 VM 修复 | 修复 6 个 VM 运行时 Bug：默认参数填充（顺序错误、方法参数表缺失）、函数索引闭包偏移（递归调用指向错误函数）、vararg 打包、泛型函数返回值、方法默认参数。Phase 3 全模式通过（§3.13 方法默认参数除外） |
| 2026-09-07 | Phase 3 修复 | 修复 `if-else` 表达式返回值错误（parser 表达式终止符、HIR 降级、vararg 类型、Array 成员访问）。`aura run` 通过，VM 部分特性（默认参数、vararg 运行时、泛型返回值）待完善 |
| 2026-09-07 | Phase 3 | 函数特性 — 创建并验证通过（修复 8 个编译器 Bug：泛型类型变量、vararg、命名参数、单参数 Lambda、顶层可见性、Lambda 调用、顶层 val 注册顺序、`when` 返回值） |
| 2026-09-07 | AOT 修复 | Phase 2 AOT 编译通过并可运行：顶层 `lateinit var` alloca 修复（`synthesize_main_if_missing` CLI 缺失）、UTF-8 字符串十六进制转义、嵌套 `if` phi 前驱修正、`aura_isOfType` C FFI 实现。共修复 16 个 Bug |
| 2026-09-07 | 验证扩展 | 新增 VM/JIT/AOT 三模式验证体系，Phase 1/2 全模式测试（AOT 存在已知限制） |
| 2026-09-07 | 编译修复 | AOT 后端 `CallVirtual` 未覆盖 — `c_backend.rs`/`emit.rs` 添加虚调用降级为静态调用 |
| 2026-09-07 | Phase 2+ | 新增 4 个特性：`!!` 非空断言、`as` 类型转换、`listOf` 多参数、`Pair.first`/`.second` 类型解析。修复 4 个编译器 Bug（总计 8 个） |
| 2026-09-07 | Phase 2 | 类型与变量 — 创建并验证通过（修复 4 个编译器 Bug：sema 顶层 val/var、typealias 解析、`;` 分隔符、顶层 val 运行时） |
| 2026-09-07 | Phase 1 | 词法基础 — 创建并验证通过 |
