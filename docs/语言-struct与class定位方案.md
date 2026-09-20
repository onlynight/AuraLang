# Aura 类型系统设计方案：`struct` → `value class`，保留 `class` + `actor`

> **状态**：设计方案 | **优先级**：P0（语言核心） | **目标版本**：v0.5 引入 → v1.0 废弃 struct → v2.0 移除
> **背景**：当前 `struct` 与 `class` 语法几乎重叠，但 `struct` 是唯一有运行时支持的类型。
> 开发者困惑"什么时候用哪个"。本方案统一语法、明确定位、平滑迁移。

---

## 一、问题陈述

### 1.1 当前状态

| 层级 | `struct` | `class` | `actor` |
|------|----------|---------|---------|
| AST | StructDecl（无 superclass） | ClassDecl（有 superclass） | ActorDecl |
| Parser | ✅ 完整 | ✅ 完整 | ✅ 完整 |
| Sema | ✅ 注册类型 + 方法 | ✅ 注册类型 + override 检查 | ✅ 注册类型 + 方法 |
| HIR | ✅ 保留为 HirStruct（值布局） | ❌ 仅展开方法为独立函数（类型丢弃） | ❌ 不保留 |
| AOT/LLVM | ✅ `%struct.Name = type {...}` | ❌ 不发射任何类型 | ❌ 不发射 |
| C 后端 | ✅ `typedef struct {...} Name` | ❌ 不发射 | ❌ 不发射 |

### 1.2 核心矛盾

- `struct` 和 `class` 语法几乎一样（都有字段、方法、泛型、`sealed`、`data` 修饰符）
- 但只有 `struct` 有完整的代码生成支持
- `class` 的 `superclass` 只用于 override 一致性检查，无运行时语义
- 开发者困惑：为什么有两个几乎一样的关键字？

### 1.3 示例分布

扫描 36 个 `.aura` 示例文件：
- `struct` 声明：**15 处**（75%）
- `class` 声明：**7 处**（35%），集中在 2 个教学文件
- 实际应用 demo（游戏、服务器、工具）：**全部用 struct，class = 0**

---

## 二、设计原则

### 2.1 三个概念，三个关键字

```
class          → 引用类型（堆分配、继承、多态）
value class    → 值类型（栈内联、值拷贝、无继承、映射 C struct）
actor          → 并发实体（消息传递、监督树）
```

- `struct` 作为 `value class` 的别名保留，平滑迁移
- `actor` 保持独立关键字（语义与 class 根本不同，不做合并）

### 2.2 默认选择：`class`

```
默认 → class（最通用，OOP 基础）
  ├── 需要值语义？  → value class
  └── 需要并发？    → actor
```

> **口诀：默认 `class`。需要值语义才 `value class`，需要并发才 `actor`。**

---

## 三、完整语法表

| 声明 | 含义 | 分配 | 继承 | 多态 | ARC | FFI |
|------|------|------|------|------|-----|-----|
| `class Foo` | 引用类型 | 堆 | ✅ | ✅ 虚方法 | ✅ | ❌ |
| `value class Foo` | 值类型 | 栈内联 | ❌ | ❌（用 when+is） | ❌ | ✅ 映射 C struct |
| `actor Foo` | 并发实体 | 运行时 | ❌ | ❌（消息） | ❌ | ❌ |
| `data class Foo` | 引用数据类 | 堆 | ✅ | ✅ | ✅ | ❌ |
| `value data class Foo` | 值数据类 | 栈 | ❌ | ❌ | ❌ | ✅ |
| `sealed class Foo` | 受控引用类型 | 堆 | ✅ 受限 | ✅ | ✅ | ❌ |
| `sealed value class Foo` | 受控值类型 | 栈 | ❌ | ❌ | ❌ | ✅ |
| `struct Foo` | ⚠️ deprecated | 等价于 `value class Foo` | ❌ | ❌ | ❌ | ✅ |

---

## 四、修饰符组合规则

| 组合 | 合法 | 说明 |
|------|------|------|
| `class` | ✅ | 引用类型，默认 |
| `value class` | ✅ | 值类型 |
| `data class` | ✅ | 引用数据类 |
| `value data class` | ✅ | 值数据类 |
| `sealed class` | ✅ | 受控引用类型 |
| `sealed value class` | ✅ | 受控值类型 |
| `actor` | ✅ | 并发实体（独立关键字） |
| `actor class` | ❌ | actor 不是 class 的修饰符 |
| `value actor` | ❌ | actor 不是 value class 的修饰符 |
| `class : Base()` | ✅ | 引用类型支持继承 |
| `value class : Base()` | ❌ | 值类型不支持继承（无 vtable） |

---

## 五、决策框架

```
需要什么？
  │
  ├── 并发实体（跨线程消息传递、监督树）
  │     → actor
  │
  └── 否 → 需要什么？
            │
            ├── 值语义（栈分配、值拷贝、FFI 映射 C struct）
            │     → value class
            │
            └── 否（继承、多态、身份、通用场景）
                  → class（默认）
```

### 决策表

| 场景 | 用什么 | 理由 |
|------|--------|------|
| 纯数据容器（坐标、颜色、配置） | `value class` | 值语义，不可变，无泄漏 |
| FFI 互操作（映射 C struct） | `value class` | 直接映射，无堆开销 |
| 游戏物理（向量、矩阵、变换） | `value class` | 性能敏感，值拷贝安全 |
| 函数返回值（创建临时对象） | `value class` | 无 ARC 泄漏风险 |
| 需要继承的层级（Animal/Dog/Cat） | `class` | 虚方法派发 |
| 需要多态（Drawable 引用列表） | `class` | 接口 + 继承 |
| 需要身份（单例、注册表、观察者） | `class` | 引用相等 |
| 并发实体（服务器、调度器） | `actor` | 消息传递 |
| 不确定 | `class` | 最通用，后续按需迁移 |

---

## 六、示例

### 6.1 游戏开发

```aura
// 引用类型：需要继承的游戏循环
sealed class GameLoop {
    fun update(dt: Float) {}    // 虚方法
    fun render() {}             // 虚方法
    fun run() { /* 模板方法 */ }
}

class PlatformGame : GameLoop() {
    override fun update(dt: Float) { /* 物理更新 */ }
    override fun render() { /* 绘制 */ }
}

// 值类型：数据 + 无继承
value class Vec2(val x: Float, val y: Float) {
    fun length(): Float = sqrt(x * x + y * y)
}

value data class Player(val id: Int, var hp: Int = 100, var x: Float = 0f, var y: Float = 0f)

// 并发实体：输入/网络
actor InputHandler {
    fun poll(): Event? { /* 轮询输入 */ }
}

actor NetworkManager {
    fun send(msg: String) { /* 发送消息 */ }
}
```

### 6.2 Web 服务器

```aura
// 值类型：配置、请求、响应
value data class Config(val port: Int = 8080, var debug: Boolean = false)
value class HttpRequest(val method: String, val path: String, val body: String)
value class HttpResponse(val status: Int, val body: String)

// 引用类型：中间件链
class Middleware {
    fun handle(req: HttpRequest): HttpResponse? = null
}

class AuthMiddleware : Middleware() {
    override fun handle(req: HttpRequest): HttpResponse? { /* 鉴权 */ }
}

class Router : Middleware() {
    override fun handle(req: HttpRequest): HttpResponse? { /* 路由 */ }
}

// 并发实体：服务器
actor Server(config: Config) {
    var running: Boolean = false
    var connections: Int = 0
    fun start() { running = true }
    fun stop() { running = false }
}
```

### 6.3 工具脚本

```aura
// 全部 value class
value class FileStats(val name: String, val size: Int, val lines: Int, val words: Int)
value class PackageInfo(val name: String, val version: String, val author: String)

fun analyzeText(content: String, name: String): FileStats {
    /* ... */
}
```

### 6.4 密封类型 + 多态

```aura
// 密封值类型
sealed value class Shape {
    fun area(): Float = 0.0f
}

value class Circle(val r: Float) : Shape {
    override fun area(): Float = 3.14f * r * r
}

value class Rectangle(val w: Float, val h: Float) : Shape {
    override fun area(): Float = w * h
}

// 手动派发（值类型无 vtable）
fun computeArea(s: Any): Float {
    return when (s) {
        is Circle    -> s.area()
        is Rectangle -> s.area()
        else         -> 0.0f
    }
}
```

### 6.5 枚举（不变）

```aura
enum Color {
    RED,
    GREEN,
    CUSTOM(val r: Int, val g: Int, val b: Int)
}
```

### 6.6 接口（不变）

```aura
interface Drawable {
    fun draw(): Unit
}
```

---

## 七、编译器改动清单

### 7.1 AST 层（`compiler/src/ast.rs`）

```rust
/// 新增：类修饰符枚举
pub enum ClassModifier {
    Value,    // value class：值类型
    Data,     // data class：自动生成 toString/equals/hashCode/copy
    Sealed,   // sealed class：受控继承
    Final,    // final class：不可继承
}

/// ClassDecl 新增 modifiers 字段
pub struct ClassDecl {
    // ... 原有字段不变 ...
    pub modifiers: Vec<ClassModifier>,
}

/// StructDecl 标记为 deprecated
/// 解析 "struct Foo" → ClassDecl { modifiers: [Value], ... }
```

### 7.2 Parser 层（`compiler/src/parser.rs`）

```
parse_declaration() 路由：

    "value data class"  → parse_class(modifiers = [Value, Data])
    "value class"       → parse_class(modifiers = [Value])
    "data class"        → parse_class(modifiers = [Data])
    "sealed class"      → parse_class(modifiers = [Sealed])
    "sealed value class" → parse_class(modifiers = [Sealed, Value])
    "class"             → parse_class(modifiers = [])

    "struct"            → parse_struct()  // 保留，内部转为 ClassDecl { modifiers: [Value] }

    "actor"             → parse_actor()   // 不变
```

### 7.3 Sema 层（`compiler/src/sema/checker.rs`）

新增检查规则：

| 规则 | 条件 | 动作 |
|------|------|------|
| 值类型不能继承 | `modifiers` 含 `Value` 且 `superclass.is_some()` | 报错："value class cannot have a superclass" |
| 值类型不能实现接口（带虚方法） | `modifiers` 含 `Value` 且 `implementations` 非空 | 警告："value class interfaces are non-virtual" |
| 非法修饰符组合 | `Value` + `Actor` 同时出现 | 报错："cannot combine value and actor" |

### 7.4 HIR 层（`compiler/src/codegen/hir.rs`）

```
当前：
    Decl::Struct(s) → HirStruct（值布局）
    Decl::Class(c)  → 展开方法为独立函数（类型丢弃）

新设计：
    Decl::Class(c) with modifiers containing Value → HirStruct（值布局，不变）
    Decl::Class(c) without Value modifier          → HirClass（引用类型，待实现）
    Decl::Actor(a)                                  → HirActor（并发实体，待实现）
```

### 7.5 AOT/LLVM 层（`compiler/src/codegen/aot/emit.rs`）

```
当前：
    HirStruct → %struct.Name = type {...}

新设计：
    HirStruct（来自 value class）→ %struct.Name = type {...}（不变）
    HirClass（来自 class）       → 待实现（堆对象 + vtable）
    HirActor（来自 actor）        → 待实现（并发实体运行时）
```

---

## 八、迁移路径

| 阶段 | 版本 | 改动 | 对现有代码影响 |
|------|------|------|---------------|
| **Phase 1** | v0.x | `struct` 保留，文档标注"等价于 value class" | 零破坏 |
| **Phase 2** | v0.5 | 引入 `value class` 语法，`struct` 解析为别名 | 零破坏 |
| **Phase 3** | v1.0 | `struct` 标记 deprecated，lint 警告 | 轻微（需改代码） |
| **Phase 4** | v2.0 | `struct` 移除（或保留为 `--legacy` 标志） | 需要迁移 |

### 迁移示例

```aura
// v0.x（当前）
struct Player(val id: Int, var hp: Int = 100)

// v0.5（等价，两种写法都行）
value class Player(val id: Int, var hp: Int = 100)
struct Player(val id: Int, var hp: Int = 100)  // 仍然可用

// v1.0（struct 警告）
value class Player(val id: Int, var hp: Int = 100)
struct Player(val id: Int, var hp: Int = 100)  // ⚠️ deprecated: use 'value class'

// v2.0（struct 移除）
value class Player(val id: Int, var hp: Int = 100)
```

### 自动迁移工具（可选）

```bash
# 扫描项目中所有 .aura 文件，将 struct 替换为 value class
aura migrate --rule struct-to-value-class

# 批量替换（dry run）
aura migrate --rule struct-to-value-class --dry-run
```

---

## 九、文档更新

### 9.1 语言卡片（`01-aura-language-card.md`）

**替换**第 13 行：

```
旧：与 Kotlin 不同：**`struct`** 而非 `data class`、`actor` 并发实体、`extern "c"` FFI、`Result<T,E>` 错误处理。
新：与 Kotlin 不同：**`value class`**（值类型）而非 `data class`（引用类型）、`actor` 并发实体、`extern "c"` FFI、`Result<T,E>` 错误处理。`struct` 是 `value class` 的别名（deprecated）。
```

**替换**第 367 行（常见陷阱表）：

```
| 数据结构体 | `value class Name(...)` | `data class Name(...)` |
```

**替换**第 368 行：

```
| 并发实体 | `actor Name { }` | `class Name { }` |
```

### 9.2 风格指南（`04-aura-style-guide.md`）

**新增**"类型选择"章节：

```markdown
## 类型选择

1. **默认** → `class`（最通用，OOP 基础）
2. **需要值语义** → `value class`（栈分配、值拷贝、FFI 映射 C struct）
3. **并发实体** → `actor`（消息传递、监督树）
4. **不确定** → `class`，后续按需迁移

### 决策树

    需要并发？
      ├── 是 → actor
      └── 否 → 需要值语义（栈分配、值拷贝、FFI）？
                  ├── 是 → value class
                  └── 否 → class
```

**替换**常见陷阱表（第 166 行）：

```
| 数据结构体 | `value class` | `data class` |
```

**替换**常见陷阱表（第 167 行）：

```
| 并发实体 | `actor` | `class` |
```

### 9.3 游戏领域规划（`游戏领域发展路线规划.md`）

将 `class GameLoop` 模板保持为 `class`（需要继承），`struct Player` 改为 `value class Player`。

---

## 十、与现有语言的对比

| 语言 | 值类型 | 引用类型 | 并发实体 | 默认选择 |
|------|--------|----------|----------|----------|
| **Aura（新）** | `value class` | `class` | `actor` | `class` |
| **Kotlin** | `value class` | `class` | 协程/`actor` | `class` |
| **Swift** | `struct` | `class` | `actor` | `class` |
| **Nim** | `object` | `ref object` | 运行时构造 | `object` |
| **Rust** | `struct`（所有都是值） | `Box<struct>` | `thread` | `struct` |
| **C++** | `struct`/`class`（等价） | 同左（按约定） | `std::thread` | `class` |

Aura 的定位最接近 Kotlin：`class` 默认，`value class` 特化，`actor` 并发。

---

## 十一、风险评估

| 风险 | 等级 | 缓解措施 |
|------|------|----------|
| 现有 demo 代码需迁移 | 低 | 分阶段迁移，`struct` 作为别名保留至少 2 个大版本 |
| 编译器改动范围 | 中 | 仅需 Parser + AST 改动，Sema/HIR/AOT 不变（struct 路径已存在） |
| LLM 生成错误 | 中 | 更新语言卡片，LLM 会跟随新文档 |
| 用户困惑 | 低 | 决策树清晰，`class` 为默认减少困惑 |

---

## 十二、最终语法总览

```aura
// ═══════════════════════════════════════════════════════════
// 类型声明
// ═══════════════════════════════════════════════════════════

// 引用类型（默认，堆分配、ARC、继承、多态）
class Animal {
    fun name(): String = "animal"
}
class Dog : Animal() {
    override fun name(): String = "dog"
}

// 值类型（栈内联、值拷贝、无继承、映射 C struct）
value class Vec2(val x: Float, val y: Float) {
    fun length(): Float = sqrt(x * x + y * y)
}

// 值数据类（自动生成 toString/equals/hashCode/copy）
value data class Player(val id: Int, var name: String = "unknown", var hp: Int = 100)

// 受控引用类型
sealed class Shape {
    fun area(): Float = 0.0f
}

// 受控值类型
sealed value class Direction {
    fun rotate(): Direction { /* ... */ }
}

// 并发实体（消息传递、监督树）
actor Server(config: Config) {
    var running: Boolean = false
    fun start() { running = true }
    fun stop() { running = false }
}

// 接口（不变）
interface Drawable {
    fun draw(): Unit
}

// 枚举（不变）
enum Color {
    RED,
    GREEN,
    CUSTOM(val r: Int, val g: Int, val b: Int)
}

// ═══════════════════════════════════════════════════════════
// 别名（兼容，deprecated）
// ═══════════════════════════════════════════════════════════

// struct = value class（别名，v2.0 移除）
struct Point(val x: Int, val y: Int)    // 等价于 value class Point(val x: Int, val y: Int)
```

---

## 十三、总结

| 问题 | 答案 |
|------|------|
| `struct` 保留还是移除？ | 保留为 `value class` 的别名，v2.0 移除 |
| 默认用什么？ | `class`（最通用，OOP 基础） |
| 什么时候用 `value class`？ | 需要值语义（栈分配、值拷贝、FFI 映射 C struct） |
| 什么时候用 `actor`？ | 需要并发实体（消息传递、监督树） |
| `value class` 可以继承吗？ | ❌ 不能（无 vtable） |
| `value class` 可以实现接口吗？ | ✅ 可以，但接口方法是静态派发 |
| `actor` 合并到 `class` 吗？ | ❌ 不合并（语义根本不同） |
| `struct` 和 `value class` 的区别？ | 无区别（别名） |

**一句话**：`class` 是默认，`value class` 是值特化，`actor` 是并发特化。`struct` 平滑过渡。
