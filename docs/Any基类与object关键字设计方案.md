# Aura 引入 Any 基类与 object 关键字设计方案

> **状态**：设计方案 | **优先级**：P0（语言核心） | **目标版本**：v0.5 引入 → v1.0 完善
> **分析日期**：2026-09
> **范围**：类型系统、运行时值表示、VM/JIT/AOT 三端、object 单例、性能影响
> **前置阅读**：[struct与class定位方案.md](struct与class定位方案.md)、[缺失关键字分析.md](缺失关键字分析.md)、[遗留问题与风险分析报告.md](遗留问题与风险分析报告.md)

---

## 〇、结论摘要

| 维度 | 结论 |
|------|------|
| **是否引入** | ✅ **引入 Any 基类** + **object 单例关键字** |
| **核心策略** | `Ty::Any` 既是顶级类型也是运行时基类；`object` 声明 Kotlin 风格单例 |
| **性能代价** | 堆对象 **+8 字节/对象**（对象头：type_id + vtable_idx），基本类型 **零代价** |
| **实现周期** | 分 4 阶段，约 3-4 周（含测试） |
| **风险等级** | 中（涉及 VM 堆布局、字节码格式、AOT 类型映射） |
| **阻塞项** | 必须与 `struct`→`value class` 迁移同步推进 |

**一句话**：Aura 已具备 `Ty::Any` 顶级类型和 `open`/`abstract`/`sealed`/`override` 语法，但**缺少运行时类层级**——`is MyClass` 对 class 实例永远返回 false（因 `type_name()` 返回 "Ref" 而非类名）。这是必须修复的语义缺陷，引入 Any 基类是正解。同时，`object` 关键字补齐 Kotlin 风格单例声明能力。

---

## 一、Any 基类设计

### 1.1 Any 的双重角色

`Ty::Any` 在 Aura 中扮演两个角色：

```
编译期（已存在）：
  Ty::Any = 顶级类型，任意类型可赋值给 Any

运行时（新增）：
  Any = 所有 class 的隐式基类
  ├── class Animal : Any()
  │   ├── class Dog : Animal()
  │   └── class Cat : Animal()
  └── class Shape : Any()
      ├── class Circle : Shape()
      └── class Square : Shape()

不参与类层级的类型：
  - 基本类型（Int/Long/Float/...）→ 栈内联，无对象头
  - value class / struct → 值类型，无对象头
  - List/Map/Enum → 特殊容器/枚举表示
```

### 1.2 Any 内置方法

```aura
// 语言内置，不可被用户定义或修改
// 所有 class 隐式继承自 Any

// Any 提供四个虚方法（Kotlin 对齐）：
// 1. toString(): String          — 返回对象的可读表示
// 2. equals(other: Any): Boolean — 值相等检查
// 3. hashCode(): Int              — 哈希码
// 4. typeOf(): Class              — 返回运行时类对象（反射）

// 默认实现：
// - toString() → "<ClassName@handle>"
// - equals(other) → this === other（身份相等）
// - hashCode() → identityHashCode(this)
// - typeOf() → 运行时类对象

// 语义规则：
// - class 类型隐式继承 Any（不写 : Any()）
// - Any 的所有方法都是 open 的（可重写）
// - value class / enum / 基本类型不继承 Any
// - object 单例也隐式继承 Any
```

### 1.3 与 Kotlin 的对比

| 特性 | Java | Kotlin | Aura |
|------|------|--------|------|
| 顶级类型 | `Object` | `Any` | `Any` ✅ |
| 基本类型是对象？ | ✅（int→Integer 装箱） | ✅（但可内联） | ❌（内联存储） |
| toString() | Object 虚方法 | Any 虚方法 | Any 虚方法 ✅ |
| equals/hashCode | Object 虚方法 | Any 虚方法 | Any 虚方法 ✅ |
| is instanceof | ✅ 层级遍历 | ✅ 层级遍历 | ✅ 层级遍历 |
| as 类型转换 | ✅ | ✅（as/as?） | ✅ CheckCast |
| 单例声明 | ❌（需手写） | ✅（object 关键字） | ✅（object 关键字） |
| 基本类型性能 | ❌ 装箱开销 | ⚠️ 内联优化 | ✅ 零开销 |

---

## 二、object 关键字设计（单例对象）

### 2.1 语法

```aura
// 基本单例
object Singleton {
    val x: Int = 42
    fun doSomething() { ... }
}

// 带继承的单例
object Logger : Singleton() {
    fun log(msg: String) { ... }
}

// 可被继承的单例
open object BaseLogger {
    open fun log(msg: String) { ... }
}
```

### 2.2 语义

| 特性 | 说明 |
|------|------|
| 实例数 | 全局唯一（懒初始化，首次访问时创建） |
| 实例化 | 不可手动实例化（无 `Singleton()` 语法） |
| 成员访问 | `Singleton.x`（字段）、`Singleton.doSomething()`（方法） |
| 继承 | 支持 `object : Parent()` 语法 |
| 继承开放性 | 默认 final，需 `open` 标记才可被继承 |
| 接口实现 | 支持 `object : Interface` 语法 |
| 类型引用 | `Singleton` 本身是类型，`is Singleton` 可用于检查 |
| toString/equals | 继承自 Any，可重写 |

### 2.3 使用示例

```aura
// ① 配置单例
object Config {
    val host: String = "localhost"
    val port: Int = 8080
    var debug: Boolean = false

    fun load(path: String) {
        // 从文件加载配置
        debug = true
    }
}

// 访问
Config.host          // → "localhost"
Config.debug = true  // → 修改配置
Config.toString()    // → "<Config@0x1a2b3c>"（默认实现）

// ② 日志单例（带继承）
open object Logger {
    var level: Int = 2
    open fun log(msg: String) {
        if (level <= 2) {
            println(msg)
        }
    }
}

object AppLogger : Logger() {
    override fun log(msg: String) {
        println("[APP] $msg")
    }
}

// ③ 服务单例
object Database {
    val connected: Boolean = false
    fun connect(url: String) { connected = true }
    fun query(sql: String): List<String> = listOf()
}

// ④ 类型检查
fun describe(obj: Any): String {
    when (obj) {
        is Config -> "配置对象"
        is Logger -> "日志对象"
        else -> "未知对象"
    }
}
```

### 2.4 object vs companion object

| 特性 | `object`（单例） | `companion object`（伴生对象） |
|------|------------------|-------------------------------|
| 作用域 | 顶层声明 | 类内部声明 |
| 访问方式 | `Singleton.member` | `Class.member` |
| 继承 | 可继承其他类/接口 | 不可继承 |
| 类型 | 自身就是类型 | 不是独立类型 |
| 实例 | 全局唯一 | 类级静态容器 |
| Kotlin 对齐 | ✅ `object` | ✅ `companion object` |

### 2.5 运行时实现策略

```
编译时降级：
  object Singleton { val x: Int = 42 }
    → 降级为：
      1. 生成单例构造函数（无参数，执行字段初始化 + init 块）
      2. 在模块作用域创建一个全局变量：__singleton_Singleton
      3. 懒初始化：首次访问时调用构造函数
      4. 成员访问：Singleton.x → __singleton_Singleton.x

  运行时 VM：
    - 模块状态中维护单例实例表：HashMap<String, Value>
    - 首次访问单例成员时检查是否已初始化
    - 若未初始化则创建实例并缓存

  运行时 AOT：
    - 单例实例存储在全局变量中
    - 懒初始化通过标记位控制
```

---

## 三、运行时对象布局

### 3.1 堆对象结构

```rust
// 当前设计：
Object {
    type_tag: u16,                        // FNV 哈希
    fields: HashMap<u16, Value>,          // 字段（HashMap）
    vtable: Option<HashMap<u16, usize>>,  // vtable（HashMap，每对象拷贝）
}

// 新设计：
Object {
    // ── 对象头（固定大小，8 字节）──
    type_id: u16,                         // 类 ID（模块内唯一索引，非哈希）
    vtable_idx: u16,                      // 类级 vtable 索引（引用，非拷贝）
    field_count: u16,                     // 字段数量
    _padding: u16,                        // 对齐填充
    // ── 字段（固定大小，非 HashMap）──
    fields: Vec<Value>,                   // 字段数组（按槽位索引）
}
```

**关键改进**：
1. **vtable 不再每对象拷贝**——对象持有 `vtable_idx`（引用模块级 vtable），零拷贝
2. **字段用 Vec<Value> 替代 HashMap**——固定大小数组，O(1) 按槽位索引访问
3. **type_id 是类索引而非哈希**——支持类层级遍历（`is Animal` 检查 Dog 是否继承 Animal）
4. **对象头仅 8 字节**（type_id 2 + vtable_idx 2 + field_count 2 + padding 2）

### 3.2 类 ID 系统

```rust
// 模块编译时分配类 ID（替代当前的 FNV 哈希）

// 当前：type_hash("Animal") → FNV 哈希（u16）
// 问题：FNV 哈希可能碰撞，且无法表达层级关系

// 新设计：编译时分配递增类 ID
// module.classes: Vec<ClassDef>  // 类定义表
// class_id: u16                  // 索引到 classes 表

// 继承链查询：
// class_def[parent_id]  // 父类 ID
// class_def.interfaces[] // 接口列表
// is_subclass(child_id, ancestor_id)  // 沿 parent_idx 链向上查找
```

### 3.3 Any 类定义

```rust
// 内置类定义（模块加载时自动注册）：
ClassDef {
    type_id: 0,           // Any 是第一个类
    name: "Any",
    parent_id: 0xFFFF,    // 无父类（顶级）
    field_count: 0,
    vtable_idx: 0,        // Any 的 vtable
    interfaces: vec![],
    is_builtin: true,
}
```

### 3.4 object 单例类定义

```rust
// object 单例在类定义表中的表示：
ClassDef {
    type_id: N,
    name: "Singleton",
    parent_id: Any_id,    // 继承自 Any
    field_count: 3,       // 字段数量
    vtable_idx: N,
    interfaces: vec![],
    is_singleton: true,   // 标记为单例
    singleton_instance: Some(handle),  // 单例实例句柄
}
```

---

## 四、`is` / `as` 语义定义

### 4.1 is 检查规则

```
1. 堆对象：读取 type_id → 沿继承链向上查找目标 type_id
   - 命中 → true
   - 未命中 → 检查是否实现了目标接口
   - 都未命中 → false

2. 基本类型：通过 Value tag 匹配
   - Value::Int → is Int ✅
   - Value::Int → is String ❌
   - Value::Int → is Any ✅（Any 是顶级类型，所有值都是 Any）

3. null：
   - null is Any → true
   - null is Any? → true
   - null is Int → false（除非 Int? 等可空类型）
```

### 4.2 as 转换规则

```
1. as T（严格）：若 is T 为 true → 返回 obj，否则 → 抛异常
2. as? T（安全）：若 is T 为 true → 返回 obj，否则 → 返回 null
3. as 到基本类型：类型不匹配时返回 0/null（与 as_int() 风格一致）
```

---

## 五、性能分析

### 5.1 堆对象内存开销

| 组件 | 当前 | 新设计 | 增量 |
|------|------|--------|------|
| type_tag | 2 字节 | type_id (2 字节) | 0 |
| vtable 存储 | HashMap（~100-200 字节/对象） | vtable_idx (2 字节) | **-98 到 -198 字节** |
| fields | HashMap（~64 字节 + N×32 字节） | field_count (2) + Vec (24 + N×16) | **-40 到 -16 字节/字段** |
| **总计** | ~164 + N×32 字节 | ~30 + N×16 字节 | **-134 - N×16 字节** |

**结论**：新设计**减少**了每对象内存开销，主要因为 vtable 不再每对象拷贝、字段用 Vec 替代 HashMap。

### 5.2 综合性能预估

| 场景 | 预估变化 |
|------|----------|
| 纯基本类型程序 | **零变化**（基本类型不分配堆对象） |
| 含 class 的多态程序 | **性能提升 2-5 倍**（字段访问+vtable 分派优化） |
| 含 `is`/`as` 的程序 | **从不可用变为可用**（性能略增但语义正确） |
| 大型堆对象程序（1000+ 对象） | **内存减少 40-60%**（vtable 不拷贝+字段用 Vec） |
| object 单例程序 | **零额外开销**（单例实例在模块初始化时创建一次） |

---

## 六、实施计划

### Phase 1: 基础设施 + object 关键字（当前阶段）

```
┌──────────────────────────────────────────────────────────────────┐
│ Phase 1: 基础设施（1-2 周）                                        │
│                                                                   │
│ 1. 类 ID 系统：替代 FNV 哈希，编译时分配递增 ID                      │
│ 2. 类定义表：module.classes: Vec<ClassDef>（含 parent_id）          │
│ 3. 字段槽位分配：编译期为每个类分配字段槽位索引                       │
│ 4. 模块序列化：.auc v6 格式（含类定义表）                           │
│ 5. Any 内置类：注册为第一个类（type_id=0）                          │
│ 6. object 关键字：AST + Parser + Sema + HIR                       │
│ 7. object 降级：降级为类 + 单例实例                                  │
├──────────────────────────────────────────────────────────────────┤
│ Phase 2: 运行时改造（1 周）                                        │
│                                                                   │
│ • HeapData::Object 重构：vtable_idx + Vec<Value> 字段             │
│ • 新增 InstanceOf / CheckCast 字节码指令                          │
│ • VM 实现：is/as 支持堆对象类层级                                  │
│ • JIT 实现：InstanceOf / CheckCast 指令                            │
│ • aura_isOfType 改造：支持堆对象类层级                             │
│ • object 单例运行时：模块级单例实例表                               │
├──────────────────────────────────────────────────────────────────┤
│ Phase 3: AOT 改造（1 周）                                          │
│                                                                   │
│ • 对象头布局：%struct.T = { i32 type_id, i32 vtable_idx, ... }   │
│ • AOT InstanceOf / CheckCast 生成                                  │
│ • AOT vtable 间接调用                                              │
│ • AOT toString 虚分派                                              │
│ • AOT object 单例全局变量                                          │
├──────────────────────────────────────────────────────────────────┤
│ Phase 4: 标准库 + 文档 + 测试（0.5-1 周）                          │
│                                                                   │
│ • Any 基类内置方法：toString / equals / hashCode / typeOf          │
│ • String + T 降级为 toString 虚调用                               │
│ • sealed class when 穷举检查（修复遗留问题 #6）                    │
│ • 更新 book/chapter-03.md（类型系统文档）                          │
│ • 更新 examples/classes/*.aura（验证 is/as/object）               │
│ • 回归测试：确保现有示例仍通过                                      │
└──────────────────────────────────────────────────────────────────┘
```

### 6.1 Phase 1 详细任务

| # | 任务 | 文件 | 预估改动 |
|---|------|------|----------|
| 1 | AST: 添加 Decl::Object + ObjectDecl | `ast.rs` | +30 行 |
| 2 | Parser: 添加 parse_object() | `parser.rs` | +50 行 |
| 3 | Sema: 处理 Decl::Object | `checker.rs` | +40 行 |
| 4 | HIR: 注册 object 到 CLASS_TABLE | `hir.rs` | +30 行 |
| 5 | opcode.rs: 添加 ClassDef 结构 | `opcode.rs` | +30 行 |
| 6 | emit.rs: 替代 FNV 哈希为顺序类 ID | `emit.rs` | +80 行 |
| 7 | serialize.rs: .auc v6 格式 | `serialize.rs` | +60 行 |
| 8 | Any 内置类注册 | `checker.rs`/`emit.rs` | +20 行 |
| 9 | object 降级为类 + 单例实例 | `hir.rs`/`emit.rs` | +80 行 |

---

## 七、风险评估

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| .auc 格式变更导致旧文件不可读 | 中 | 高 | v6 格式设计为向后兼容读取 v5 |
| HeapData 重构导致内存泄漏 | 低 | 高 | 分阶段重构，ARC 分析器同步更新 |
| AOT 对象头导致性能回退 | 中 | 中 | 仅堆对象受影响，基本类型零代价 |
| object 单例初始化顺序 | 中 | 中 | 懒初始化 + 标记位控制 |
| 跨模块类 ID 冲突 | 中 | 中 | Phase 1 不支持跨模块继承，延后到 v1.0 |

---

## 八、决策记录

### 8.1 为什么用 Any 而不是 Object

| 考虑 | 理由 |
|------|------|
| **Kotlin 对齐** | Kotlin 用 `Any` 作为顶级类型，Aura 语法对齐 Kotlin |
| **已存在** | `Ty::Any` 已存在于类型系统中，无需新增类型 |
| **语义一致** | `Any` 表示"任意类型"，作为基类语义自然 |
| **避免混淆** | 如果叫 `Object`，会与 `object` 关键字混淆（`object` 是小写单例声明） |
| **C# 参考** | C# 的顶级类型是 `object`，但 Kotlin/Swift/Scala 都用 `Any` |

### 8.2 object 关键字设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 单例实例化 | 懒初始化（首次访问时创建） | Kotlin 风格，避免初始化顺序问题 |
| 成员访问 | `Singleton.member` | 与 companion object 一致 |
| 继承支持 | 支持 `object : Parent()` | 与 class 一致，增加灵活性 |
| 类型检查 | 支持 `is Singleton` | 单例也是类型，可参与多态 |
| 与 companion object 区分 | 独立关键字 | Kotlin 风格，语义清晰 |

---

## 附录 A：核心代码位置索引

| 功能 | 文件 | 行号 |
|------|------|------|
| Ty::Any 定义 | `compiler/src/sema/ty.rs` | 28-29 |
| can_assign_to 顶级类型检查 | `compiler/src/sema/ty.rs` | 191-196 |
| Value 枚举 | `compiler/src/vm/value.rs` | 14-37 |
| Value::type_name() | `compiler/src/vm/value.rs` | 117-130 |
| HeapData::Object | `compiler/src/vm/heap.rs` | 20-30 |
| alloc_object_with_vtable | `compiler/src/vm/heap.rs` | 138-149 |
| NewObject 指令（vtable 拷贝） | `compiler/src/vm/interp.rs` | 132-150 |
| CallMethod 虚分派 | `compiler/src/vm/interp.rs` | 192-197 |
| aura_isOfType | `compiler/src/vm/native.rs` | 372-384 |
| vtable 构建（emit） | `compiler/src/codegen/emit.rs` | 67-105 |
| type_index（FNV 哈希） | `compiler/src/codegen/emit.rs` | 679 |
| field_index（FNV 哈希） | `compiler/src/codegen/emit.rs` | 689 |
| JIT JitValue | `compiler/src/vm/abi.rs` | 13-37 |
| AOT 类型映射 | `compiler/src/codegen/aot/types.rs` | 33-99 |
| AOT aura_isOfType 编译期解析 | `compiler/src/codegen/aot/emit.rs` | 1820-1853 |
| is 表达式降级 | `compiler/src/codegen/hir.rs` | 3201-3210 |
| pattern_to_expr（is 模式） | `compiler/src/parser.rs` | 2992-2999 |
| 继承开放性检查 | `compiler/src/sema/checker.rs` | 831-847 |
| override 一致性检查 | `compiler/src/sema/checker.rs` | 848-896 |
| value class 不可继承 | `compiler/src/sema/checker.rs` | 563-571 |
| object 关键字（词法） | `compiler/src/lexer.rs` | 56 |
| companion object 解析 | `compiler/src/parser.rs` | 932-959 |
| companion 成员降级 | `compiler/src/codegen/hir.rs` | 1266-1290 |
| .auc 序列化版本 | `compiler/src/codegen/serialize.rs` | 35 |

---

## 附录 B：测试用例设计

```aura
// tests/any_object_test.aura

// ① Any 顶级类型
fun test_any_top() {
    val a: Any = 1          // Int → Any
    val b: Any = "hello"    // String → Any
    val c: Any = true       // Boolean → Any
    val d: Any = null       // Null → Any
    assert(a is Int)
    assert(b is String)
    assert(c is Boolean)
    assert(d is Any)
}

// ② object 单例
object Config {
    val host: String = "localhost"
    val port: Int = 8080
    var debug: Boolean = false
    init { debug = true }
}

fun test_object_singleton() {
    assert(Config.host == "localhost")
    assert(Config.port == 8080)
    assert(Config.debug == true)      // init 块执行
    Config.debug = false
    assert(Config.debug == false)     // 可修改
    Config.debug = true               // 恢复
}

// ③ object 单例是类型
fun test_object_type() {
    val c: Any = Config
    assert(c is Config)
    assert(c is Any)
}

// ④ object 继承
open object BaseLogger {
    var level: Int = 2
    open fun log(msg: String) {
        if (level <= 2) { println(msg) }
    }
}

object AppLogger : BaseLogger() {
    override fun log(msg: String) {
        println("[APP] $msg")
    }
}

fun test_object_inheritance() {
    val l: Any = AppLogger
    assert(l is AppLogger)
    assert(l is BaseLogger)
}

// ⑤ object 与 companion object 共存
class MathUtil {
    companion object {
        val PI: Float = 3.14f
        fun max(a: Int, b: Int): Int = if (a > b) a else b
    }
}

fun test_companion_still_works() {
    assert(MathUtil.PI == 3.14f)
    assert(MathUtil.max(3, 7) == 7)
}
```
