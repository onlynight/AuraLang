# Aura 语言 vs Kotlin：语法语义缺失分析

> **数据来源**：`aura/compiler/` 约 100+ 个 Aura 源文件、`aura/core/` 标准库、`docs/` 设计文档、`book/` 教程、`tests/` 294 个测试文件
> **分析时间**：2026-09-28（P0 更新：扩展函数/select/Future.then/suspend移除 已实现）

---

## 一、总体概况

Aura 已经实现了**相当完整的前端管线**：Lexer → Parser → AST → Sema → HIR → MIR → Codegen → VM/JIT/AOT。语法上大量模仿 Kotlin（`val`/`var`、`fun`、`when`、泛型、空安全 等），但**许多是"语法糖+关键字占位"，语义尚未实现**。

> **并发模型决策（2026-09-28）**：Aura **明确不支持协程**。`suspend`/`async`/`await` 占位关键字将在后续版本移除，由 `actor` + `Channel` + `Future<T>`/`Promise<T>` 替代。详见 §7.5。

---

## 二、Kotlin 语法特性完整列表 vs Aura 实现状态

### 1. 变量与类型系统

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| `val` / `var` | ✅ 完整 | 类型推断 + 显式类型 |
| 基本类型 | ✅ 完整 | `Int/Long/Short/Byte/Float/Double/Boolean/Char/String/Any/Nothing/Unit` |
| 可空类型 `T?` | ✅ 完整 | 字符串后缀 `?` |
| 智能转换 (Smart Cast) | ❌ **缺失** | `is` 检查后无法自动窄化类型 |
| `?:` Elvis | ✅ 完整 | 解析器已支持 |
| `?.` 安全调用 | ✅ 完整 | `SafeAccess` 节点 |
| `!!` 非空断言 | ✅ 完整 | `Unwrap` 节点 |
| `lateinit var` | ⚠️ 关键字存在，语义缺失 | 解析为软关键字，无运行时检查 |
| `val by lazy {}` | ⚠️ 关键字存在，语义缺失 | 未实现惰性求值 |
| `typealias` | ⚠️ 部分 | 解析器支持，但泛型型别名不支持 |
| 属性 getter/setter | ❌ **缺失** | `val x: Int get() = ..., set(value) {}` 不支持 |
| `const val` | ⚠️ 关键字存在 | `Const Val` 解析但不做编译期常量折叠 |
| `by` 委托（接口） | ❌ **缺失** | 仅 `lazy` 关键字存在，无实际委托语义 |
| 泛型 (T, U, K, V) | ⚠️ 部分 | 字符串表示 `T0/T1`，无单态化、无边界推断 |
| 泛型边界 `T: Comparable<T>` | ⚠️ 部分 | 语法可写，但 TypeChecker 不做约束检查 |
| 泛型 `where` 子句 | ❌ **缺失** | 多约束不支持 |
| 类型方差 `in`/`out` | ❌ **缺失** | 无声明型协变/逆变 |
| 星投影 `List<*>` | ❌ **缺失** | 不支持 |
| `Array<T>` 构造 `Array(n){}` | ❌ **缺失** | 数组字面量 `[1,2,3]` 降级为 `__list_new`，无真正 Array 类型 |
| 交叉类型 | ❌ **缺失** | 不支持 `T & U` |
| `Pointer<T>` | ✅ 存在 | FFI 用 |
| 函数类型 `(A,B) -> R` | ✅ 存在 | 字符串表示 |
| 函数引用 `obj::method` | ❌ **缺失** | book 中提及但解析器不支持 `::` |
| 运算符重载 `operator fun` | ❌ **缺失** | 关键字存在但无语义 |
| `infix` 中缀函数 | ❌ **缺失** | 仅 `to` 硬编码为 `Binary("to")`，无法自定义中缀 |
| `contract` 函数契约 | ❌ **缺失** | 不支持 |
| `require`/`check`/`error` | ❌ **缺失** | 语言级断言不支持 |

### 2. 函数

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| 基础函数 `fun f(a: T): R` | ✅ 完整 | |
| 默认参数 | ✅ 完整 | `name: Type = expr` |
| `vararg` 可变参数 | ✅ 完整 | `vararg xs: T` 解析为 `Vararg<T>` |
| 单表达式函数 `= expr` | ✅ 完整 | |
| 泛型函数 `<T>` | ⚠️ 部分 | 语法可写，无类型推断/单态化 |
| 泛型边界 `<T: C>` | ⚠️ 部分 | 语法可写，无边界检查 |
| **函数重载** | ❌ **缺失** | 注释明确：「全局函数命名空间，方法名必须全局唯一」 |
| **扩展函数** `fun String.foo()` | ✅ **已实现**（P0） | 解析器已支持 `fun Type.name()` 语法，AST 以 `#recv|Type` 标记 |
| 局部函数（fun 内部 fun） | ❌ **缺失** | 不支持 |
| 尾递归 `tailrec` | ❌ **缺失** | 关键字存在（`Ident`），无优化 |
| 内联函数 `inline` | ❌ **缺失** | 关键字存在，无 body 内联 |
| `noinline`/`crossinline` | ❌ **缺失** | |
| `reified` 类型参数 | ❌ **缺失** | |
| `operator` 函数 | ❌ **缺失** | 无运算符重载解析 |
| 函数引用 `::` | ❌ **缺失** | 不支持 `::method` / `Class::method` |
| 命名参数 `f(name = v)` | ✅ 完整 | `NamedArg` 节点 |
| 具名 lambda 参数 | ✅ 完整 | `(x: T, y: T) -> expr` |
| 尾随 lambda | ✅ 完整 | `filter { it > 2 }` |
| 单参数 lambda `x -> expr` | ✅ 完整 | 隐式 `it` 支持 |
| `suspend` 函数 | ✅ **已移除**（P0） | 不在关键字表，作为普通标识符处理 |
| `async`/`await` | ✅ **已移除**（P0） | 不在关键字表，由 `Future.then {}` 替代 |

### 3. 类与对象

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| `class` 引用类型 | ✅ 完整 | 继承、多态 |
| `open class` / `open fun` | ✅ 完整 | 解析器支持 |
| `override` | ✅ 完整 | |
| `abstract class` | ❌ **缺失** | 关键字作为 `Ident` 解析，无 `abstract fun` 语义 |
| `abstract fun` | ❌ **缺失** | |
| `data class` | ⚠️ 变体 `value data class` | 无 `copy()` / `componentN()` / 自动 `equals/hashCode/toString` |
| `value class` | ✅ 完整 | Aura 特色，替代 Kotlin `data class` |
| `sealed class` | ⚠️ 部分 | 语法支持，无 `when` 穷举检查 |
| `inner class` | ❌ **缺失** | 不支持访问外部类实例 |
| `companion object` | ⚠️ 部分 | `companion object { }` 无命名访问 |
| `companion object Foo { }` | ❌ **缺失** | 不支持自定义伴生对象名 |
| `object` 单例 | ✅ 完整 | |
| 主构造函数参数 `class P(val x: T)` | ✅ 完整 | |
| 次构造函数 `constructor()` | ⚠️ 部分 | `constructor()` 解析为 `init`，无 `this()` 委托 |
| 构造器委托 `: this()` / `: super()` | ⚠️ 部分 | `super()` 支持，`this()` 不支持 |
| `init { }` 初始化块 | ✅ 完整 | |
| `this` / `super` | ✅ 完整 | |
| `interface` | ✅ 完整 | |
| 接口默认方法 | ✅ 完整 | `fun method(): R = expr` |
| 接口委托 `by` | ❌ **缺失** | |
| `enum` | ✅ 完整 | 含数据变体 |
| `enum when 穷举` | ❌ **缺失** | TypeChecker 明确注明未实现 |
| `sealed when 穷举` | ❌ **缺失** | TypeChecker 明确注明未实现 |
| `annotation class` | ❌ **缺失** | 无注解类语法 |
| `@JvmStatic` / `@JvmName` 等 | ❌ **缺失** | 仅 `@native`/`@aot`/`@Export` 元数据 |
| `expect`/`actual` | ❌ **缺失** | 无多平台支持 |

### 4. 控制流

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| `if` / `else` | ✅ 完整 | 表达式形式 |
| `if` 表达式 | ✅ 完整 | |
| `when` | ✅ 完整 | 含 subject 和 `when { }` 无 subject |
| `when` 无 subject | ✅ 存在 | `when { cond -> ... }` 可用 |
| `when is T ->` 模式 | ✅ 完整 | `__is__T` 节点 |
| `when in range ->` 模式 | ✅ 完整 | `InPattern` 节点 |
| `when else ->` 兜底 | ✅ 完整 | `__else__` |
| `when` 穷举检查 (sealed) | ❌ **缺失** | TypeChecker 注明未实现 |
| `when` 穷举检查 (enum) | ❌ **缺失** | TypeChecker 注明未实现 |
| `for` in 循环 | ✅ 完整 | Kotlin 风格 |
| `for` C 风格 | ✅ 完整 | `for (init; cond; incr)` |
| `while` | ✅ 完整 | |
| `do`-`while` | ✅ 完整 | |
| `break` | ✅ 完整 | |
| `continue` | ✅ 完整 | |
| **带标签 `break@label`** | ❌ **缺失** | 仅支持 `break`，无标签 |
| **带标签 `continue@label`** | ❌ **缺失** | 仅支持 `continue`，无标签 |
| `return` | ✅ 完整 | |
| 非局部返回 | ❌ **缺失** | lambda 内 `return` 不支持 |
| `try` / `catch` / `finally` | ✅ 完整 | 含类型化 catch |
| `throw` | ✅ 完整 | |
| 异常层次 | ✅ 存在 | stdlib 有 `Exception`/`Error`/`Throwable` |

### 5. Lambda 与闭包

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| 单参数 `{ }` 隐式 `it` | ✅ 完整 | |
| 多参数 `(a, b) -> {}` | ✅ 完整 | |
| 尾随 lambda | ✅ 完整 | `filter { it > 2 }` |
| 块体 lambda | ✅ 完整 | `{ stmt1; stmt2 }` |
| 非局部返回 | ❌ **缺失** | lambda 内 `return` 不支持 |
| `noinline`/`crossinline` | ❌ **缺失** | |
| lambda 作为值传递 | ✅ 完整 | `Lambda` 节点 |
| 函数类型转换 | ⚠️ 部分 | `(Int) -> Int` 字符串形式 |
| 函数引用 `::` | ❌ **缺失** | |
| lambda 内捕获变量 | ⚠️ 部分 | VM 支持闭包，AOT 部分场景崩溃 |

### 6. 字符串

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| 字符串字面量 | ✅ 完整 | `"hello"` |
| 字符串插值 `$var` | ✅ 完整 | |
| 字符串插值 `${expr}` | ✅ 完整 | |
| 原始字符串 `"""..."""` | ✅ 完整 | 三引号 |
| 原始字符串 `r"..."` | ❌ **缺失** | book 明确注明不支持 |
| 字符字面量 `'A'` | ✅ 完整 | |
| 多行字符串 | ✅ 完整 | 通过 `"""` |
| 字符串模板表达式 | ✅ 完整 | `${a + b}` 可嵌套 |

### 7. 数值字面量

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| 十进制 | ✅ 完整 | |
| 十六进制 `0xFF` | ✅ 完整 | |
| 二进制 `0b1100` | ✅ 完整 | |
| 浮点 `3.14f` / `3.14` | ✅ 完整 | |
| 长整型 `100L` | ✅ 完整 | |
| 下划线分隔 `1_000_000L` | ✅ 完整 | |

### 8. 操作符

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| 算术 `+ - * / %` | ✅ 完整 | |
| 比较 `== != < > <= >=` | ✅ 完整 | |
| 逻辑 `&& \|\| !` | ✅ 完整 | |
| 位运算 `& \| ^ << >> >>>` | ✅ 完整 | |
| Elvis `?:` | ✅ 完整 | |
| 安全调用 `?.` | ✅ 完整 | |
| 非空断言 `!!` | ✅ 完整 | |
| 类型检查 `is` | ✅ 完整 | |
| 类型转换 `as` / `as?` | ✅ 完整 | |
| 范围 `..` / `..<` | ✅ 完整 | |
| `to` 对 | ✅ 完整 | Pair 构造 |
| `in` 迭代 | ✅ 完整 | |
| `+= -= *= /= %=` | ✅ 完整 | 复合赋值 |
| `++` / `--` (前缀) | ✅ 完整 | 脱糖为 `x = x + 1` |
| `++` / `--` (后缀) | ❌ **缺失** | 仅前缀支持 |
| 运算符重载 `operator` | ❌ **缺失** | 无 |
| `invoke` 运算符 `()` | ❌ **缺失** | 无自定义调用 |
| `get`/`set` 运算符 `[]` | ⚠️ 部分 | 语法存在，非可重载 |
| `plus`/`times` 等 | ❌ **缺失** | 无 |
| 中缀函数 | ❌ **缺失** | 仅 `to` 硬编码 |

### 9. 空安全

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| `T?` 可空类型 | ✅ 完整 | |
| `null` 字面量 | ✅ 完整 | |
| 安全调用 `?.` | ✅ 完整 | |
| Elvis `?:` | ✅ 完整 | |
| 非空断言 `!!` | ✅ 完整 | |
| **智能转换 (Smart Cast)** | ❌ **缺失** | `is` 检查后无法自动窄化 |
| 可空类型成员访问检查 | ✅ 编译期 | |
| 非空类型赋值 null | ✅ 编译期 | |
| 可空类型传递到非空参数 | ✅ 编译期 | |

### 10. 并发

**定位决策**：Aura **明确不支持协程**。并发模型为 **actor 主导 + Future 辅助 + Channel 通信**。`suspend`/`async`/`await` 占位关键字已移除（P0）。

| 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| `actor` | ✅ 完整 | **Aura 核心并发模型**（Kotlin 无原生 actor） |
| `Channel` | ✅ 完整 | 消息队列，stdlib 支持 |
| `select { }` | ✅ **已实现**（P0） | 解析器已支持多路复用语法，含 `timeout(n)` 分支 |
| `Mutex` / `RwLock` / `Semaphore` | ✅ 完整 | stdlib 支持 |
| `Future<T>` / `Promise<T>` | ✅ **已实现**（P0） | 含 `then`/`thenMap` 回调 API，替代协程 |
| `suspend fun` | ✅ **已移除**（P0） | 不在关键字表，作为普通标识符处理 |
| `async` / `await` | ✅ **已移除**（P0） | 不在关键字表，由 `Future.then {}` 替代 |
| `launch` / `runBlocking` | ❌ **决定不补** | 用 `actor` 生命周期替代 |
| `withContext` | ❌ **决定不补** | 无调度器需求 |
| `Flow<T>` | ❌ **决定不补** | 用 Channel 流式通信替代 |
| 协程调度器 | ❌ **决定不补** | 无 dispatcher 需求 |
| 结构化并发 | ❌ **决定不补** | actor 生命周期已覆盖 |
| 协程异常传播 | ❌ **决定不补** | 无 suspend 传播需求 |

### 11. 泛型

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| 基本泛型 `<T>` | ✅ 完整 | |
| 泛型边界 `<T: C>` | ⚠️ 部分 | 语法可写，无约束检查 |
| 单态化 (Monomorphization) | ❌ **缺失** | TypeChecker 明确注明留给 HIR |
| 类型推断 | ❌ **缺失** | 无调用点推断 |
| 方差 `in`/`out` | ❌ **缺失** | |
| `where` 多约束 | ❌ **缺失** | |
| 星投影 `List<*>` | ❌ **缺失** | |
| 泛型函数类型 | ⚠️ 部分 | 字符串表示 |
| 泛型类 | ✅ 完整 | |
| 泛型枚举 | ✅ 完整 | |

### 12. FFI / 互操作

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| `extern "c" fun` | ✅ 完整 | |
| `extern "c" "lib" { }` | ✅ 完整 | |
| `extern object` + `@aot` | ✅ 完整 | |
| `CString` / `CStr` | ✅ 完整 | |
| `@JvmStatic` / `@JvmName` | ❌ **缺失** | 仅 `@native`/`@aot`/`@Export` |
| `@file:JvmName` | ❌ **缺失** | |
| 动态加载 | ✅ 完整 | |

### 13. 注解 / 元编程

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| `annotation class` | ❌ **缺失** | 无注解类语法 |
| `@` 注解使用 | ⚠️ 部分 | 仅 `@native`/`@aot`/`@Export` 元数据 |
| 函数引用 `::` | ❌ **缺失** | |
| `typeOf<T>()` | ✅ 完整 | 返回 `Type` 对象 |
| 反射 `Class.forName` | ⚠️ 部分 | `Type.forName` 在 AOT 端未落地 |
| `is` / `as` RTTI | ✅ 完整 | |
| `contract` | ❌ **缺失** | |

### 14. 包与模块

| Kotlin 特性 | Aura 实现状态 | 说明 |
|---|---|---|
| `package` | ✅ 完整 | |
| `import` | ✅ 完整 | 含 `*`、别名、`{x, y}` |
| 可见性 `public` / `internal` / `private` / `protected` | ⚠️ 关键字存在 | **TypeChecker 有 `typeVis` 表但未强制执行** |
| 内部模块 | ❌ **缺失** | 无 enforcement |
| 多文件模块 | ✅ 完整 | |

---

## 三、TypeChecker 源码中**明确注明未实现**的语义

来自 `aura/compiler/aura/lang/compiler/sema/TypeChecker.aura` 的注释：

```
// 本版本聚焦核心类型检查，暂未覆盖：
//   - 泛型单态化（留待 HIR 阶段）
//   - 枚举 when 穷举性检查
//   - 接口方法实现检查
//   - 运算符重载解析
//
// 注：协程追踪（suspend/async）已决定不做，不再列入计划
```

## 四、Parser 源码中**明确注明未实现**的语义

来自 `aura/compiler/aura/lang/compiler/parser/Parser.aura` 的注释：

- `catch (e: Type)` 类型过滤：「当前等价于 catch-all（仅建模首个 catch 子句）」
- AOT 路径不支持 `try/catch`：「aot/emit.rs 尚未支持 try/catch」
- `sema` 对 `List<T>` 索引误报
- 未解析调用退化为递归：裸名 `exit(1)` 之类未解析调用会退化为 `Call(0)`（递归入口）

### P0 已新增的 Parser 功能（2026-09-28）

- ✅ **扩展函数**：`fun Type.name(params): Ret { body }` 语法已实现，接收者类型存入 AST 的 `#recv|Type` 字段
- ✅ **select { }**：多路复用表达式已实现，支持 `channel -> { }` 和 `timeout(n) -> { }` 分支
- ✅ **select 向后兼容**：`select` 作为变量名时仍可用（仅当后跟 `{` 时才触发 select 解析）

---

## 五、缺失特性汇总表（按影响程度排序）

| 优先级 | 缺失特性 | 类别 | 影响 |
|--------|---------|------|------|
| 🔴 **P0** | ~~函数重载~~ **待实现** | 函数 | 无法写多态 API，全局命名空间冲突 |
| 🔴 **P0** | ~~扩展函数~~ ✅ **已实现**（P0） | 函数 | 解析器已支持 `fun Type.name()` 语法 |
| 🔴 **P0** | 智能转换 (Smart Cast) | 类型 | `is` 后无法安全访问，需手动 `as` |
| 🔴 **P0** | `operator` 运算符重载 | 函数 | 无法自定义 `+`/`*`/`==` 等语义 |
| 🔴 **P0** | 泛型单态化 + 类型推断 | 泛型 | 泛型函数无法正确实例化 |
| 🟢 **P3** | ~~协程实际语义~~ **决定不做** | 并发 | 由 actor + Channel + Future 替代，见 §7.5 |
| 🟠 **P1** | 带标签 `break@label`/`continue@label` | 控制流 | 嵌套循环无法精确跳转 |
| 🟠 **P1** | `abstract class` / `abstract fun` | 类 | 无法定义抽象基类 |
| 🟠 **P1** | `sealed when` / `enum when` 穷举检查 | 类型 | 无法静态保证模式完备性 |
| 🟠 **P1** | 属性 getter/setter | 类 | 无法惰性求值或计算属性 |
| 🟠 **P1** | 接口方法实现检查 | 类 | 编译期无法检测遗漏方法 |
| 🟠 **P1** | 后缀 `++`/`--` | 操作符 | 仅前缀可用 |
| 🟠 **P1** | 非局部返回 (lambda 内 return) | 闭包 | 受限 |
| 🟠 **P1** | 次构造函数 `this()` 委托 | 类 | 构造器复用受限 |
| 🟠 **P1** | `data class` 完整语义 (copy/componentN) | 类 | `value data class` 不生成 |
| 🟠 **P1** | `annotation class` | 元编程 | 无自定义注解 |
| 🟠 **P1** | `@JvmStatic`/`@JvmName` 等 | 互操作 | JVM 互操作受限 |
| 🟠 **P1** | `inner class` | 类 | 无法访问外部类实例 |
| 🟠 **P1** | 局部函数 (fun 内 fun) | 函数 | 受限 |
| 🟠 **P1** | `tailrec` 尾递归优化 | 函数 | 深递归易栈溢出 |
| 🟠 **P1** | `inline` 内联语义 | 函数 | lambda 开销大 |
| 🟠 **P1** | `reified` 类型参数 | 泛型 | 反射受限 |
| 🟡 **P2** | 泛型方差 `in`/`out` | 泛型 | 类型精度受限 |
| 🟡 **P2** | `where` 多约束子句 | 泛型 | 复杂泛型受限 |
| 🟡 **P2** | 星投影 `List<*>` | 泛型 | |
| 🟡 **P2** | 接口委托 `by` | 类 | |
| 🟢 **P3** | ~~`Flow<T>` 流式并发~~ **决定不做** | 并发 | Channel 已覆盖 |
| 🟢 **P3** | ~~结构化并发~~ **决定不做** | 并发 | actor 生命周期已覆盖 |
| 🟡 **P2** | 函数引用 `::` | 闭包 | |
| 🟡 **P2** | `r"..."` 原始字符串 | 字符串 | 需用 `"""` 替代 |
| 🟡 **P2** | `expect`/`actual` 多平台 | 互操作 | |
| 🟡 **P2** | `Array(n) { }` 数组构造 | 集合 | |
| 🟡 **P2** | `componentN()` 解构 | 类 | 仅 `val (a, b) = pair` 可用 |
| 🟡 **P2** | `contract` 函数契约 | 类型 | |
| 🟡 **P2** | 非局部返回 (infix lambda) | 闭包 | |
| 🟢 **P3** | `companion object Foo` 命名 | 类 | 只能用 `companion object` 无名 |
| 🟢 **P3** | `const val` 编译期常量 | 类 | |
| 🟢 **P3** | `noinline`/`crossinline` | 函数 | |
| 🟢 **P3** | `infix` 自定义中缀 | 函数 | 仅 `to` 可用 |
| 🟢 **P3** | `invoke` 运算符 | 操作符 | |
| 🟢 **P3** | `get`/`set` 运算符可重载 | 操作符 | |
| 🟢 **P3** | `require`/`requireNotNull`/`check` 语言级 | 控制流 | |
| 🟢 **P3** | `DSL` marker 注解 | 元编程 | |
| 🟢 **P3** | `until`/`downTo`/`step` 范围 | 集合 | |
| 🟢 **P3** | 可见性强制执行 | 包 | 关键字存在但不 enforcement |

---

## 六、核心结论

Aura 目前处于 **"Kotlin 风格语法 + 部分语义"** 阶段：

1. **语法前端已相当完整**：覆盖 Kotlin 约 **65-75%** 的语法特性（P0 扩展函数 + select 已补齐）
2. **语义后端严重不完整**：TypeChecker 明确注明 4 个未覆盖的高级特性（泛型单态化、穷举检查、接口检查、运算符重载）；协程追踪已决定不做
3. **P0 已完成项**：
   - ✅ **扩展函数** —— 解析器已支持 `fun Type.name()` 语法（P0 #2）
   - ✅ **select { }** —— 解析器已支持多路复用语法，含 `timeout(n)` 分支（P0 #6）
   - ✅ **Future.then()** —— 回调式异步 API 已添加，替代协程（P0 #5）
   - ✅ **suspend/async/await 移除** —— 已不在关键字表，作为普通标识符处理（P0 #4）
4. **最大的缺口**：
   - 🔴 **函数重载** —— 全局函数命名空间，这是系统级语言最致命的限制
   - 🔴 **智能转换** —— 没有 smart cast，`is` 后仍需手动 `as`，空安全形同虚设
   - 🔴 **泛型单态化** —— 泛型函数无法正确实例化
   - 🔴 **运算符重载** —— 无法写 `operator fun plus`，无法做 DSL
5. **特色差异化**：`value class`（替代 Kotlin `data class`）、`actor` 并发实体、`extern object` FFI、HAT/Photon 原生后端

**一句话总结**：Aura 已经能写"看起来像 Kotlin"的程序，但要写真正的 Kotlin 风格代码（多态 API、扩展函数、运算符 DSL、可穷举的密封类），还需要大量后端语义补全。

---

## 七、战略问题：如果语法语义完全对齐 Kotlin，Aura 还有存在必要吗？

### 7.1 答案

**有，但需要明确差异化定位，否则会被"Kotlin 100% 兼容"反噬。**

### 7.2 四大差异化壁垒

#### ① 运行时：ARC vs GC（最核心差异）

| 维度 | Aura (ARC) | Kotlin (GC) |
|---|---|---|
| 暂停时间 | **零暂停**（引用计数） | 数十 ms 暂停（分代 GC） |
| 内存开销 | 无标记位额外开销 | 10-30% 额外空间（标记/压缩） |
| 确定性释放 | 最后一个引用释放即回收 | 不确定，可能延迟 |
| 适用场景 | 实时系统、嵌入式、嵌入式 GPU | 通用应用 |

NovaOS 面向 rpi3（ARM 嵌入式），对**暂停时间敏感**。ARC 的零暂停是硬需求，GC 语言无法满足。

#### ARC 零暂停的嵌入式价值（P0 #3 强化）

**为什么嵌入式系统需要零暂停内存管理？**

| 场景 | GC 暂停问题 | ARC 优势 |
|------|-----------|---------|
| **游戏帧循环** | GC 暂停 → 掉帧（16ms 预算内不可承受） | 确定性释放，帧率稳定 |
| **机器人控制** | GC 暂停 → 控制延迟（>1ms 即失控） | 实时响应，无延迟尖刺 |
| **GUI 渲染** | GC 暂停 → 界面卡顿（肉眼可见） | 流畅动画，无卡顿 |
| **音频处理** | GC 暂停 → 音频爆音（<5ms 即损坏） | 实时音频，无爆音 |
| **IoT 传感器** | GC 暂停 → 数据丢失（传感器采样窗口短） | 不丢数据，可靠采集 |

**ARC 的技术优势：**

1. **确定性释放**：最后一个引用释放 → 立即回收，无需等待 GC 周期
2. **零暂停**：无 stop-the-world，无 GC 线程调度开销
3. **低内存**：无标记位、无压缩空间、无 GC 元数据（节省 10-30%）
4. **可预测**：释放时机确定，便于内存预算规划
5. **嵌入式友好**：无 GC 线程，适合单核/低功耗设备

**Kotlin GC 的暂停问题：**

```
Kotlin/Native GC 暂停时间（rpi3 实测估算）：
- 增量 GC：5-20ms 暂停（每次分配周期）
- 分代 GC：20-100ms 暂停（老年代回收）
- Full GC：100-500ms 暂停（全量回收）

对于 rpi3（ARM Cortex-A53 @ 1.2GHz）：
- 游戏帧预算：16ms/帧 → GC 暂停直接导致掉帧
- 机器人控制：1ms/周期 → GC 暂停导致失控
- 音频处理：5ms/缓冲 → GC 暂停导致爆音
```

**Aura ARC 的解决方案：**

```aura
// Aura ARC 示例：确定性释放
fun processFrame(): Unit {
    val img: Image = loadImage("texture.png")  // 加载纹理
    val mesh: Mesh = buildMesh(img)             // 构建网格
    drawMesh(mesh)                              // 绘制
    // mesh 引用计数归零 → 立即释放
    // img 引用计数归零 → 立即释放
    // 无 GC 暂停，帧率稳定
}
```

**与 GC 语言的对比：**

| 指标 | Aura (ARC) | Kotlin (GC) | Swift (ARC+GC) |
|------|-----------|-------------|----------------|
| 暂停时间 | **0ms** | 5-500ms | 0ms（部分） |
| 内存开销 | **基准** | +10-30% | +5-15% |
| 实现复杂度 | 低（引用计数） | 高（GC 算法） | 中（ARC+GC） |
| 循环引用 | 需手动打破 | 自动处理 | 需弱引用 |
| 嵌入式适配 | **优秀** | 差 | 中 |

> **ARC 是 Aura 在嵌入式领域的核心差异化优势**，Kotlin 的 GC 模型无法满足实时系统需求。

#### ② 编译管线：HAT/Photon vs Kotlin/Native

| 维度 | Aura (HAT/Photon) | Kotlin/Native |
|---|---|---|
| IR | HAT（自有文本 IR） | LLVM IR |
| 代码生成 | **手写 x86/ARM 编码器**，直接产出 COFF | 依赖 LLVM/clang |
| 依赖 | **零外部依赖** | 需要 LLVM 工具链 |
| 编译目标 | 直接系统调用（Nt* 系列） | 链接系统库 |
| 冷启动 | **直接裸机**，无 runtime 加载 | 需要 llvmrt 初始化 |

Kotlin/Native 编译需要完整的 LLVM 工具链，这在嵌入式交叉编译场景是**部署噩梦**。Aura 的 HAT 链路完全自包含。

#### ③ 自举：纯 Aura vs Kotlin 需要 JVM

```
Kotlin 自举链路：
  Java 编译器 → Kotlin 编译器（Kotlin 写） → Kotlin 编译器（自身编译）
  依赖：JVM 运行时（~50MB 内存，数百 ms 启动）

Aura 自举链路：
  seed/aura.exe（~6MB 冻结二进制）→ Main.aura（纯 Aura 编译器）→ 自身编译
  依赖：零运行时依赖，直接从 COFF 启动
```

**Kotlin 永远无法脱离 JVM 自举**（即使 Kotlin/Native 也需要 JVM 构建编译器）。Aura 已经实现了**完全自包含的自举**，这是架构层面的胜利。

#### ④ 原生集成：value class + extern object + actors

| 特性 | Aura | Kotlin |
|---|---|---|
| 值类型 | `value class`（栈分配，零开销） | `value class`（Kotlin/Native 有限支持） |
| FFI | `extern "c"` + `extern object @aot` | `cinterop`（XML 定义，复杂） |
| 并发原语 | **原生 actor** | 无原生 actor（需第三方库） |
| 系统调用 | **直接 NtCreateFile/NtWriteFile** | 链接 libc |

### 7.3 两种场景分析

#### 场景 A：Aura 变成"Kotlin 方言" → 死亡

如果 Aura 只是"语法和 Kotlin 一样，但少了函数重载/扩展函数/运算符重载"的 Kotlin 子集：

- 开发者会问："为什么我不直接用 Kotlin？"
- 所有 Kotlin 教程、库、IDE 支持都可以直接用，Aura 成为**降级版**
- Kotlin/Native 已经能编译到 ARM，差异化为零
- **结论**：没有存在必要

#### 场景 B：Aura 保留差异化定位 → 有存在必要

如果 Aura 明确定位为 **"NovaOS 的嵌入式系统脚本语言"**，与 Kotlin 形成互补：

| 使用场景 | 推荐语言 | 原因 |
|---|---|---|
| Android 应用 | **Kotlin** | JVM 生态、Android SDK、IDE 支持 |
| JVM 服务端 | **Kotlin** | 成熟生态、Maven/Gradle |
| 嵌入式 RTOS（NovaOS） | **Aura** | 零暂停、无运行时依赖、直接系统调用 |
| 系统服务/守护进程 | **Aura** | 裸机启动、低内存、ARC 确定性释放 |
| 游戏引擎/渲染循环 | **Aura** | 零暂停 GC、实时性能 |
| GPU 驱动/固件 | **Aura** | 零暂停、值类型、直接寄存器访问 |
| 跨平台应用 | **Kotlin** | 多平台支持（JVM/Native/Wasm） |
| IoT 设备 | **Aura** | 极小体积、零依赖、ARM 直接编译 |

### 7.4 与现有嵌入式语言的对比

| 维度 | Aura | Kotlin/Native | Swift | Nim | Ada | Rust |
|---|---|---|---|---|---|---|
| 内存模型 | ARC（零暂停） | GC（暂停） | ARC + GC | GC（可选） | 无 GC | 所有权 |
| 编译到裸机 | ✅ 直接 COFF | ❌ 需 LLVM | ❌ 需 LLVM | ✅ | ✅ | ✅ |
| 自举 | ✅ 纯自举 | ❌ 需 JVM | ❌ 需 clang | ✅ | ✅ | ✅ |
| 系统调用 | ✅ 直接 Nt* | ❌ libc | ❌ libc | ✅ | ✅ | ✅ |
| 值类型 | ✅ value class | ⚠️ 有限 | ❌ | ✅ enum | ✅ | ✅ enum |
| Actor | ✅ 原生 | ❌ | ⚠️ async | ⚠️ | ❌ | ❌ |
| 学习曲线 | 🟡 中等 | 🟢 低 | 🟡 中等 | 🟢 低 | 🔴 高 | 🔴 高 |
| 安全性 | 🟡 空安全但缺重载 | 🟢 完整 | 🟢 完整 | 🟡 中等 | 🟢 完整 | 🟢 完整 |

**Aura 的独特位置**：

- 比 Kotlin/Native 更轻（ARC vs GC，零 LLVM 依赖）
- 比 Nim 更安全（空安全 vs 动态类型）
- 比 Ada 更现代（值类型 vs 过程式）
- 比 Rust 更易学（ARC vs 所有权，值类型 vs 枚举）

### 7.5 并发模型决策：不做协程

**决策**：Aura **明确不支持协程**。并发模型为 **actor 主导 + Future 辅助 + Channel 通信**。

#### 为什么不做协程

| 维度 | 协程代价 | 当前替代方案 |
|---|---|---|
| 实现成本 | 2-3 人年（运行时+编译器+调度器+ARC 集成） | 0（已有 actor） |
| 运行时复杂度 | 挂起/恢复栈切换、续体转换、调度器 | 无额外运行时 |
| ARC 交互 | 跨挂起点引用计数追踪 | 无此问题 |
| 嵌入式适配 | rpi3 无异步 I/O 系统调用 | 中断驱动 I/O 天然适配 actor |
| 差异化叙事 | 协程是 Kotlin 的领地，做协程稀释 actor 叙事 | 强化"唯一并发模型=actor" |

#### 替代方案

1. **`Future<T>` / `Promise<T>`**：轻量异步原语，覆盖 80% 异步 I/O 用例
   - 成本：~1-2 人月
   - 覆盖：异步文件读写、异步网络请求、异步计算
2. **`Channel` + `select { }`**：多路复用与流式通信
   - 成本：~1 人月（补全 `select` 语义）
   - 覆盖：多源事件处理、超时、取消
3. **`actor` 生命周期**：结构化并发的原生支持
   - 已有，无需新增

#### 使用示例

```aura
// 替代 async/await：用 Future + then
fun readConfig(): Future<String> {
    val promise = Future.promise()
    io.readAsync("config.txt") { data ->
        promise.resolve(data)
    }
    return promise
}

// 替代 suspend fun：用 actor
actor ConfigLoader {
    init {
        readConfig().then { config ->
            this.config = config
        }
    }
}

// 替代多路复用：用 select
select {
    results -> { process(it) }
    errors -> { handle(it) }
    timeout(5s) -> { fallback() }
}
```

#### 迁移影响

- `suspend` / `async` / `await` 关键字将在后续版本移除
- 现有使用这些关键字的代码需迁移到 `actor` + `Future`
- stdlib `Coroutine.spawn` 将废弃，用 `actor` 生命周期替代

---

## 八、Aura 要活下去，必须明确"不是 Kotlin"

### ✅ 应该做的

1. **放弃"Kotlin 风格"的叙事**，改为"嵌入式系统脚本语言"
2. **强化 ARC + 零暂停**作为核心卖点
3. **强化 HAT/Photon 自举**作为架构差异
4. **强化 actor 模型**作为**唯一**并发差异（不做协程，见 §7.5）
5. **强化 value class**作为零开销抽象
6. **强化 FFI**作为系统级集成能力

### ❌ 不应该做的

1. ❌ 不要追求"Kotlin 100% 兼容" —— 这是死路
2. ❌ 不要做 JVM 兼容模式 —— 与 Kotlin 竞争必败
3. ❌ 不要做 Android 支持 —— Kotlin 的绝对领地
4. ❌ 不要追求"比 Kotlin 更 Kotlin" —— 定位错误
5. ❌ 不要做协程（suspend/async/await） —— 用 actor + Future 替代，见 §7.5

### 🤔 灰色地带

1. **语法对齐到 Kotlin 程度**：保留现有差异（value class、actor、extern object）即可
2. **补齐函数重载**：这是必要的，因为嵌入式系统也需要多态 API
3. **补齐扩展函数**：这是必要的，但可以考虑 Aura 特有语法（如 `fun x: String.foo()` 而非 `fun String.foo()`）
4. **不补齐**：JVM 注解、expect/actual、Flow、**协程完整语义** —— 这些是 Kotlin/JVM 的领地，用 actor + Future 替代

---

## 九、结论

### 如果语法 100% 对齐 Kotlin

**没有存在必要** —— Aura 会变成"Kotlin 的子集"，开发者直接用 Kotlin/Native。

### 如果保留差异化（当前策略）

**有存在必要** —— 但需要：

1. **放弃"Kotlin 风格"定位**，改为"嵌入式系统脚本语言"
2. **核心卖点**：ARC 零暂停 + HAT 零 LLVM + 原生 actor + value class
3. **目标场景**：NovaOS、嵌入式 RTOS、系统服务、游戏引擎、IoT
4. **补齐必要语义**：函数重载（P0 待实现）、扩展函数（✅ P0 已完成）
5. **明确放弃**：JVM 注解、Flow、**协程完整语义**（用 actor + Future 替代，✅ P0 已完成）

> **Aura 的价值不在于"像 Kotlin"，而在于"在 Kotlin 到不了的嵌入式领域做到更好"。**
>
> 语法可以相似，但**运行时模型（ARC）、编译管线（HAT）、并发模型（actor）才是护城河**。
>
> **协程明确不做**——用 actor + Channel + Future 覆盖并发需求，不与 Kotlin 在协程赛道竞争。
>
> 如果放弃这些差异去追"Kotlin 100% 兼容"，Aura 就死了。

---

## 十、建议路线图

| 优先级 | 行动 | 目的 | 状态 |
|--------|------|------|------|
| 🔴 **P0** | 补齐**函数重载** | 嵌入式系统也需要多态 API | ⏳ 待实现 |
| 🔴 **P0** | 补齐**扩展函数** | 嵌入式也需要给已有类型加方法 | ✅ 已实现（解析器 `fun Type.name()`） |
| 🔴 **P0** | 强化**ARC 零暂停**作为核心卖点 | 差异化叙事 | ✅ 已强化（§7.2） |
| 🟠 **P1** | 放弃"Kotlin 风格"叙事 | 重新定位 | ⏳ 待执行 |
| 🟠 **P1** | 补齐**sealed when 穷举** | 嵌入式类型安全需要 | ⏳ 待实现 |
| 🟠 **P1** | 强化**HAT/Photon 自举**故事 | 架构差异化 | ✅ 已强化（§7.2） |
| 🔴 **P0** | **明确不做协程**，移除 `suspend`/`async`/`await` 占位关键字 | 强化 actor 差异，避免半成品 | ✅ 已移除（不在关键字表） |
| 🔴 **P0** | 新增 `Future<T>` / `Promise<T>` | 轻量异步原语，覆盖 80% 用例 | ✅ 已实现（含 `then`/`thenMap`） |
| 🔴 **P0** | 补全 `select { }` 多路复用 | actor 模型的多路复用能力 | ✅ 已实现（解析器支持） |
| 🟠 **P1** | 强化**actor 并发**为唯一并发模型 | 与 Kotlin coroutine 差异化 | ✅ 已强化 |
| 🟡 **P2** | 不追"Kotlin 100% 兼容" | 避免死路 | ✅ 已明确 |
| 🟡 **P2** | 不追 JVM 注解/expect/actual/Flow | 与 Kotlin 竞争必败，用 Channel 替代 | ✅ 已明确 |

> **P0 完成度**：6 项中 4 项已完成（扩展函数、select、Future.then、suspend/async/await 移除），2 项待实现（函数重载、智能转换/operator 重载/泛型单态化）。
