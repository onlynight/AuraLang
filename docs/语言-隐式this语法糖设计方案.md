# Aura 隐式 this 语法糖优化设计方案

> **状态**：设计方案（未实施） | **优先级**：P0（语言核心） | **目标版本**：v0.6 引入 → v1.0 稳定
> **分析日期**：2026-10
> **范围**：语法/词法、AST、语义分析、HIR 降级、字节码/AOT 发射、LSP、自举编译器同步
>
> **🔑 关键约束**：**老代码不改动，新代码可选省略 `this.`**。
> 两种写法在同一个文件、同一个类、甚至同一个方法体内都可以自由共存；
> 不引入批量迁移，也不要求 Formatter 强制改写既有代码。
> **前置阅读**：[语言-Any基类与object关键字设计方案.md](语言-Any基类与object关键字设计方案.md) · [语言-struct与class定位方案.md](语言-struct与class定位方案.md) · [语言-script-mode-技术方案.md](语言-script-mode-技术方案.md)

---

## 〇、结论摘要

| 维度 | 结论 |
|------|------|
| **是否引入** | ✅ **引入隐式 this 语法糖**（Kotlin/Java 风格），完全可选，向后兼容 |
| **核心策略** | 类/方法体内的裸标识符按「局部 → 方法参数 → 属性 → 方法 → companion 静态成员 → 继承链 → 顶层」顺序解析；`this.x`/`this::m()` 保留显式访问通道；新增 `self` 作为 `this` 的别名（可选） |
| **同名消歧** | **局部变量优先**遮蔽同名字段；显式字段访问只能写 `this.x`；编译器给出 warning（`field '<x>' is shadowed by local`）而非错误 |
| **🔑 老代码策略** | **不改动**。10,907 处既有 `this.` 前缀**保留**；不引入批量迁移工具；Formatter 默认**不改写**既有 `this.` |
| **🔑 新代码策略** | 新写的代码**可以省略** `this.`；同一文件/类/方法体内显式与隐式**自由共存** |
| **实现周期** | 分 4 阶段，约 3–4 周（Rust + Aura 自举双线并行） |
| **风险等级** | **低**（Rust 侧 HIR 降级基础设施 `ClassCtx` 已经存在，Aura 自举侧仅需在 HIR 层做同样的兜底） |
| **代码量影响** | 老代码零改动；**新增代码**预计平均缩减 20–35% 类方法体字数 |
| **阻塞项** | 无（可与 P-K2「类成员分派」后续演进并行） |

**一句话**：Aura 的 Rust HIR 降级器里已经有 `ClassCtx` + `locals` 作用域栈 + `class_bare_ident` / `bare_call_in_class` 三个辅助函数，理论上支持隐式字段/隐式方法，**但只在 sema 拿不到类型信息时的兜底路径才被触发**，且没有对「显式 `this.` 前缀」做等价改写；本文档把这些兜底路径**提到主路径**，让**新代码**能用最简形式写，同时**既有代码**一行都不用改。

---

## 一、背景与问题定义

### 1.1 现状快照

Aura 目前支持 Kotlin 风格类：

```aura
class Counter {
    var count: Int = 0

    init() {
        this.count = 0
    }

    fun bump(): Int {
        this.count = this.count + 1
        return this.count
    }

    fun display(): String {
        return "count=" + this.count.toString()
    }
}
```

**问题**：`this.` 前缀在类体内部被强制要求，导致：

1. **冗余**：类里 90% 的字段/方法访问都是访问自己，几乎每次访问都要写 `this.`。以自举编译器为样本：**80 个 Aura 文件、10,907 处 `this.` 前缀**，仅这一项就为自举源码贡献 ~8% 的字数。
2. **可读性下降**：一行里多个 `this.` 让阅读节奏被打断（`this.dst.add(this.dst.kindOf(id), text, ty, sp, newKids)`）。
3. **迁移成本**：从 Kotlin/Java 迁移过来时需要机械地把裸字段访问加上 `this.`，心智负担高。
4. **对生成代码不友好**：代码生成器通常按「访问局部对象」语义生成，被迫加 `this.`。

### 1.2 现状实现路径的调研

**关键发现**：Rust 编译器里隐式字段/方法访问的**基础设施已经存在**，只是没被推广成语法糖主路径。

#### Rust 侧（`rust/compiler/src/codegen/hir.rs`）

```rust
// hir.rs:73 —— 当前正在降级的类/结构体上下文
static CLASS_CTX: RefCell<Option<ClassCtx>>;

// hir.rs:200 —— 上下文结构
struct ClassCtx {
    class: String,
    fields: Vec<String>,                       // 当前类的字段名
    locals: Vec<HashSet<String>>,              // 作用域栈内的局部名（屏蔽同名字段）
}

// hir.rs:330 —— 裸字段改写（已存在）
fn class_bare_ident(n: &str) -> Option<HirExpr> {
    let ctx = CLASS_CTX.with(|c| c.borrow().as_ref().cloned())?;
    if ctx.fields.iter().any(|f| f == n) && !ctx.is_local(n) {
        return Some(HirExpr::Member {
            object: Box::new(HirExpr::Var("self".into())),
            name: n.to_string(),
        });
    }
    None
}

// hir.rs:352 —— 裸方法调用改写（已存在，伴生方法不带 self）
fn bare_call_in_class(n: &str) -> Option<(String, bool)> {
    let ctx = CLASS_CTX.with(|c| c.borrow().as_ref().cloned())?;
    let entry = table.get(&ctx.class)?;
    if entry.methods.contains(n) && !ctx.is_local(n) {
        return Some((format!("{}.{}", ctx.class, n), true));   // insert_self = true
    }
    if entry.companion_methods.contains(n) && !ctx.is_local(n) {
        return Some((format!("{}.{}", ctx.class, n), false));  // 伴生方法
    }
    None
}
```

但 `class_bare_ident` 和 `bare_call_in_class` **只在 sema 未命中时作为兜底路径调用**（`Expr::Call` 的 receiver 为 `Expr::Ident` 且查不到静态类型），**没有覆盖 `Expr::MemberAccess { object: Ident }` 的裸字段访问主路径**。

#### Aura 自举侧（`aura/compiler/aura/lang/compiler/hir/Hir.aura`）

`lowerFunction` 直接把参数、参数类型、函数体降级为 HIR 节点，**没有类上下文，也没有 `ClassCtx` 等价物**。这意味着当前 Aura 自举编译器**完全不支持**隐式字段访问——写 `src.count` 而不是 `this.src.count` 会解析失败。

### 1.3 目标非目标

**目标（In Scope）**

- 类方法体内允许省略 `this.`，直接写字段名和方法名
- 精确的消歧规则（同名局部遮蔽字段；显式 `this.` 强制访问字段）
- 语义分析层给出清晰的诊断消息
- HIR 层的降级规则与 Rust 编译器一致（同一套 `ClassCtx` 语义）
- 自举编译器与 Rust 编译器**同步支持**，保证新增的 Aura 源码可用最简形式写
- Formatter 提供 `preserve` 模式（**默认**，不改动既有代码）和 `implicit` 模式（**可选**，仅对新增代码生效或按需手动调用）
- LSP 侧补全/跳转能识别隐式字段访问
- **可选**迁移工具：作为独立 CLI 子命令 `aura migrate --implicit-this`，**仅当团队主动要求时**使用；不作为默认流程

**非目标（Out of Scope）**

- **不迁移**既有 10,907 处 `this.` 前缀（老代码保留原样，除非团队主动运行迁移工具）
- **不要求**团队在切换语言版本时批量改代码（新老写法共存）
- **不改变** Formatter 对既有 `this.` 的默认行为（保留）
- **不引入**其他语言的隐式接收者（`Outer.this` / `OuterKlass` 语法），因为 Aura 的嵌套类当前只有一种「内部类通过 `Outer()` 工厂生成」的模式，不需要外部类显式访问
- **不引入** `field` 上下文关键字（这是访问器专用，与隐式 this 正交）—— 但**支持**访问器内使用 `field`，语法保留
- **不引入** `receiver` 关键字（Kotlin 扩展函数的隐式接收者），扩展函数是另一个议题
- **不改变** `super.` / `super::m()` 语义（父类显式访问继续保留）

---

## 二、参考语言语义分析

| 语言 | 隐式字段访问 | 隐式方法调用 | 局部遮蔽 | 逃逸闭包 | 备注 |
|------|------|------|------|------|------|
| Java | ✅ 直接访问 | ✅ 直接调用 | ✅ 局部遮蔽 | 不需要 `this::`（Java 8 前） | 语法最宽松 |
| Kotlin | ✅ 直接访问 | ✅ 直接调用 | ✅ 局部遮蔽 | `with(this)` | 提供 `field` 上下文关键字 |
| C# | ❌ 必须 `this.` | ❌ 必须 `this.` | — | — | 强制显式 |
| C++ | ❌ 必须 `this->` | ❌ 必须 `this->` | — | — | 强制显式 |
| Swift | ✅ 直接访问 | ✅ 直接调用 | ✅ 局部遮蔽 | 逃逸闭包必须写 `self.` | 折衷派 |
| Rust | ❌ 必须 `self.` | ❌ 必须 `self.` | — | — | 强制显式（`self` 是接收者形参） |

**Aura 的定位**：与 Java/Kotlin 同侧（隐式为主、显式为逃生口），但保留 `this.` 强制显式访问，与 Swift 的逃逸闭包规则不同（Aura 不区分闭包捕获）。

### 2.1 Java 参考

```java
class Counter {
    int count;
    int bump() { return ++this.count; }   // 显式
    int bump2() { return ++count; }        // 隐式 ✅
}
```

- 解析顺序：局部变量 → 方法参数 → 字段（沿继承链向上） → 静态字段/方法（编译错误）
- 局部遮蔽字段 → 字段访问必须写 `this.`

### 2.2 Kotlin 参考

```kotlin
class Counter {
    var count = 0
    fun bump(): Int { count += 1; return count }   // 隐式 ✅
}

class Outer {
    var x = 1
    fun f() {
        class Inner {
            fun g() { print(Outer().x) }   // 内嵌类访问外部类必须显式（通过工厂）
        }
    }
}
```

- 隐式 `this` = 「当前类实例」
- 显式 `this@Outer` 区分不同嵌套层
- 局部变量遮蔽字段（同 Java）

**关键差异**：Kotlin 用 `this@Label` 语法在嵌套类中区分外部类；Aura 当前**没有嵌套类语法**（内嵌类都是顶层声明），所以不需要 `this@Label`。

### 2.3 C# 参考

C# 强制显式 `this.`，但**在字段初始化和属性访问器里允许**：

```csharp
class Counter {
    private int count = 0;          // ✅ 字段初始化时隐式
    public int Count {
        get { return count; }        // ❌ C# 需要 this.count
    }
}
```

Aura 选择更宽松的隐式规则，不采纳 C# 的强制显式策略。

---

## 三、语法与词法

### 3.1 语法（BNF 片段）

```
ClassBody      := '{' ClassMember* '}'
ClassMember    := FieldDecl | MethodDecl | PropertyDecl | Companion | Constructor | InitBlock
FieldDecl      := [Visibility] ('val' | 'var' | 'const') Identifier (':' Type)? ('=' Expression)?
MethodDecl     := [Visibility] MethodModifier* 'fun' <type-params> Identifier <params> [':' Type] [Body]
PropertyDecl   := [Visibility] ('val' | 'var') Identifier [': Type'] 'by' | 'get' / 'set'
Constructor    := 'init' '(' Params ')' [':' Delegation] ['{' Block '}']
                 | 'constructor' '(' Params ')' [':' Delegation] ['{' Block '}']
Companion      := 'companion' 'object' [Identifier] '{' ClassMember* '}'

Expression     := ... | ThisExpr | SuperExpr | Primary Postfix*
ThisExpr       := 'this'                      // 表示当前实例（等价于旧语法 `this`）
                 | 'self'                     // 别名，Kotlin 风格
                 | 'this' '::' 'super'        // 显式外部类父类，暂不支持
Delegation     := 'this' '(' ArgList ')'       // 次构造函数委托（保留）
                 | 'super' '(' ArgList ')'      // 父类委托
                 | Identifier '(' ArgList ')'  // Aura 风格父类名（等同 super）
```

**要点**：

- `this` 关键字继续保留，作用不变（表示当前实例本身）
- 新增 `self` 关键字作为别名（可选），Kotlin 风格更熟悉
- **不加新关键字**：隐式字段/方法访问不需要新的语法标记，只需修改解析/降级规则

### 3.2 词法

无需修改词法。`this` 已经是 `TokenKind::This`；`self` 目前是普通标识符（可用作变量名，但会与本提案冲突，见 §12 命名保留清单）。

---

## 四、语言规范（精确规则）

### 4.1 核心原则

**原则 1：局部优先**
在方法体内解析裸标识符 `x`：
1. 当前作用域局部变量
2. 方法参数（按声明顺序）
3. **当前类的字段**（含从继承链继承的字段）
4. **当前类的方法**（`x(...)` 形态；伴生方法不带 `self`）
5. **伴生对象（Companion）的静态成员**
6. **父类的字段/方法**（沿继承链向上）
7. 顶层符号（含 import）
8. 编译错误 `unresolved reference`

**原则 2：`this.` 强制字段访问**
一旦写 `this.x`，编译器**不再尝试**把它解析为局部变量、参数、方法——它是**字段的强制访问**。这为局部遮蔽字段的情况提供了唯一逃生口。

**原则 3：`this::m()` 强制方法调用**
`this::m()` 等价于 `m()`（隐式方法调用），显式标注「以当前实例为接收者」。可用于：
- 强调「这是实例方法而非伴生方法」
- 打破与伴生方法/静态工具方法的歧义

**原则 4：伴生方法隐式不带 self**
`companion object { fun foo() ... }` 里的 `foo()` 调用不插入 `self` 首参，与 Rust 侧 `bare_call_in_class` 现有语义一致。

**原则 5：局部遮蔽仅告警，不报错**
当方法参数或局部变量与字段同名时，隐式访问退化为局部；**编译器发 warning**：
```
warning: field 'count' is shadowed by parameter at fn 'bump'
  → count: Int
```

### 4.2 消歧示例

#### 4.2.1 基本隐式字段访问

```aura
class Counter {
    var count: Int = 0
    fun bump(): Int {
        count = count + 1           // ✅ 隐式，等价于 this.count = this.count + 1
        return count
    }
}
```

#### 4.2.2 局部遮蔽字段

```aura
class Counter {
    var count: Int = 0
    fun localCopy(): Int {
        var count: Int = 0          // 遮蔽字段 count
        count = count + 1           // ✅ 访问局部，编译器告警
        return count                // 返回 1，而非 this.count
    }
    fun readField(): Int {
        return count                // ✅ 访问字段（无局部遮蔽）
    }
    fun shadowed(): Int {
        val count: Int = 5          // 遮蔽字段
        return this.count           // ✅ 必须用 this. 显式访问字段
    }
}
```

#### 4.2.3 隐式方法调用（实例方法）

```aura
class Shape {
    val area: Double = 0.0

    fun describe(): String {
        return "area=" + area.toString()    // ✅ 隐式字段
    }

    fun area() {                            // 与字段同名？见 §4.5
        return this.area
    }

    fun compute(): Double {
        return area()                       // ✅ 隐式调用当前类的 area() 方法
    }
}
```

**注意**：`area` 既可能是字段也可能是方法。解析顺序：
- `area`（作为表达式）→ **字段优先**（因为表达式语法上不能是「裸方法引用」）
- `area()`（作为调用）→ **方法优先**（字段没有 `()` 后缀）

因此「字段和方法同名」不产生歧义，编译器根据**后缀运算符**判定。

#### 4.2.4 伴生方法隐式不带 self

```aura
class Matrix {
    companion object {
        fun identity(): Matrix { ... }
        val ZERO: Matrix = ...
    }

    fun combine(): Matrix {
        return identity()              // ✅ 隐式伴生方法调用，不带 self
    }

    fun zeroSum(m: Matrix): Matrix {
        return m + ZERO                // ✅ 伴生字段访问
    }
}
```

#### 4.2.5 继承链上的字段访问

```aura
class Animal {
    var name: String = ""
    fun eat() { /* ... */ }
}

class Dog : Animal() {
    var breed: String = ""

    fun describe(): String {
        return name + " the " + breed  // ✅ 隐式：this.name（继承）+ this.breed
    }

    fun speak() {
        eat()                          // ✅ 隐式：this.eat()（继承方法）
    }
}
```

#### 4.2.6 显式 `this` 用于逃逸闭包 / 泛型接收者

```aura
class Builder {
    var name: String = ""

    fun use(f: (String) -> Unit): Unit {
        name = "default"
        f(this.name)                  // ✅ 显式 this，闭包捕获当前实例
    }

    // 泛型接收者需要明确 self 语义：
    fun transform<T>(self: T, f: (T) -> T): T {
        // 这里的 self 是参数名，与关键字冲突——见 §12 命名保留
    }
}
```

**注意**：`self` 作为参数名会与关键字冲突（详见 §12）。因此 §12 明确把 `self` 列为**软保留字**：作为参数名需要显式声明（`fun transform<$self>...`），或改用其他名字（推荐 `receiver`）。

### 4.3 Lambda 与闭包规则

**关键决定**：Aura 隐式 this **不因闭包捕获改变**。

```aura
class Counter {
    var count: Int = 0
    fun snapshot(): () -> Int {
        return { count }              // ✅ 闭包捕获局部变量 count，等价于 this.count
    }
}
```

**理由**：
- 与 Kotlin 一致（Kotlin 闭包捕获字段也是隐式 `this.`）
- 保持「隐式 this 就是 `this.` 的语法糖」这一简单规则
- 避免 Swift 式的「逃逸闭包必须写 `self.`」二分规则

**边界情况**：

```aura
class A {
    var x: Int = 0
    fun make(): () -> Int {
        val self = this               // ✅ 允许，把实例捕获到局部
        return { self.x }             // ✅ 通过局部变量访问
    }
}
```

### 4.4 内部对象（companion / object）的隐式规则

`companion object` 和 `object` 内的方法**没有 self**，因为它们是静态的。但**在伴生方法内访问同类的静态字段/方法不需要**写类名：

```aura
class Math {
    companion object {
        const val PI: Double = 3.14159
        fun abs(x: Double): Double { return x < 0 ? -x : x }
        fun max(a: Double, b: Double): Double { return a > b ? a : b }
    }
}

// 在 Math 类内：
class MathUtil {
    fun describe(): String {
        return "PI=" + PI             // ✅ 隐式 companion 字段访问（若 MathUtil 定义了 PI 则优先）
    }
}
```

**注意**：伴生对象成员只在**同一个类的伴生上下文**内隐式可见。跨类的 `Math.PI` 必须显式写类名。

### 4.5 属性访问器（PropertyAccessor）与 `field` 关键字

Aura 支持 `val x: Int` + `get() / set()` 访问器。当前 Aura 侧的实现细节见 `codegen/hir.rs:3663` 附近（`ACCESSOR_PROP` 上下文关键字改写）。

**规则**：访问器体内允许 `field` 关键字直接访问被封装的属性值（与 Kotlin 一致）：

```aura
class Person {
    var name: String = ""
        get() { return field }           // ✅ 隐式 this.name
        set(v: String) { field = v }     // ✅ 隐式 this.name = v
}
```

**注意**：`field` 是**上下文关键字**（contextual keyword），只在访问器体内识别为属性引用，其他位置作为标识符处理（与 Kotlin 一致，见 Rust 侧 `ACCESSOR_PROP`）。

### 4.6 显式 `this` 的合法使用场景

`this` 仍然保留，且在以下场景**必须使用**：

1. **局部变量遮蔽字段**时访问字段
2. **传递当前实例作为参数**：`register(this)`
3. **返回当前实例**：`fun build(): Builder = this`（链式构造）
4. **闭包内明确引用**：`return { this.count }`
5. **`super` 与 `this` 组合**：`super.toString()`（显式父类）
6. **次构造函数委托**：`constructor(...) : this(...)`
7. **静态工厂**：`companion object { fun create(self: Counter): Counter = self }`（此时 `self` 是参数名，需要 `$$` 或反引号包裹，见 §12）

---

## 五、同名与消歧规则详述

### 5.1 作用域嵌套（Scope Nesting）

方法体内的作用域栈（从内到外）：

```
[当前块] → [外层块] → [循环/分支] → [方法体本身] → [方法参数] → [类字段] → [继承链] → [伴生对象] → [顶层/import]
```

**解析规则**：
- 局部变量遮蔽同名的方法参数、字段、伴生字段
- 内层块遮蔽外层块
- 方法参数遮蔽类字段

### 5.2 关键消歧表

| 表达式 | 上下文 | 解析结果 | 说明 |
|--------|--------|----------|------|
| `x` | 方法参数 `x` 存在 | 参数 | 参数优先于字段 |
| `x` | 局部 `x` 存在 | 局部 | 局部优先于参数 |
| `x` | 字段 `x` 存在，无局部 | `this.x` | 隐式字段访问 |
| `x` | 都不存在 | 报错 | `unresolved reference 'x'` |
| `this.x` | 字段 `x` 存在 | `this.x` | 强制字段访问 |
| `this.x` | 字段 `x` 不存在 | 报错 | `no field 'x' in class 'C'` |
| `this.x` | 局部 `x` 遮蔽字段 | `this.x`（字段） | 显式访问绕过遮蔽 |
| `x()` | 方法 `x` 存在，无参数 `x` | `this.x()` | 隐式方法调用 |
| `x()` | 伴生方法 `x` 存在 | `C.x()`（无 self） | 伴生方法不带 self |
| `this::x()` | 方法 `x` 存在 | `this.x()` | 强制实例方法 |
| `C.x()` | 伴生方法 | `C.x()` | 显式类名前缀 |
| `super.x` | 父类有字段 `x` | `super.x` | 强制父类访问 |
| `x` | 父类字段 `x` 存在，本类无 | `this.x`（沿继承链） | 继承字段访问 |

### 5.3 字段和方法同名的处理

在同一个类中，`x` 既作为字段又作为方法：

```aura
class Weird {
    val x: Int = 0
    fun x(n: Int): Int { return x + n }   // ✅ 语法合法，语义靠后缀判定
}
```

**解析规则**：
- 表达式位（无 `()`）`x` → **字段**
- 调用位 `x(...)` → **方法**

编译器**不报冲突**，但**发 linter warning**：
```
warning: field and method have the same name 'x' in class 'Weird'
  → consider renaming one of them for clarity
```

**注意**：这与 Kotlin 一致（Kotlin 也允许同名但推荐用 `@JvmName` 区分）。

### 5.4 遮蔽链与继承

```aura
class Base {
    var x: Int = 0
}

class Derived : Base() {
    var x: Int = 0         // ⚠️ 声明期报「same name as super class field」warning（可选）

    fun describe(): String {
        return x.toString()  // ✅ 访问 Derived.x（本类优先）
    }
}
```

**默认策略**：本类字段遮蔽父类字段（与 Java 一致）。编译器**发 warning**，不报错。

如果本类字段遮蔽父类字段而**本类方法想访问父类字段**，必须写 `super.x`。

### 5.5 泛型参数的作用域

```aura
class Builder<T> {
    var items: ArrayList<T> = ArrayList<T>()

    fun add(item: T): Unit {
        items.add(item)                // ✅ items 是字段（隐式 this）
    }

    fun <U> transform(x: U): U {
        items.add(item)               // ❌ U 是泛型参数，不是变量
        // 实际写：items.add(item) 中 item 是外部泛型 T 参数，OK
    }
}
```

**规则**：类型参数不占用「变量作用域」，与值域互不冲突。

---

## 六、Rust 编译器实现细节

### 6.1 修改点总览

**目标**：把 `class_bare_ident` / `bare_call_in_class` 从「sema 兜底路径」提升为「主解析路径」。

| 文件 | 修改性质 | 修改量估计 |
|------|----------|-----------|
| `rust/compiler/src/ast.rs` | 新增 `Expr::Self` 别名节点（可选） | +3 行 |
| `rust/compiler/src/lexer.rs` | 无修改（`self` 通过 `is_keyword` 检测） | 0 |
| `rust/compiler/src/token.rs` | 新增 `TokenKind::Self_`（可选，或复用 Ident） | +1 行 |
| `rust/compiler/src/parser.rs` | 支持 `self` 关键字（`Expr::This` 别名） | +5 行 |
| `rust/compiler/src/sema/checker.rs` | **主修改**：新增 `resolve_ident_in_class_scope` | +200 行 |
| `rust/compiler/src/sema/symbol.rs` | 类成员查找辅助（可能无需改） | 0 |
| `rust/compiler/src/sema/ty.rs` | 无 | 0 |
| `rust/compiler/src/codegen/hir.rs` | 提前插入 `this.x` 改写的时机：在 sema 完成后**主动改写** AST，而非仅在兜底时改写 | +300 行 |
| `rust/compiler/src/codegen/mir.rs` | 无（MIR 消费 HIR，不受影响） | 0 |
| `rust/compiler/src/codegen/emit.rs` | 无 | 0 |
| `rust/compiler/src/lsp.rs` | 补全候选增加隐式字段/方法 | +100 行 |
| `rust/compiler/src/docgen.rs` | 文档生成时识别隐式访问（可选） | +50 行 |

**总代码增量估计**：~650 行 Rust，主要在 sema 和 codegen/hir。

### 6.2 Parser 修改

```rust
// parser.rs —— parse_expression_primary 内部
let primary = if tok.kind == TokenKind::This {
    Expr::This(start)
} else if tok.kind == TokenKind::Super {
    Expr::Super(start)
} else if tok.literal == "self" {                // ← 新增：self 作为 this 别名
    Expr::This(start)                             // 复用同一 AST 节点
} else {
    Expr::Ident(tok.literal.clone(), tok.span)
};
```

**注意**：复用 `Expr::This`，不引入新 AST 节点，最小化对下游代码的冲击。

### 6.3 Semantic Analyzer 修改

**核心新增**：`resolve_ident_in_class_scope`

```rust
impl TypeChecker {
    /// 在类作用域内解析裸标识符，按 §4.1 原则 1 的顺序返回
    /// - Some(("local", ...))     —— 局部/参数
    /// - Some(("field", field_id, field_ty))
    /// - Some(("method", class_name, method_ty, insert_self))
    /// - Some(("companion", ...))  —— 伴生静态成员
    /// - Some(("inherit", ...))    —— 沿继承链
    /// - Some(("top_level", ...))  —— 顶层
    /// - None                      —— 未解析，报错
    fn resolve_ident_in_class_scope(
        &self,
        name: &str,
        span: Span,
    ) -> Option<ResolvedIdent> { ... }
}
```

**关键修改点**：

1. `check_expr` 处理 `Expr::Ident` 时，先尝试 `resolve_ident_in_class_scope`，而不是走裸标识符兜底路径。
2. 处理 `Expr::Call { callee: Ident(name), ... }` 时，检查 `name` 是否是当前类的方法（含伴生、继承）。
3. 处理 `Expr::MemberAccess { object: Ident("_implicit"), name }` 语义（内部改写产物），指向字段解析。

### 6.4 HIR 降级修改

**关键修改**：在 sema 完成后，对 AST 做**显式脱糖** pass：

```rust
// codegen/hir.rs —— 新增 AST 脱糖 pass
pub fn desugar_implicit_this(program: &mut Program) {
    for decl in &mut program.declarations {
        if let Decl::Class(c) = decl {
            desugar_class_body(c);
        }
    }
}

fn desugar_class_body(c: &mut ClassDecl) {
    // 1. 收集类字段、方法、伴生方法、继承链
    let class_ctx = build_class_ctx(c);

    // 2. 遍历方法体，改写裸标识符为 `self.x` 或 `Class.m(self, ...)`
    for m in &mut c.methods {
        rewrite_method_body(m, &class_ctx);
    }
    // ... init 块、构造函数、伴生对象
}

fn rewrite_method_body(m: &mut FnDecl, ctx: &ClassCtx) {
    let body = m.body.as_mut().unwrap();
    rewrite_expr(body, ctx);
}

fn rewrite_expr(e: &mut Expr, ctx: &mut ClassCtx) {
    match e {
        Expr::Ident(name, span) => {
            if ctx.is_local(name) { return; }   // 局部遮蔽，不改写
            if ctx.has_field(name) {
                *e = Expr::MemberAccess {
                    object: Box::new(Expr::This(*span)),
                    name: name.clone(),
                    span: *span,
                };
            }
        }
        Expr::Call { callee, .. } => {
            // 隐式方法调用：m() → Class.m(self, ...)
            if let Expr::Ident(name, span) = callee.as_ref() {
                if ctx.has_method(name) {
                    let new_callee = Expr::MemberAccess {
                        object: Box::new(Expr::Var("self".into(), *span)),
                        name: name.clone(),
                        span: *span,
                    };
                    *callee = Box::new(new_callee);
                } else if ctx.has_companion_method(name) {
                    // 伴生方法：不带 self，改为 Class.method
                    let new_callee = Expr::MemberAccess {
                        object: Box::new(Expr::Ident(ctx.class.clone(), *span)),
                        name: name.clone(),
                        span: *span,
                    };
                    *callee = Box::new(new_callee);
                }
            }
        }
        // 递归处理其他子节点
        Expr::Block(stmts, _) => for s in stmts { /* ... */ },
        // ...
    }
}
```

### 6.5 与现有 `CLASS_CTX` 的关系

现有 `ClassCtx` 结构（`hir.rs:200`）**继续保留**，用于**运行时降级**阶段的兜底（处理 sema 遗漏的极少数场景）。新引入的 `ClassCtx` 用于**编译期脱糖**（desugar pass），两者共享同一个数据结构定义。

**分工**：
- **Desugar pass**（新增）：把裸标识符改写为显式 `self.x` / `Class.m(...)`，改写后 sema 走正常路径
- **`CLASS_CTX` 兜底**（保留）：处理 desugar 未覆盖的极端情况（如 sema 类型缺失时的 fallback）

### 6.6 Diagnostics 修改

新增诊断消息（`rust/compiler/src/errors.rs`）：

```rust
pub enum CompileErrorKind {
    // ... 现有 ...
    /// 字段被局部变量遮蔽，隐式访问退化为局部
    FieldShadowedByLocal { field: String, local: String },
    /// 字段和方法同名
    FieldAndMethodSameName { name: String },
    /// 本类字段遮蔽父类字段
    FieldShadowsSuperField { name: String },
    /// 隐式方法调用找不到接收者（罕见）
    NoReceiverForImplicitCall { method: String },
    /// `this` 在非类上下文使用
    ThisOutsideClass { .. },
}
```

### 6.7 与 AOT / VM 的兼容性

- **VM**：字节码级 API 不受影响（`self` 参数、成员访问都是既有 opcodes）
- **AOT (LLVM)**：`%self` 参数在函数签名中的位置不变，隐式访问改写为显式 `getelementptr`
- **JIT (Cranelift)**：同上，隐式访问在 HIR→MIR 阶段已展开

**结论**：隐式 this 是纯语法/AST 层改造，**不触及 IR 和运行时**。

### 6.8 Formatter（loom fmt）

新增配置项：

```toml
# aura.toml
[format]
implicit-this = "auto"    # "on" | "off" | "auto"
                          # auto: 优先去掉 this.（除非遮蔽或必要场景）
                          # on:   保留/添加 this.
                          # off:  全部去掉（除非遮蔽）
```

**实现**：`loom fmt` 新增 AST 遍历，按 §4 规则决定改写。

---

## 七、Aura 自举编译器实现细节

### 7.1 现状分析

Aura 自举编译器（`aura/compiler/aura/lang/compiler/`）当前**没有类作用域解析**。`TypeChecker.aura`（30KB）主要是类型推断，没有 `ClassCtx`。

**必须同步实现**的原因：
- 自举编译器要编译的源码就是 Aura 源码本身
- 一旦 Rust 侧引入隐式 this，自举源码就可以去掉 `this.`
- 自举编译器若不支持，会**编译失败**

### 7.2 修改点总览

| 文件 | 修改性质 | 修改量估计 |
|------|----------|-----------|
| `aura/lang/compiler/parser/Parser.aura` | 支持 `self` 关键字，解析 `this::m` | +30 行 |
| `aura/lang/compiler/sema/SymbolTable.aura` | 新增 `ClassCtx` 数据结构 + 方法体作用域压栈 | +200 行 |
| `aura/lang/compiler/sema/TypeChecker.aura` | 裸标识符解析路径改写 | +300 行 |
| `aura/lang/compiler/hir/Hir.aura` | `lowerFunction` 前插入 desugar pass | +400 行 |
| `aura/lang/compiler/hir/Desugar.aura` | 新增 `DesugarImplicitThis` pass | +500 行 |
| `aura/lang/compiler/errors/CompileError.aura` | 新增诊断类型 | +30 行 |
| **总计** | | **~1500 行** |

### 7.3 新增 `ClassCtx.aura`

```aura
// aura/lang/compiler/sema/ClassCtx.aura
package aura.lang.compiler.sema

/// 类作用域上下文（Kotlin/Java 风格隐式 this 解析）
class ClassCtx {
    var className: String = ""
    var fields: ArrayList<String> = ArrayList<String>()
    var methods: ArrayList<String> = ArrayList<String>()
    var companionMethods: ArrayList<String> = ArrayList<String>()
    var companionFields: ArrayList<String> = ArrayList<String>()
    var superClass: String = ""
    var locals: ArrayList<ArrayList<String>> = ArrayList<ArrayList<String>>()

    fun pushScope() {
        locals.add(ArrayList<String>())
    }

    fun popScope() {
        locals.remove(locals.size() - 1)
    }

    fun registerLocal(name: String) {
        val top = locals.get(locals.size() - 1)
        top.add(name)
    }

    fun isLocal(name: String): Boolean {
        for (i in 0..locals.size()) {
            if (locals.get(i).contains(name)) {
                return true
            }
        }
        return false
    }

    fun hasField(name: String): Boolean {
        return fields.contains(name)
    }

    fun hasMethod(name: String): Boolean {
        return methods.contains(name)
    }

    fun hasCompanionMethod(name: String): Boolean {
        return companionMethods.contains(name)
    }
}
```

### 7.4 `DesugarImplicitThis.aura`

仿照现有 `Desugar.aura`（9.3KB）的结构，新增一个 pass：

```aura
// aura/lang/compiler/hir/DesugarImplicitThis.aura
package aura.lang.compiler.hir

/// 隐式 this 脱糖 pass。
///
/// 输入：包含裸标识符的 HIR（`this.x` 尚未显式化）
/// 输出：所有隐式访问已改写为 `self.x` / `Class.m(...)` 的 HIR
class DesugarImplicitThis {
    var src: Hir = Hir()
    var dst: Hir = Hir()
    var ctx: ClassCtx = ClassCtx()

    fun desugar(srcHir: Hir, ctx: ClassCtx): Hir {
        this.src = srcHir
        this.dst = Hir()
        this.ctx = ctx
        return this.desugarNode(srcHir, srcHir.rootId)
    }

    fun desugarNode(src: Hir, id: Int): Int {
        val kind = src.kindOf(id)
        if (kind == "HirIdent") {
            val name = src.textOf(id)
            val sp = src.spanOf(id)
            // 隐式字段访问：name → HirMember { HirVar("self"), name }
            if (ctx.hasField(name) && !ctx.isLocal(name)) {
                val selfId = this.dst.leaf("HirVar", "self", sp)
                return this.dst.add("HirMember", name, "", sp, kidsAdd(noKids(), selfId))
            }
            // 类型名、其他标识符：原样返回
        }
        // ... 其他节点类型，递归
        return this.dst.add(kind, src.textOf(id), src.tyOf(id), src.spanOf(id), newKids)
    }
}
```

### 7.5 与 `lowerFunction` 的集成

在 `Hir.aura:lowerFunction` 之前调用：

```aura
fun lowerProgram(ast: Ast, program: Int): Int {
    // ... 现有逻辑

    // 新增：在进入类作用域前，先构建 ClassCtx
    // 在降低每个类的方法体前，先跑 DesugarImplicitThis pass

    // 伪代码
    for (i in 0..ast.kidsCount(program)) {
        val decl = ast.kidsAt(program, i)
        if (ast.kindOf(decl) == "Class") {
            val ctx = buildClassCtx(decl)
            // 对类内每个方法：先 desugar，再 lowerFunction
            for each method in class.members {
                val desugared = DesugarImplicitThis().desugar(method.hir, ctx)
                lowerFunction(ast, desugared, ctx)
            }
        }
    }
}
```

### 7.6 自举源码迁移策略

**🔑 决策**：**不迁移既有 10,907 处 `this.` 前缀**。

- 自举源码保留原样，Rust + Aura 两侧编译器都能正确编译**带 `this.` 前缀的老写法**
- 新写的自举源码**可以省略** `this.`，与老代码在同一个类中自由共存
- 不引入 `aura migrate` 作为默认流程；工具作为**可选**独立 CLI 保留（团队主动使用时才跑）

**理由**：
1. **零风险**：不改动既有代码就不会引入编译回归；老源码的 diff 面归零
2. **降低评审成本**：不需要审查 10,907 行 diff
3. **降低 CI 负担**：不需要跑大规模 diff-based 测试
4. **渐进式推广**：新功能/新模块自然使用新语法；老模块按需手动改写
5. **符合"向后兼容"原则**：老代码继续工作，新代码获得简洁性

**唯一需要做的自举侧同步工作**：
1. Aura 自举编译器实现 `ClassCtx.aura` + `DesugarImplicitThis.aura` + TypeChecker 消歧
2. **验证**自举编译器能正确编译**既有 10,907 处带 `this.` 前缀的自举源码**（不改动）
3. 后续**新写**的 Aura 文件可以自由省略 `this.`

---

## 八、测试策略

### 8.1 单元测试

在 `rust/compiler/tests/` 下新增：

```rust
// tests/implicit_this_tests.rs
mod tests {
    use aura_compiler::sema::TypeChecker;
    use aura_compiler::parser::Parser;
    use aura_compiler::lexer::Lexer;

    fn compile(code: &str) -> Result<HirProgram, Vec<CompileError>> { ... }

    #[test]
    fn test_implicit_field_access() {
        let code = r#"
            class Counter {
                var count: Int = 0
                fun bump(): Int {
                    count = count + 1
                    return count
                }
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }

    #[test]
    fn test_implicit_field_with_local_shadow() {
        let code = r#"
            class Counter {
                var count: Int = 0
                fun f(): Int {
                    val count = 5
                    return count          // 返回 5，非 this.count
                }
            }
        "#;
        let result = compile(code);
        // 允许通过，但必须有 warning
        assert_has_warning(result, "field 'count' is shadowed");
    }

    #[test]
    fn test_explicit_this_bypasses_shadow() {
        let code = r#"
            class Counter {
                var count: Int = 0
                fun f(): Int {
                    val count = 5
                    return this.count     // 返回字段值
                }
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }

    #[test]
    fn test_implicit_method_call() {
        let code = r#"
            class C {
                fun f(): Int = 1
                fun g(): Int { return f() }
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }

    #[test]
    fn test_companion_method_no_self() {
        let code = r#"
            class C {
                companion object {
                    fun static(): Int = 1
                    fun f(): Int = 2
                }
                fun call_static(): Int = static()
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }

    #[test]
    fn test_inherited_field_access() {
        let code = r#"
            class Base { var x: Int = 0 }
            class D : Base() {
                fun get(): Int = x
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }

    #[test]
    fn test_this_in_top_level_error() {
        let code = r#"
            fun f() { return this }
        "#;
        let result = compile(code);
        assert_error(result, "this can only be used inside a class/struct/actor");
    }

    #[test]
    fn test_super_access() {
        let code = r#"
            class Base { fun m(): Int = 1 }
            class D : Base() {
                fun m(): Int { return super.m() }
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }

    #[test]
    fn test_lambda_capture_implicit() {
        let code = r#"
            class C {
                var x: Int = 0
                fun f(): () -> Int { return { x } }
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }

    #[test]
    fn test_accessor_field_keyword() {
        let code = r#"
            class P {
                var name: String = ""
                    get() { return field }
                    set(v: String) { field = v }
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }

    #[test]
    fn test_self_alias() {
        let code = r#"
            class C {
                var x: Int = 0
                fun get(): Int = self.x
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }

    #[test]
    fn test_this_arrow_method() {
        let code = r#"
            class C {
                fun f(): Int = 1
                fun g(): Int = this::f()
            }
        "#;
        let hir = compile(code).unwrap();
        assert_no_errors(&hir);
    }
}
```

### 8.2 集成测试

在 `tests/` 下新增 `tests/phase_implicit_this/`：

```
tests/phase_implicit_this/
├── implicit_field_basic.aura
├── implicit_field_shadowing.aura
├── implicit_method_basic.aura
├── implicit_method_companion.aura
├── implicit_inheritance.aura
├── lambda_capture.aura
├── accessor_field.aura
├── this_arrow.aura
└── error_cases.aura
```

### 8.3 自举测试

在 `aura/compiler` 下新增：

```
aura/tests/phase4_implicit_this_tests.aura
```

参照 `phase3_mir_tests.aura` 的结构，验证自举编译器的隐式 this 支持。

### 8.4 快照测试

更新现有快照（`tests/snapshots.rs`）以反映新的 HIR 输出。

### 8.5 性能基准

新增 `bench_implicit_this.rs`，对比启用/禁用隐式 this 时的编译耗时。**预期**：隐式 this 使编译耗时增加 **~3%**（sema 多一次解析尝试），但新代码字数减少 20–35%。

### 8.6 向后兼容回归测试（**关键**）

**目标**：证明**既有 10,907 处 `this.` 前缀**在引入隐式 this 后**编译行为完全不变**。

**测试内容**：
1. **快照回归**：编译 `aura/compiler/**/*.aura`（**不改动源码**），对比本方案落地前后的字节码/AST 快照，要求 **100% 一致**
2. **端到端运行**：跑 `tests/bootstrap_test.rs` 全量测试，确保 VM/JIT/AOT 三端行为不变
3. **示例回归**：跑 `examples/` 下所有 `.aura` 文件（含带 `this.` 前缀的老示例），要求全部通过

**CI 集成**：新增 `regression_implicit_this.sh`，每次 PR 自动运行，失败即阻塞合并。

**验证脚本示例**：
```bash
# 编译前快照
cargo run --bin aura -- compile aura/compiler/**/*.aura --snapshot > before.auc
# 应用本方案后
cargo run --bin aura -- compile aura/compiler/**/*.aura --snapshot > after.auc
# 对比
diff before.auc after.auc || echo "❌ 老代码行为发生变化，需修复"
```

---

## 九、LSP 集成

### 9.1 补全候选

`rust/compiler/src/lsp.rs` 修改：

```rust
fn complete_ident(&self, position: Position) -> Vec<CompletionItem> {
    // ... 现有候选 ...

    // 新增：在类作用域内，追加隐式可见的候选
    if let Some(ctx) = self.class_ctx_at(position) {
        for field in &ctx.effective_fields {
            items.push(CompletionItem {
                label: field.clone(),
                detail: format!("field ({})", ctx.class_name),
                kind: CompletionItemKind::Field,
            });
        }
        for method in &ctx.effective_methods {
            items.push(CompletionItem {
                label: format!("{}()", method),
                detail: format!("method ({})", ctx.class_name),
                kind: CompletionItemKind::Method,
            });
        }
    }
}
```

### 9.2 跳转（Go To Definition）

`rust/compiler/src/lsp.rs` 修改：
- `x` 在类内被点击 → 跳转到字段声明（或伴生字段）
- `x()` → 跳转到方法声明

### 9.3 悬停（Hover）

显示「隐式访问」提示：
```
x : Int (field of Counter, accessed implicitly)
```

### 9.4 重构（Rename）

重命名字段时，同步重命名：
- 显式 `this.x` 引用
- 隐式 `x` 引用（**注意**：局部遮蔽场景不重命名）

---

## 十、Formatter 设计

### 10.1 语法

`loom fmt --implicit-this={preserve,implicit}`

或项目配置：
```toml
# aura.toml
[format]
implicit-this = "preserve"   # 默认：不改动既有 this.
# implicit-this = "implicit" # 可选：主动改写为隐式（团队主动要求）
```

### 10.2 两种模式行为

| 模式 | 行为 | 典型场景 |
|------|------|----------|
| `preserve` | **默认**。保留既有 `this.` 前缀不动；格式化不改写隐式/显式 | 团队既有代码继续工作；渐进式推广 |
| `implicit` | **可选**。把可省略的 `this.` 前缀去掉 | 团队主动切换到 Kotlin 风格；一次性改写 |

### 10.3 `preserve` 模式（默认）

- **不改动**任何既有 `this.x` / `this.m()`
- 也不主动给隐式访问**添加** `this.`
- 只做空白/缩进/换行等常规格式化
- 对新写的代码，**用户自己决定**要不要写 `this.`（Formatter 不干预）

### 10.4 `implicit` 模式（可选）

仅在团队显式启用时生效：

1. 遍历方法体，收集遮蔽关系
2. 对每个 `this.x` 判断：
   - 若 `x` 与局部变量同名 → **保留** `this.`（否则会歧义）
   - 若 `x` 是伴生方法/静态 → **不能去掉**（会误改）
   - 否则 → **去掉** `this.`
3. 对裸标识符（原本没有 `this.`）**不动**

### 10.5 实现位置

`rust/compiler/src/codegen/opt.rs`（或新增 `src/format.rs`），在 `loom fmt` 命令中调用。

**注意**：`preserve` 模式下，Formatter 的 AST 遍历**必须跳过**所有 `Expr::This` 节点（保持原样）；只有 `implicit` 模式才会触发改写逻辑。

---

## 十一、迁移与兼容性

### 11.1 向后兼容保证（**核心承诺**）

- **老代码一行不改**：既有 10,907 处 `this.` 前缀**全部保留**，编译行为完全不变
- **新代码可选**：新写的类/方法可以省略 `this.`，与老代码共存
- **同一文件自由混用**：一个类里可以同时有 `this.x` 和裸 `x`，编译器按相同规则解析
- **次构造函数委托** `: this(...)` 继续支持
- **`super(...)` / `: Base(...)`** 继续支持
- **`super.` / `this::m()`** 语义不变

### 11.2 版本控制

**默认策略**：语言版本 `0.6+` 启用隐式 this 支持，`0.5` 及以下**也支持**（隐式 this 是"能力增量"，不破坏老语法）。

- **不需要** `--language-version=0.5` 兼容性开关（无语法被废弃）
- 老代码在所有版本下都能编译

### 11.3 迁移工具（**可选，非默认流程**）

新增独立 CLI：`aura migrate --implicit-this`

**⚠️ 关键约束**：
- **不作为默认推广流程**，团队**主动要求**时才使用
- **仅作为代码风格工具**，不承诺行为变更（AST 层等价改写）
- 生成的 diff 必须人工 review，尤其是遮蔽场景

**运行方式**：
```bash
aura migrate --implicit-this --path=./src/       # 生成改写后源码
aura migrate --implicit-this --in-place          # 直接改写
aura migrate --implicit-this --dry-run           # 仅显示 diff
```

**注意**：即使不用迁移工具，团队也能通过「写新代码时用隐式风格」自然推广，无需批量改。

### 11.4 分阶段推广

```
Phase 1：Rust 编译器实现 + 测试覆盖（隐式 this 单元测试、集成测试）
Phase 2：Aura 自举编译器同步实现 + 自举源码验证（不改动，只验证编译通过）
Phase 3：Formatter preserve 默认 + implicit 可选
Phase 4：LSP 增强（补全/跳转/悬停）
Phase 5：迁移工具（可选，团队主动运行时才跑）
```

**Phase 2 的关键点**：Aura 自举编译器实现完成后，**必须**跑一次自举源码的完整编译（不改动源码），确认 10,907 处既有 `this.` 前缀全部编译通过。

---

## 十二、命名保留与冲突处理

### 12.1 软保留字

| 名称 | 用途 | 使用规则 |
|------|------|----------|
| `self` | `this` 的别名 | 表达式位；作为标识符需要 `$$self` 或反引号 `` `self` `` |
| `field` | 属性访问器上下文关键字 | 仅在访问器体内生效，其他位置作为普通标识符 |

### 12.2 硬保留字

以下词不能作为标识符（Aura 已有）：
`val`, `var`, `fun`, `class`, `object`, `init`, `constructor`, `companion`, `interface`, `struct`, `enum`, `import`, `package`, `this`, `super`

新增硬保留字：`self`（可选）

### 12.3 冲突示例

```aura
class C {
    fun transform(self: Int): Int {       // ❌ self 是保留字
        return self
    }
    fun transform($self: Int): Int {      // ✅ 用 $ 前缀
        return $self
    }
    fun transform(`self`: Int): Int {     // ✅ 反引号包裹
        return `self`
    }
    fun transform(receiver: Int): Int {   // ✅ 推荐，避免保留字冲突
        return receiver
    }
}
```

---

## 十三、已知限制与开放问题

### 13.1 已知限制

1. **无嵌套类语法**：Aura 目前不支持类中定义类，`this@Label` 语法不需要
2. **无内部类（inner class）**：Kotlin 的「内部类持有外部类引用」不支持，隐式访问无歧义
3. **无匿名类**：匿名类语法未来若引入，需要考虑 `super` 与 `this` 的嵌套

### 13.2 开放问题

| 问题 | 建议 | 待决策 |
|------|------|--------|
| `self` 是否作为关键字 | ✅ 引入（Kotlin 风格） | 需要 LSP/Formatter 配套 |
| 隐式 this 是否作用于顶层脚本 | ❌ 不作用（顶层无 `this`） | 已定 |
| 伴生方法是否隐式调用 | ✅ 隐式调用（`Class.m(...)` 不带 self） | 已定 |
| 遮蔽是否报错 | ❌ 只告警，不报错（Kotlin 一致） | 已定 |
| `this` 能否作为属性 | ❌ 硬保留字 | 已定 |
| 泛型接收者类型 | 未来议题（`fun <T>(self: T) ...`） | 待决策 |
| 逃逸闭包捕获语义 | 与 Kotlin 一致（隐式捕获字段） | 已定 |

### 13.3 与未来议题的关系

- **Extension Function**：扩展函数的隐式 `receiver` 语法与本方案正交，可以独立引入
- **Operator Overload**：运算符重载已经隐式带 `self`（Rust 侧 `Class.plus(a, b)`），本方案不改变
- **Property Delegation**：`by` 关键字未来引入时，需要与隐式 this 协同设计
- **Coroutine**：`suspend` 函数中的 `this` 语义与协程接收者正交，独立议题

---

## 十四、工作量与时间线

### 14.1 工作量估算

| 组件 | 工作量 | 说明 |
|------|--------|------|
| Rust: Parser + AST | 1 天 | `self` 关键字支持 |
| Rust: Sema (核心) | 5 天 | `resolve_ident_in_class_scope` + 消歧 |
| Rust: HIR (脱糖) | 3 天 | DesugarImplicitThis pass |
| Rust: LSP | 2 天 | 补全、跳转、悬停 |
| Rust: Formatter (`preserve` + `implicit`) | 2 天 | 两种模式实现 |
| Rust: 测试 | 3 天 | 单元 + 集成 + 快照 |
| Rust: 迁移工具（可选） | 2 天 | `aura migrate --implicit-this` |
| Aura: ClassCtx.aura | 1 天 | 数据结构 + 作用域栈 |
| Aura: TypeChecker.aura | 3 天 | 消歧规则 |
| Aura: DesugarImplicitThis.aura | 3 天 | HIR 脱糖 pass |
| Aura: 测试 + 既有源码验证 | 3 天 | 自举测试 + 10,907 处 `this.` 前缀回归测试 |
| 文档 + 示例 | 1 天 | 更新 book 与示例 |

**总工作量**：**~29 人日**（含并行）

> **说明**：本估算**不包含**「自举源码批量迁移」——根据 §7.6 的决策，既有代码保留原样不改动。仅当团队主动要求批量改写时才另计成本（预计 +3 天）。

### 14.2 时间线

```
Week 1: Rust sema + hir 核心实现（并行开发）
Week 2: Rust 测试 + LSP/Formatter + 迁移工具（可选）
Week 3: Aura 自举侧同步实现 + 既有源码回归测试
Week 4: 收尾、文档、示例、发布
```

### 14.3 依赖

- 依赖：Aura 自举编译器当前处于 **P4 进行中**（README 状态），本方案落地前需保证 P4 稳定
- 不依赖：其他语法糖议题（extension function、operator overload 等）

---

## 十五、附录

### 附录 A：完整示例

#### A.1 老代码（保留原样，不改动）

```aura
// 现状 —— 全部带 this. 前缀
class Counter {
    var count: Int = 0

    init() {
        this.count = 0
    }

    fun bump(): Int {
        this.count = this.count + 1
        return this.count
    }

    fun display(): String {
        return "count=" + this.count.toString()
    }
}
```

#### A.2 新代码（可选，省略 `this.`）

```aura
// 新写法 —— 隐式 this
class Counter {
    var count: Int = 0

    init() {
        count = 0                                // ✅ 隐式
    }

    fun bump(): Int {
        count = count + 1                        // ✅ 隐式
        return count                             // ✅ 显式返回也可以
    }

    fun display(): String {
        return "count=" + count.toString()       // ✅ 隐式
    }

    // 遮蔽场景：
    fun displayWithLocal(): String {
        val count = 42                           // 局部遮蔽
        return "local=" + count                  // ✅ 访问局部
    }

    // 显式访问字段：
    fun forceFieldAccess(): Int {
        val count = 42
        return this.count                        // ✅ 强制字段
    }
}
```

#### A.3 新老共存（同一文件、同一类、甚至同一方法体内）

```aura
// 一个类里既有 this. 前缀（老风格），也有裸标识符（新风格）
// 编译器按相同规则解析，行为等价。
class Hybrid {
    var x: Int = 0
    var y: Int = 0

    // 老方法：保留 this. 前缀
    fun oldStyle(): Int {
        this.x = 10
        this.y = 20
        return this.x + this.y
    }

    // 新方法：使用隐式访问
    fun newStyle(): Int {
        x = 30
        y = 40
        return x + y
    }

    // 混合方法：同一个方法体里混用（合法，但不推荐）
    fun mixedStyle(): Int {
        x = 100          // 隐式
        this.y = 200     // 显式
        return x + y     // 隐式（返回 300）
    }

    // 遮蔽场景：隐式访问退化为局部；必须用 this. 才能访问字段
    fun withShadowing(): Int {
        val x = 999                       // 局部遮蔽
        x = x + 1                         // ✅ 访问局部
        return this.x + x                 // ✅ 显式字段 + 隐式局部
    }
}
```

**编译器视角**：`x`（隐式）和 `this.x`（显式）在 HIR 中会被改写为**完全相同的节点** `HirMember { HirVar("self"), "x" }`——两者的语义完全等价，差异仅在**源码文本**层。

**团队策略建议**：
- 老代码保留 `this.` 不动，避免大 diff
- 新模块/新功能自由使用隐式写法
- 团队约定在**同一个 PR 内保持一致风格**（例如"改这个类的这个方法时，全部改成新风格"或"这个类继续用老风格"）

### 附录 B：语法参考（完整 BNF 摘录）

```
ClassDecl  := [Visibility] 'class' Identifier <TypeParams> [':' TypeRef] [ImplementationList]
                '{' ClassMember* '}'

ClassMember:= [Visibility] FieldDecl | MethodDecl | PropertyDecl | Companion | Constructor
           | InitBlock | Annotation

FieldDecl  := ('val' | 'var' | 'const') Identifier [': Type'] [ '=' Expression ]
MethodDecl := MethodModifier* 'fun' <TypeParams>? Identifier <Params>
                [':' Type'] ['{' Block '}' | '=' Expression]
PropertyDecl := ('val' | 'var') Identifier [': Type']
                ['{' AccessorBody '}' | 'by' Expression]
Constructor:= 'init' <Params> [':' Delegation] ['{' Block '}']
           | 'constructor' <Params> [':' Delegation] ['{' Block '}']
Companion  := 'companion' 'object' [Identifier] '{' ClassMember* '}'

Expression := ... | ThisExpr | SuperExpr | Primary Postfix*
ThisExpr   := 'this' [ '::' Identifier ]        // :: 强制方法调用
           | 'self'                             // 别名
Delegation := 'this' '(' ArgList ')'            // 次构造函数
           | 'super' '(' ArgList ')'
           | Identifier '(' ArgList ')'         // Aura 风格父类

// 隐式 this 只在 class / object / actor 的方法体、init 块、访问器体内生效
// 顶层函数、脚本模式、伴生方法（无 self）不适用
```

### 附录 C：与 Rust 侧现有代码的映射

| 现有位置 | 现有作用 | 新方案调整 |
|----------|----------|------------|
| `hir.rs:73 CLASS_CTX` | 运行时降级上下文 | **保留**，兜底用途 |
| `hir.rs:200 ClassCtx` | 类作用域数据结构 | **复用**，加泛型 |
| `hir.rs:330 class_bare_ident` | 裸字段兜底改写 | **保留**，作为 desugar 的 fallback |
| `hir.rs:352 bare_call_in_class` | 裸方法兜底改写 | **保留**，作为 desugar 的 fallback |
| `hir.rs:495 field_receiver_method` | 字段接收者兜底 | 保留 |
| `hir.rs:3663 ACCESSOR_PROP` | `field` 上下文关键字 | 保留，规则不变 |
| `sema/checker.rs:2775 Expr::This` | `this` 关键字类型推断 | **保留** |
| `parser.rs:3154 TokenKind::This` | `this` 关键字解析 | **保留**，添加 `self` 别名 |
| **新增** `hir.rs desugar_implicit_this` | 编译期脱糖 pass | 新增 |
| **新增** `sema/checker.rs resolve_ident_in_class_scope` | 主解析路径 | 新增 |

### 附录 D：诊断消息汇总

```
error: this can only be used inside a class/struct/actor
  → this outside class body

error: no field 'x' in class 'C'
  → this.x where 'x' is not a declared field

error: unresolved reference 'foo'
  → bare identifier not found in class scope

warning: field 'count' is shadowed by parameter/local 'count'
  → implicit access resolves to local; use 'this.count' for field access

warning: field 'x' and method 'x' have the same name in class 'C'
  → consider renaming for clarity

warning: field 'x' shadows super class field
  → use 'super.x' to access parent's field

warning: unnecessary 'this.' — field 'x' is not shadowed
  → can be simplified to bare 'x'
```

### 附录 E：Java/Kotlin/C#/Swift 对比速查

```
                     Java    Kotlin   C#      Swift   C++     Aura (新)
字段隐式访问        ✅       ✅      ❌       ✅      ❌      ✅
方法隐式调用        ✅       ✅      ❌       ✅      ❌      ✅
局部遮蔽字段        ✅       ✅      n/a      ✅      n/a     ✅
逃逸闭包强制 self   n/a      n/a     n/a      ✅      n/a     ❌ (与 Kotlin 一致)
field 关键字        ❌       ✅      ❌       ❌      ❌      ✅ (访问器内)
this@Label          ❌       ✅      ❌       ❌      ❌      ❌ (无嵌套类)
伴生方法隐式        ❌       ✅      ❌       ❌      ❌      ✅
```

---

## 十六、决策清单

**核心决策**：

- [x] 引入隐式 this 语法糖（Kotlin/Java 风格）
- [x] 新增 `self` 关键字作为 `this` 别名（可选）
- [x] 局部遮蔽字段，隐式访问退化为局部
- [x] 显式 `this.` 用于绕过遮蔽
- [x] `field` 上下文关键字仅访问器体内生效
- [x] 伴生方法隐式调用不带 self
- [x] 遮蔽只告警不报错
- [x] Formatter 提供 `preserve`（默认）+ `implicit`（可选）两档
- [x] 保留 `this.` 前缀的向后兼容（**无语法废弃**）
- [x] 自举编译器同步支持
- [x] **🔑 老代码不改动**（10,907 处 `this.` 前缀保留）
- [x] **🔑 新代码可选**省略 `this.`（新老写法在同一文件/类/方法体内自由共存）
- [x] **🔑 迁移工具仅作可选工具**（不作为默认推广流程）
- [x] **🔑 Formatter 默认不改写**既有 `this.`（`preserve` 模式）

**待决策**：

- [ ] `self` 是否作为**硬保留字**（阻塞其他位置使用），还是**软保留字**（仅表达式位识别）
- [ ] 是否在 book 与 examples 中**新增**以隐式 this 风格为主的示例（老示例保留原样）
- [ ] 迁移工具是否作为独立 CLI 子命令（`aura migrate`），还是内置在 `loom fmt` 里

---

**文档结束**。

> 本方案不修改任何代码。所有实现细节仅供参考，实际开发时需按团队评审结果微调。
