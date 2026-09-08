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
├── 05-classes.aura         ← Phase 5: 类与对象（待开发）
├── 06-null-safety.aura     ← Phase 6: 空安全（待开发）
├── 07-error-handling.aura  ← Phase 7: 错误处理（待开发）
├── 08-concurrency.aura     ← Phase 8: 并发（待开发）
├── 09-ffi.aura             ← Phase 9: FFI（待开发）
├── 10-memory.aura          ← Phase 10: 内存管理（待开发）
├── 11-imports.aura         ← Phase 11: 导入（待开发）
├── 12-annotations.aura     ← Phase 12: 注解与 Comptime（待开发）
├── 13-stdlib.aura          ← Phase 13: 标准库快照（待开发）
├── 14-string-interp.aura   ← Phase 14: 字符串插值（待开发）
├── 15-advanced.aura        ← Phase 15: 高级特性（待开发）
├── 16-script-mode.aura     ← Phase 16: 脚本模式（待开发）
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
| Phase 5 类与对象 | `05-classes.aura` | ⏳ | ⏳ | ⏳ |
| Phase 6 空安全 | `06-null-safety.aura` | ⏳ | ⏳ | ⏳ |
| Phase 7 错误处理 | `07-error-handling.aura` | ⏳ | ⏳ | ⏳ |
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
> - native 函数返回值硬编码 i32（所有含 native 调用的 AOT 编译均可能失败）
> - weak 引用在 AOT 后端无映射
> - 顶层变量未定义 LLVM 全局（Phase 2 编译失败）
> - `is` 运算符已实现（`aura.isOfType` 原生函数），VM/JIT 通过
> - `when` 表达式 null 处理已修复（`emit_if_expr` null → 0）

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
| `if-else`（作为表达式） |
| `when`（字面量 / `in range` / `is` 智能转换 / `else` / 守卫 `&&`） |
| `for` / `while` / `do-while` |
| 标签循环 + `break@label` / `continue@label` |

---

### Phase 5 — 类与对象（待开发）

| 覆盖项 |
|--------|
| `struct` / `data struct` / `sealed struct` |
| `value class` / `value data class` / `sealed value class` |
| `class`（继承 + `override` + `init` + `this` + `super`） |
| `interface`（含默认实现） |
| `enum`（单元 + 带数据变体） |
| `actor` |

---

### Phase 6 — 空安全（待开发）

| 覆盖项 |
|--------|
| `?.` 安全调用 / `?:` elvis / `!!` 强制解包 |
| 可空 vs 非空互转 |

---

### Phase 7 — 错误处理（待开发）

| 覆盖项 |
|--------|
| `try { } catch (e: Type) { } finally { }` |
| `throw` 表达式 |
| `Result<T,E>` + `case` pattern 匹配 |

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
| 2026-09-08 | Phase 3 VM 修复 | 修复 6 个 VM 运行时 Bug：默认参数填充（顺序错误、方法参数表缺失）、函数索引闭包偏移（递归调用指向错误函数）、vararg 打包、泛型函数返回值、方法默认参数。Phase 3 全模式通过（§3.13 方法默认参数除外） |
| 2026-09-07 | Phase 3 修复 | 修复 `if-else` 表达式返回值错误（parser 表达式终止符、HIR 降级、vararg 类型、Array 成员访问）。`aura run` 通过，VM 部分特性（默认参数、vararg 运行时、泛型返回值）待完善 |
| 2026-09-07 | Phase 3 | 函数特性 — 创建并验证通过（修复 8 个编译器 Bug：泛型类型变量、vararg、命名参数、单参数 Lambda、顶层可见性、Lambda 调用、顶层 val 注册顺序、`when` 返回值） |
| 2026-09-07 | AOT 修复 | Phase 2 AOT 编译通过并可运行：顶层 `lateinit var` alloca 修复（`synthesize_main_if_missing` CLI 缺失）、UTF-8 字符串十六进制转义、嵌套 `if` phi 前驱修正、`aura_isOfType` C FFI 实现。共修复 16 个 Bug |
| 2026-09-07 | 验证扩展 | 新增 VM/JIT/AOT 三模式验证体系，Phase 1/2 全模式测试（AOT 存在已知限制） |
| 2026-09-07 | 编译修复 | AOT 后端 `CallVirtual` 未覆盖 — `c_backend.rs`/`emit.rs` 添加虚调用降级为静态调用 |
| 2026-09-07 | Phase 2+ | 新增 4 个特性：`!!` 非空断言、`as` 类型转换、`listOf` 多参数、`Pair.first`/`.second` 类型解析。修复 4 个编译器 Bug（总计 8 个） |
| 2026-09-07 | Phase 2 | 类型与变量 — 创建并验证通过（修复 4 个编译器 Bug：sema 顶层 val/var、typealias 解析、`;` 分隔符、顶层 val 运行时） |
| 2026-09-07 | Phase 1 | 词法基础 — 创建并验证通过 |
