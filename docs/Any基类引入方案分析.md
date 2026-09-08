# Aura 引入 Any 基类设计方案分析

> ⚠️ **已废弃（SUPERSEDED）**：本分析文档已被 [Any基类与object关键字设计方案.md](Any基类与object关键字设计方案.md) 取代。
> **最终决策**：基类统一使用 **`Any`**（不引入 `Object` 类的实现），同时引入 `object` 单例关键字。
> 本文档保留作为历史分析与技术参考（性能评估、风险评估等内容仍有价值），但设计方案以 `Any基类与object关键字设计方案.md` 为准。

> **原始状态**：设计分析 | **优先级**：P0（语言核心） | **建议版本**：v0.5 引入 → v1.0 完善
> **分析日期**：2026-09
> **范围**：类型系统、运行时值表示、VM/JIT/AOT 三端、内存管理、性能影响
> **前置阅读**：[struct与class定位方案.md](struct与class定位方案.md)、[缺失关键字分析.md](缺失关键字分析.md)、[遗留问题与风险分析报告.md](遗留问题与风险分析报告.md)

---

## 〇、结论摘要

| 维度 | 结论 |
|------|------|
| **是否引入** | ✅ **建议引入**，但采用**分层 Any 模型**（非 Java 式全对象化） |
| **核心策略** | 保留 `Ty::Any` 作为顶级类型（已存在），为堆对象引入**运行时类层级**（class hierarchy），**保留 value class 不参与继承** |
| **性能代价** | 堆对象 **+16 字节/对象**（对象头：vtable 指针 + type_id），基本类型 **零代价** |
| **实现周期** | 分 4 阶段，约 3-4 周（含测试） |
| **风险等级** | 中（涉及 VM 堆布局、字节码格式、AOT 类型映射） |
| **阻塞项** | 必须与 `struct`→`value class` 迁移同步推进，否则两套类型系统共存会产生歧义 |

**一句话**：Aura 已具备 `Ty::Any` 顶级类型和 `open`/`abstract`/`sealed`/`override` 语法，但**缺少运行时类层级**——`is MyClass` 对 class 实例永远返回 false（因 `type_name()` 返回 "Ref" 而非类名）。这是必须修复的语义缺陷，引入 Any 基类是正解。

---

## 一、现状分析

### 1.1 当前类型体系

#### 语义类型（`compiler/src/sema/ty.rs`）

```rust
pub enum Ty {
    // 基本类型
    Int, Long, Short, Byte, Float, Double, Boolean, Char, String,
    // 顶级类型
    Any,        // ← 已存在！顶级类型
    Nothing,   // ← 已存在！底部类型
    Unit,
    // 复合类型
    Nullable(Box<Ty>), Pointer(Box<Ty>), Array(Box<Ty>),
    List(Box<Ty>), Map(Box<Ty>, Box<Ty>),
    // 命名类型（class/struct/interface/enum）
    Named(String),
    // 其他
    Function { params: Vec<Ty>, ret: Box<Ty> },
    TypeVar(u32), Error,
}
```

**关键发现**：`Ty::Any` **已存在**且 `can_assign_to` 已实现"任意类型可赋值给 Any"的语义：

```rust
// ty.rs:191-196
pub fn can_assign_to(&self, target: &Ty) -> bool {
    if self == target { return true; }
    if target == &Ty::Any { return true; }  // ← Any 是顶级类型
    ...
}
```

#### 运行时值（`compiler/src/vm/value.rs`）

```rust
pub enum Value {
    Int(i64),           // 内联存储
    Float(f64),         // 内联存储
    Bool(bool),         // 内联存储
    Str(Rc<str>),       // 引用计数共享
    Null,               // 空值
    Ref(usize),         // 堆对象句柄 ← 无类型信息！
    Weak(usize),        // 弱引用
    Ptr(i64),           // 原始指针（FFI）
    List(Vec<Value>),   // 列表
    Map(HashMap<Value, Value>), // 映射
}
```

**关键问题**：`Value::Ref(usize)` 是一个**无类型信息的堆句柄**。`type_name()` 对所有堆对象返回 `"Ref"`：

```rust
// value.rs:117-130
pub fn type_name(&self) -> &'static str {
    match self {
        Value::Int(_) => "Int",
        Value::Float(_) => "Float",
        // ...
        Value::Ref(_) => "Ref",      // ← 永远返回 "Ref"，不是类名！
        Value::Weak(_) => "Weak",
        // ...
    }
}
```

#### 堆对象布局（`compiler/src/vm/heap.rs`）

```rust
pub enum HeapData {
    Object {
        type_tag: u16,                              // FNV 哈希，无层级信息
        fields: HashMap<u16, Value>,                // ← 字段用 HashMap！
        vtable: Option<HashMap<u16, usize>>,        // ← vtable 也是 HashMap！
    },
    Array(Vec<Value>),
    List(Vec<Value>),
    Map(HashMap<Value, Value>),
    Closure { ... },
    Enum(u16),
    FnRef(usize),
}
```

**三个严重性能问题**：

1. **字段是 `HashMap<u16, Value>`**：每次字段访问需哈希计算 + 动态查找，且每对象独立分配
2. **vtable 是 `HashMap<u16, usize>` 且每对象拷贝一份**：分配时从类级 vtable 复制到对象级 HashMap（`interp.rs:135-145`），O(methods) 内存和分配开销
3. **`type_tag` 是 FNV 哈希**：只能做精确匹配，无法做继承检查

#### JIT 值表示（`compiler/src/vm/abi.rs`）

```rust
pub struct JitValue {
    pub tag: i64,      // 类型标签：TAG_INT=0, TAG_FLOAT=1, TAG_OBJ=6, ...
    pub payload: i64,  // 值载荷
}
```

JIT 使用**扁平标签**（`TAG_OBJ = 6` 表示所有堆对象），同样**无类层级信息**。

#### AOT 值表示（`compiler/src/codegen/aot/types.rs`）

```rust
"Any" => "i8*".to_string(),      // ← Any 是裸指针，无类型信息
"Int" => "i32",                  // 基本类型直接映射
"Player" => "%struct.Player",    // 用户类是不透明结构体，无类型头
```

AOT 中**完全没有运行时类型标签**——`aura_isOfType` 在 AOT 中是编译期解析的（`aot/emit.rs:1821`），无法处理动态分派场景。

### 1.2 继承机制现状

#### 语法层（已实现）

- `open`/`abstract`/`sealed` 修饰符 ✅
- `class Dog : Animal()` 语法 ✅
- `override` 关键字 ✅
- 构造函数委托 `super(...)` / `this(...)` ✅
- `init` 块 ✅
- companion object ✅

#### 语义层（部分实现）

- 继承开放性检查（非 open 类不可继承）✅
- override-open 合法性检查 ✅
- abstract 方法校验 ✅
- 接口实现完整性检查 ✅
- **sealed 类型追踪** ✅
- **when 穷举性检查仅支持 enum** ⚠️（遗留问题 #6）

#### 运行时层（部分实现）

- 类方法降级为 `Class.method(self, ...)` ✅
- **vtable 构建**：emit 为每个类构建 `type_tag → slot → func_idx` vtable ✅
- **CallMethod 虚分派** ✅
- **vtable 槽位全局编号**（所有 open/abstract 方法共享槽位池）✅
- **vtable 沿继承链查找**（子类 vtable 从父类继承未重写的槽位）✅
- **`is` 检查**：❌ 字符串比较，对 class 实例永远返回 false
- **`as` 转换**：❌ 未实现
- **多态字段访问**：⚠️ 按静态类型分派（`缺失关键字分析.md` 提到的"调用点按接收者静态类型分派"），非虚方法不走 vtable

#### 证据：`class_runtime.aura` 示例

```aura
open class Animal {
    open fun name(): String { return "animal" }
}
class Dog : Animal() {
    override fun name(): String { return "dog" }
}

fun main() {
    val a: Animal = Dog()  // ← 多态赋值
    println(a.name())      // ← 调用 Animal.name()，但通过 vtable 分派到 Dog.name()
}
// 输出: "dog" ✅ 多态生效
```

**但无法做类型检查**：

```aura
val x: Any = Dog()
when (x) {
    is Dog -> ...       // ← aura_isOfType(x, "Dog") → x.type_name() == "Ref" ≠ "Dog" → false！
    is Animal -> ...    // ← 同样返回 false
    else -> ...         // ← 永远走 else 分支
}
```

### 1.3 核心缺陷总结

| 缺陷 | 严重程度 | 根因 | 影响 |
|------|----------|------|------|
| `is MyClass` 对 class 实例永远返回 false | 🔴 致命 | `type_name()` 返回 "Ref" 而非类名；无类型层级 | 运行时类型检查完全不可用 |
| `as` 类型转换未实现 | 🔴 致命 | 无类型层级，无法安全转换 | 多态代码无法安全类型收窄 |
| AOT 无运行时类型信息 | 🔴 致命 | `Any → i8*`，用户类无对象头 | AOT 模式下 `is`/`as` 完全不可用 |
| 字段用 HashMap 存储 | 🟡 性能 | `HashMap<u16, Value>` | 字段访问 O(1) 但常数极大，每对象分配 |
| vtable 每对象拷贝 | 🟡 性能 | `HashMap<u16, usize>` per object | 内存浪费，分配慢 |
| toString 无统一 API | 🟡 语义 | 无 Any 基类，无虚 toString | String+T 需隐式 toString 插入（已实现但不优雅） |
| equals/hashCode 无统一语义 | 🟢 语义 | `PartialEq` 直接比较 | 无法实现值相等 vs 身份相等的区分 |
| sealed class when 穷举无效 | 🟡 语义 | 仅支持 enum 穷举检查（遗留问题 #6） | sealed class 的多态穷举不安全 |

---

## 二、设计选项分析

### 选项 A：Java 式全对象化（❌ 不推荐）

**设计**：所有类型（包括 Int、String、Boolean）都是对象，继承自 Any，每个值都有对象头。

```
Any (基类)
├── Int (包装 32 位整数)
├── String (堆上字符串)
├── Boolean (包装 1 位)
├── Float (包装 32 位浮点)
├── Double (包装 64 位浮点)
└── MyClass (用户类)
```

| 维度 | 评估 |
|------|------|
| **语义** | ✅ 统一，所有值都有 toString/equals/hashCode |
| **性能** | ❌ 灾难性——每个 Int 都要堆分配对象头（+16 字节），基本类型操作从寄存器级退化为堆指针解引用 |
| **内存** | ❌ 所有 Int 操作都需要堆分配，ARC 引用计数翻倍 |
| **AOT** | ❌ LLVM 无法将 `i32` 映射为带对象头的结构体而不影响性能 |
| **value class** | ❌ 与 `value class`（值类型/栈类型）设计根本冲突 |
| **NovaOS 场景** | ❌ 系统级脚本语言要求基本类型高性能，全对象化违背设计目标 |
| **实现复杂度** | ❌ 需要重写 Value 枚举、Heap 布局、JIT 值表示、AOT 类型映射、ARC 分析器 |
| **参考** | Java（JVM 对象化）、JavaScript V8（Smi 优化例外） |

**结论**：❌ 与 Aura 的系统级性能目标和 value class 设计冲突，不可取。

---

### 选项 B：分层 Any 模型（✅ 推荐）

**设计**：区分**堆对象**（参与类层级）和**基本类型/值类型**（不参与类层级），但都可以通过 `Any` 类型统一访问。

```
┌─────────────────────────────────────────────────────────────┐
│                    Ty::Any (顶级类型)                         │
│                can_assign_to: 任意类型 → Any                  │
├────────────────────────────┬────────────────────────────────┤
│   堆对象（参与类层级）       │    基本类型/值类型（不参与层级）   │
│                            │                                │
│   Any (运行时基类)       │    Int / Long / Float / ...    │
│   ├── class Animal         │    String (Rc<str>, 共享)       │
│   │   ├── class Dog        │    value class Vec2             │
│   │   └── class Cat        │    value class Color            │
│   └── class Shape          │    enum Color                   │
│       ├── class Circle     │    List / Map (堆但非 Any)   │
│       └── class Square     │                                │
│                            │                                │
│   运行时特征：              │    运行时特征：                  │
│   - 对象头（vtable+type_id）│    - 内联存储（无堆分配）        │
│   - 虚方法分派              │    - 值语义（拷贝 = 值拷贝）     │
│   - is/as 支持              │    - is 仅限基本类型标签          │
│   - toString/equals 虚方法  │    - toString 为原生函数         │
└────────────────────────────┴────────────────────────────────┘
```

| 维度 | 评估 |
|------|------|
| **语义** | ✅ 堆对象有完整类层级，基本类型保持高效 |
| **性能** | ✅ 基本类型零代价（内联），堆对象 +16 字节对象头 |
| **内存** | ✅ 基本类型不分配堆，仅堆对象分配 |
| **AOT** | ⚠️ 需要为堆对象引入对象头（`{i8* vtable, i32 type_id, ...}`），但基本类型不受影响 |
| **value class** | ✅ 完全兼容——value class 是值类型，不参与类层级 |
| **NovaOS 场景** | ✅ 系统级性能优先，基本类型保持高性能 |
| **实现复杂度** | ⚠️ 中等——需修改 Heap 布局、Value 表示、字节码（新增 InstanceOf 指令）、AOT 类型映射 |
| **参考** | C#（引用类型 vs 值类型）、Go（interface{} 含 itab+data）、Kotlin（Any 顶层但不全对象化） |

**结论**：✅ **推荐**，兼顾语义完整性和性能。

---

### 选项 C：最小 Any（仅 toString/equals，无层级）（❌ 不推荐）

**设计**：引入 Any 基类但只包含 `toString()`/`equals()`，不做类层级，`is` 继续用字符串比较。

| 维度 | 评估 |
|------|------|
| **语义** | ⚠️ 部分修复——toString 统一了，但 `is MyClass` 仍然对 class 实例返回 false |
| **性能** | ✅ 基本类型零代价 |
| **实现复杂度** | ✅ 低——只需添加原生函数 |
| **核心缺陷** | ❌ **不解决** `is`/`as` 对 class 实例返回 false 的致命缺陷 |
| **参考** | 无（这不是任何成熟语言的设计） |

**结论**：❌ 不解决核心问题，仅是表面修复。

---

### 选项 D：维持现状（❌ 不推荐）

**设计**：保持当前实现，不引入 Any 基类。

| 维度 | 评估 |
|------|------|
| **语义** | ❌ `is`/`as` 对 class 实例不可用 |
| **实现成本** | ✅ 零 |
| **未来影响** | ❌ 随着类系统完善，缺乏运行时类层级会成为系统性障碍 |
| **参考** | 当前状态 |

**结论**：❌ 短期省事，长期代价更大。随着 `class` 类型完善和 sealed class 使用增加，缺乏类层级会成为越来越大的负担。

---

## 三、推荐方案详细设计（选项 B）

### 3.1 类型层级模型

```
编译期类型格（Ty lattice）：
    Any              ← 顶级类型（已存在）
    ├── Int          ← 基本类型
    ├── String       ← 基本类型
    ├── Boolean      ← 基本类型
    ├── ...          ← 其他基本类型
    └── Named("Animal")  ← 命名类型（class/struct/interface）
        └── Named("Dog")      ← 子类（通过 superclass 链接）

运行时类层级（仅堆对象）：
    Any (隐式基类，运行时存在)
    ├── Animal (class)
    │   ├── Dog (class)
    │   └── Cat (class)
    └── Shape (sealed class)
        ├── Circle (class)
        └── Square (class)

不参与类层级的类型：
    - 基本类型（Int/Long/Float/...）→ 栈内联，无对象头
    - value class → 值类型，无对象头
    - List/Map → 堆但非 Any（特殊容器类型）
    - enum → 特殊表示（变体标签）
```

### 3.2 Any 基类定义

```aura
// 语言内置，不可被用户定义或修改
// 所有 class 隐式继承自 Any

// 抽象基类 Any 提供：
// 1. toString(): String          — 返回对象的可读表示
// 2. equals(other: Any): Boolean — 值相等检查
// 3. hashCode(): Int              — 哈希码
// 4. typeOf(): Class              — 返回运行时类对象（反射）

// 语义规则：
// - class 类型隐式继承 Any（不写 : Any()）
// - Any 的所有方法都是 open 的（可重写）
// - Any.toString() 默认返回 "<ClassName@handle>"
// - Any.equals() 默认是身份相等（=== 等价）
// - Any.hashCode() 默认基于身份
// - value class / enum / 基本类型不继承 Any
```

### 3.3 运行时对象布局

#### VM 堆对象（修改 `HeapData::Object`）

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
    vtable_idx: u16,                      // 类级 vtable 索引（非拷贝，引用）
    // ── 字段（固定大小，非 HashMap）──
    field_count: u16,                     // 字段数量
    fields: Vec<Value>,                   // 字段数组（按槽位索引）
    // ── 元数据 ──
    // vtable 在模块级存储，对象仅持有索引
}
```

**关键改进**：
1. **vtable 不再每对象拷贝**——对象持有 `vtable_idx`（引用模块级 vtable），零拷贝
2. **字段用 Vec<Value> 替代 HashMap**——固定大小数组，O(1) 按槽位索引访问，无哈希开销
3. **type_id 是类索引而非哈希**——支持类层级遍历（`is Animal` 检查 Dog 是否继承 Animal）
4. **对象头仅 8 字节**（type_id 2 + vtable_idx 2 + field_count 2 + padding 2）

#### JIT 值表示

```rust
// JitValue 保持不变（tag + payload 结构已足够）
// TAG_OBJ = 6 表示堆对象，payload 是堆句柄
// 新增：TAG_CLASS = 13（Class 对象，反射用）

// 对象头在堆中，JitValue 只持有堆句柄
// InstanceOf 指令通过堆句柄读取对象头 type_id 判断
```

#### AOT 值表示

```llvm
; 当前：Any → i8*，用户类 → %struct.Name（不透明结构体）
; 新设计：

; 堆对象 = 带对象头的结构体
%struct.Animal = type {
    i32,         ; type_id（运行时类型标签）
    i32,         ; vtable_idx（虚方法表索引）
    ...fields... ; 字段（内联，非 HashMap）
}

; Any → ptr（不透明指针，指向带对象头的堆对象）
; Animal → %struct.Animal（带对象头的结构体）
; Dog → %struct.Dog（继承 Animal 的字段 + 自身字段）

; InstanceOf 在 AOT 中变为：读取对象的 type_id，沿继承链检查
```

### 3.4 新增字节码指令

```rust
// opcode.rs 新增指令

// InstanceOf(type_id): 栈顶引用 → Boolean（是否为指定类型或子类）
// 语义：读取对象头的 type_id，沿继承链检查是否匹配
// 操作数：u16 type_id
// 栈效果：pop(obj) → push(Boolean)
// 基本类型：通过 tag 匹配（Int → is Int, String → is String）

// CheckCast(type_id): 栈顶引用 → 引用（若匹配则返回，否则返回 Null）
// 语义：安全类型转换，失败返回 Null 而非抛异常（与 Kotlin as? 一致）
// 操作数：u16 type_id
// 栈效果：pop(obj) → push(obj or Null)

// CheckCastStrict(type_id): 栈顶引用 → 引用（若匹配则返回，否则抛异常）
// 语义：严格类型转换（Kotlin as）
// 操作数：u16 type_id
// 栈效果：pop(obj) → push(obj or throw)
```

### 3.5 类 ID 系统

```rust
// 模块编译时分配类 ID（替代当前的 FNV 哈希）

// 当前：type_hash("Animal") → FNV 哈希（u16）
// 问题：FNV 哈希可能碰撞，且无法表达层级关系

// 新设计：编译时分配递增类 ID
// module.classes: Vec<ClassDef>  // 类定义表
// class_id: u16                  // 索引到 classes 表

// 继承链查询：
// class_def[parent_idx]  // 父类 ID
// class_def.interfaces[] // 接口列表
// is_subclass(child_id, ancestor_id)  // 沿 parent_idx 链向上查找
```

### 3.6 `is` / `as` 语义定义

```
is 检查规则：
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

as 转换规则：
1. as T（严格）：若 is T 为 true → 返回 obj，否则 → 抛异常
2. as? T（安全）：若 is T 为 true → 返回 obj，否则 → 返回 null
3. as 到基本类型：类型不匹配时返回 0/null（与 as_int() 风格一致）
```

### 3.7 sealed class 穷举性

引入 Any 基类后，sealed class 的 when 穷举检查可以正常工作：

```aura
sealed class Shape {
    fun area(): Float = 0.0f
}
class Circle : Shape() {
    override fun area(): Float = 3.14f * r * r
}
class Square : Shape() {
    override fun area(): Float = side * side
}

// when 穷举检查：编译器知道 Shape 只有 Circle 和 Square 两个子类
fun computeArea(s: Shape): Float {
    when (s) {
        is Circle -> s.area()
        is Square -> s.area()
        // 不需要 else 分支——编译器验证穷举性 ✅
    }
}
```

### 3.8 toString 统一

```
当前：String + T 需要隐式 toString 插入（已实现，docs/toString-隐式拼接优化方案.md）
新设计：Any.toString() 是虚方法，String + T 直接调用 T.toString()

// Any.toString() 默认实现：
//   返回 "<ClassName@handle>"（如 "<Dog@0x1a2b3c>"）
// 用户可重写：
//   class Dog : Animal() {
//       override fun toString(): String = "Dog(name=$name, age=$age)"
//   }

// String + T 降级：
//   "prefix: " + anyValue
//   → aura_string_concat("prefix: ", anyValue.toString())
//   → 对 Any 类型，toString() 是虚分派（通过 vtable）
```

### 3.9 equals / hashCode

```
Any.equals(other: Any): Boolean = this === other  // 身份相等
Any.hashCode(): Int = identityHashCode(this)      // 基于身份的哈希

// 用户可重写：
// class Vec2 {
//     val x: Float
//     val y: Float
//     override fun equals(other: Any): Boolean {
//         if (other is !Vec2) return false
//         return x == other.x && y == other.y
//     }
//     override fun hashCode(): Int {
//         return x.hashCode() * 31 + y.hashCode()
//     }
// }

// 注意：value class 的 equals 是值相等（编译期生成），
//       与 Any.equals 的虚分派是不同的机制。
```

---

## 四、性能分析

### 4.1 堆对象内存开销

| 组件 | 当前 | 新设计 | 增量 |
|------|------|--------|------|
| type_tag | 2 字节 | type_id (2 字节) | 0 |
| vtable 存储 | HashMap<u16, usize>（~100-200 字节/对象） | vtable_idx (2 字节) | **-98 到 -198 字节** |
| fields | HashMap<u16, Value>（~64 字节 + N*32 字节） | field_count (2) + Vec<Value> (24 + N*16) | **-40 到 -16 字节/字段** |
| **总计** | ~164 + N*32 字节 | ~30 + N*16 字节 | **-134 - N*16 字节** |

**结论**：新设计**减少**了每对象内存开销，主要因为 vtable 不再每对象拷贝、字段用 Vec 替代 HashMap。

### 4.2 字段访问性能

| 操作 | 当前 | 新设计 |
|------|------|--------|
| 读字段 | `HashMap.get(hash)` → O(1) 均摊，~100ns（含哈希计算+可能的探测） | `Vec[槽位索引]` → O(1)，~5ns（数组直接索引） |
| 写字段 | `HashMap.insert(hash, val)` → O(1) 均摊，~150ns | `Vec[槽位索引] = val` → O(1)，~10ns |
| 首次分配 | 分配 HashMap 内部结构 | 分配 Vec（容量预计算） |

**结论**：字段访问性能提升约 **10-20 倍**。

### 4.3 vtable 分派性能

| 操作 | 当前 | 新设计 |
|------|------|--------|
| 方法调用 | 从对象 vtable HashMap 查方法 → ~100ns | 从对象头读 vtable_idx → 查模块级 vtable Vec → ~10ns |
| 对象分配 | 拷贝类级 vtable 到对象 HashMap → ~500ns（含分配） | 设置 vtable_idx → ~5ns |

**结论**：方法分派性能提升约 **10 倍**，对象分配性能提升约 **100 倍**。

### 4.4 `is` 检查性能

| 场景 | 当前 | 新设计 |
|------|------|--------|
| `is Int`（基本类型） | `type_name() == "Int"` → ~5ns | `tag == TAG_INT` → ~2ns |
| `is MyClass`（堆对象，自身类） | `type_name() == "Ref"` → false（~5ns） | `type_id == target` → true（~5ns） |
| `is Animal`（堆对象，父类） | false（~5ns） | 沿继承链向上查找 → ~20ns（链深度×5ns） |
| `is Any`（顶级类型） | true（所有值都是 Any） | true（所有值都是 Any） |

**结论**：基本类型 `is` 略快，堆对象 `is` 从"永远 false"变为"正确判断"（性能略增但语义正确）。

### 4.5 AOT 性能影响

| 场景 | 当前 | 新设计 |
|------|------|--------|
| 基本类型操作 | 寄存器级，零开销 | 不变（基本类型仍直接映射为 LLVM 标量） |
| 堆对象字段访问 | `%struct.Player` 直接 load/store | 对象头偏移 + 字段偏移（+2-3 条指令） |
| 堆对象方法调用 | 无（AOT 不支持虚分派） | 需添加 vtable 间接调用（~5-10ns） |
| `is` 检查 | 编译期解析（仅基本类型） | 运行时 type_id 比较 + 继承链遍历 |

**结论**：AOT 基本类型零代价，堆对象操作略增开销（+2-3 条指令），但语义完整性大幅提升。

### 4.6 综合性能预估

| 场景 | 预估变化 |
|------|----------|
| 纯基本类型程序 | **零变化**（基本类型不分配堆对象） |
| 含 class 的多态程序 | **性能提升 2-5 倍**（字段访问+vtable 分派优化） |
| 含 `is`/`as` 的程序 | **从不可用变为可用**（性能略增但语义正确） |
| 大型堆对象程序（1000+ 对象） | **内存减少 40-60%**（vtable 不拷贝+字段用 Vec） |

---

## 五、实施计划

### 5.1 阶段划分

```
┌──────────────────────────────────────────────────────────────────┐
│ Phase 1: 基础设施（1 周）                                          │
│                                                                   │
│ • 类 ID 系统：替代 FNV 哈希，编译时分配递增 ID                      │
│ • 类定义表：module.classes: Vec<ClassDef>（含 parent_id）          │
│ • 字段槽位分配：编译期为每个类分配字段槽位索引                       │
│ • 模块序列化：.auc v6 格式（含类定义表）                           │
├──────────────────────────────────────────────────────────────────┤
│ Phase 2: 运行时改造（1 周）                                        │
│                                                                   │
│ • HeapData::Object 重构：vtable_idx + Vec<Value> 字段             │
│ • 新增 InstanceOf / CheckCast 字节码指令                          │
│ • VM 实现：is/as 支持堆对象类层级                                  │
│ • JIT 实现：InstanceOf / CheckCast 指令                            │
│ • aura_isOfType 改造：支持堆对象类层级                             │
├──────────────────────────────────────────────────────────────────┤
│ Phase 3: AOT 改造（1 周）                                          │
│                                                                   │
│ • 对象头布局：%struct.T = { i32 type_id, i32 vtable_idx, ... }   │
│ • AOT InstanceOf / CheckCast 生成                                  │
│ • AOT vtable 间接调用                                              │
│ • AOT toString 虚分派                                              │
├──────────────────────────────────────────────────────────────────┤
│ Phase 4: 标准库 + 文档 + 测试（0.5-1 周）                          │
│                                                                   │
│ • Any 基类内置方法：toString / equals / hashCode / typeOf       │
│ • String + T 降级为 toString 虚调用                               │
│ • sealed class when 穷举检查（修复遗留问题 #6）                    │
│ • 更新 book/chapter-03.md（类型系统文档）                          │
│ • 更新 examples/classes/*.aura（验证 is/as）                      │
│ • 回归测试：确保现有 class_runtime.aura 等示例仍通过              │
└──────────────────────────────────────────────────────────────────┘
```

### 5.2 影响文件清单

| 文件 | 修改类型 | 预估改动量 |
|------|----------|-----------|
| `compiler/src/sema/ty.rs` | 新增 Class 类型相关 | +20 行 |
| `compiler/src/sema/checker.rs` | 类层级检查增强 | +80 行 |
| `compiler/src/codegen/emit.rs` | 类 ID 分配、字段槽位 | +150 行 |
| `compiler/src/codegen/opcode.rs` | InstanceOf/CheckCast 指令 | +40 行 |
| `compiler/src/codegen/serialize.rs` | .auc v6 格式 | +60 行 |
| `compiler/src/vm/value.rs` | type_name 支持堆对象 | +30 行 |
| `compiler/src/vm/heap.rs` | HeapData::Object 重构 | +100 行 |
| `compiler/src/vm/interp.rs` | InstanceOf/CheckCast 实现 | +60 行 |
| `compiler/src/vm/jit.rs` | JIT InstanceOf/CheckCast | +50 行 |
| `compiler/src/vm/abi.rs` | TAG_CLASS 标签 | +5 行 |
| `compiler/src/codegen/aot/emit.rs` | AOT 对象头 + vtable | +120 行 |
| `compiler/src/codegen/aot/types.rs` | 类型映射更新 | +30 行 |
| `compiler/src/std/std_builtin.rs` | typeOf 增强 | +20 行 |
| `compiler/src/vm/native.rs` | aura_isOfType 改造 | +30 行 |
| `book/chapter-03.md` | 文档更新 | +200 行 |
| `examples/classes/class_runtime.aura` | is/as 验证 | +40 行 |

**总计**：约 1045 行新增/修改代码。

### 5.3 与 struct→value class 迁移的协调

`struct与class定位方案.md` 计划将 `struct` 重命名为 `value class`。引入 Any 基类时需注意：

```
时间线：
  v0.5: struct 仍是别名，class 开始完善运行时支持
  v1.0: struct deprecated，推荐 value class
  v2.0: struct 移除

Any 基类引入与 struct→value class 迁移的关系：
  - class → 参与 Any 层级（堆对象，有对象头）
  - value class → 不参与 Any 层级（值类型，无对象头）
  - struct（deprecated）→ 等价于 value class
  - Any 基类引入不影响 struct→value class 迁移，两者正交
```

### 5.4 向后兼容性

| 场景 | 兼容性 |
|------|--------|
| 现有 `class Dog : Animal()` 代码 | ✅ 兼容（语法不变，语义增强） |
| 现有 `value class` 代码 | ✅ 兼容（不参与类层级，行为不变） |
| 现有 `struct` 代码 | ✅ 兼容（等价于 value class） |
| 现有 `.auc` v5 文件 | ⚠️ 需支持 v5→v6 读取（向后兼容读取） |
| 现有 `is Int` 基本类型检查 | ✅ 兼容（通过 tag 匹配，行为不变） |
| 现有 `is MyClass` 堆对象检查 | ⚠️ 行为变化：从 false 变为可能 true（修复缺陷） |
| 现有 `aura_isOfType` 调用 | ✅ 兼容（增强支持，基本类型行为不变） |

---

## 六、风险评估

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| .auc 格式变更导致旧文件不可读 | 中 | 高 | v6 格式设计为向后兼容读取 v5（新增字段可选） |
| HeapData 重构导致内存泄漏 | 低 | 高 | 分阶段重构，保留旧结构为 fallback；ARC 分析器同步更新 |
| AOT 对象头导致性能回退 | 中 | 中 | 仅堆对象受影响，基本类型零代价；AOT 基准测试验证 |
| JIT 白名单扩展导致 JIT 覆盖不足 | 低 | 中 | InstanceOf/CheckCast 加入 JIT 白名单；回退到解释器 |
| 类 ID 碰撞 | 低 | 高 | 编译时分配递增 ID（非哈希），模块内唯一 |
| 跨模块类 ID 冲突 | 中 | 中 | 全局类型表：模块加载时分配全局类 ID 范围 |
| value class 与 Any 冲突 | 低 | 高 | 明确规则：value class 不参与类层级，is 检查时 type_name 返回类名 |
| sealed class 穷举检查引入错误 | 中 | 低 | 分阶段实现，先修复 enum 穷举，再扩展 sealed class |

### 6.1 关键风险详解

#### 风险 1：.auc 格式变更

**问题**：当前 .auc v5 格式中 vtable 是 `Vec<VirtualTable>`（type_tag → slots）。新设计需要类定义表（含 parent_id、字段槽位映射）。

**缓解**：
```
.auc v6 格式设计：
  1. 保留 v5 的 vtables 段（向后兼容读取）
  2. 新增 class_defs 段（类定义表）
  3. 版本号检查：v6 文件含 class_defs 段，v5 文件回退到旧逻辑
  4. header_flags 新增 HEADER_HAS_CLASS_DEFS 标志
```

#### 风险 2：跨模块类型系统

**问题**：当前模块系统是扁平的（`.auc` 文件含一个模块的字节码）。引入类层级后，跨模块的类继承（模块 A 定义 Animal，模块 B 继承 Dog : Animal()）需要全局类型表。

**缓解**：
```
方案 A（简单，推荐）：
  - 每个模块独立分配类 ID（0-based）
  - 跨模块继承通过类名解析（与当前 class 解析一致）
  - 运行时 is 检查在单模块内有效（跨模块 is 暂不支持）

方案 B（完整，后续版本）：
  - 全局类型注册表：模块加载时注册类 ID 范围
  - 跨模块继承通过全局类 ID 解析
  - 运行时 is 检查支持跨模块继承链
```

#### 风险 3：AOT 对象头性能

**问题**：AOT 中堆对象需要对象头（type_id + vtable_idx），字段访问多一次间接寻址。

**缓解**：
```
1. 对象头放在结构体最前面（LLVM 可利用 layout 优化）
2. 小类（字段 < 4 个）可考虑将对象头内联到调用者栈（逃逸分析后）
3. AOT 基准测试：对比有无对象头的性能差异
4. 如果性能差异 > 20%，考虑延迟到 v0.6 版本实现 AOT 对象头
```

---

## 七、与 Java/Kotlin 的对比

| 特性 | Java | Kotlin | Aura（当前） | Aura（新设计） |
|------|------|--------|-------------|---------------|
| 顶级类型 | `Object` | `Any` | `Ty::Any`（类型级） | `Ty::Any`（类型级）+ Any（运行时级） |
| 基本类型是对象？ | ✅（int→Integer 装箱） | ✅（但可内联） | ❌（内联存储） | ❌（内联存储） |
| toString() | Object 虚方法 | Any 虚方法 | 无统一 API | Any 虚方法 ✅ |
| equals/hashCode | Object 虚方法 | Any 虚方法 | 无统一 API | Any 虚方法 ✅ |
| is instanceof | ✅ 层级遍历 | ✅ 层级遍历 | ❌ 字符串比较 | ✅ 层级遍历 |
| as 类型转换 | ✅（ClassCastException） | ✅（as/as?） | ❌ 未实现 | ✅ CheckCast 指令 |
| 值类型 | ❌（无） | ✅（value class） | ✅（value class/struct） | ✅（不变） |
| sealed class | ❌（Java 17+ 有） | ✅ | ✅（语法） | ✅（+穷举检查） |
| 反射 getClass() | ✅ | ✅ | ❌ | ✅（typeOf 增强） |
| 基本类型性能 | ❌ 装箱开销 | ⚠️ 内联优化 | ✅ 零开销 | ✅ 零开销 |
| 堆对象内存 | 16 字节头 + 字段 | 16 字节头 + 字段 | ~164 字节/对象 | ~30 字节/对象 |

**关键差异**：
- Java/Kotlin 所有类型都是对象（或可装箱为对象），Aura 保持基本类型内联——这是性能优势
- Aura 的堆对象内存开销**小于** Java/Kotlin（~30 字节 vs ~16+ 字节头，因为字段用 Vec 而非数组+哈希表）
- Aura 的 vtable 不每对象拷贝（仅持有索引）——Java/Kotlin 也如此（通过类型对象共享 vtable）

---

## 八、决策建议

### 8.1 推荐决策

**✅ 引入 Any 基类，采用分层 Any 模型（选项 B）**

理由：
1. **修复致命缺陷**：`is`/`as` 对 class 实例不可用，这是类系统完善的前提
2. **性能不降反升**：vtable 不拷贝 + 字段用 Vec，内存和速度均优于当前设计
3. **与 value class 兼容**：value class 不参与类层级，两者正交
4. **与 struct→value class 迁移协调**：Any 基类引入不影响迁移时间线
5. **AOT 基本类型零代价**：仅堆对象受影响
6. **标准库受益**：toString/equals/hashCode 统一，String+T 降级更自然

### 8.2 不做什么

- ❌ 不全对象化（不引入 Int 是 Any 的语义）
- ❌ 不改变 value class 的语义（值类型仍无 vtable）
- ❌ 不在 Phase 1 支持跨模块类层级（延后到 v1.0）
- ❌ 不改变基本类型的 is 检查行为（通过 tag 匹配，不变）

### 8.3 前置条件

1. **struct→value class 迁移方向已确认**（`struct与class定位方案.md` 已定稿）
2. **class 运行时支持完成**（P-K2 已完成 vtable 分派）
3. **遗留问题 #6 修复计划**（sealed class 穷举检查与 Any 基类同步修复）

### 8.4 时间线建议

```
v0.5（当前 → +2 月）：
  Phase 1-2: 基础设施 + VM/JIT 改造
  → is/as 对 class 实例可用（VM/JIT 模式）

v0.6（+4 月）：
  Phase 3: AOT 改造
  → is/as 对 class 实例可用（AOT 模式）
  → AOT vtable 间接调用

v1.0（+6 月）：
  Phase 4: 标准库完善 + 跨模块类层级
  → Any 基类内置方法完整
  → sealed class 穷举检查
  → 跨模块继承支持
```

---

## 九、附录

### 附录 A：核心代码位置索引

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
| JIT JitValue | `compiler/src/vm/abi.rs` | 13-37 |
| AOT 类型映射 | `compiler/src/codegen/aot/types.rs` | 33-99 |
| AOT aura_isOfType 编译期解析 | `compiler/src/codegen/aot/emit.rs` | 1820-1853 |
| is 表达式降级 | `compiler/src/codegen/hir.rs` | 3201-3210 |
| pattern_to_expr（is 模式） | `compiler/src/parser.rs` | 2992-2999 |
| 继承开放性检查 | `compiler/src/sema/checker.rs` | 831-847 |
| override 一致性检查 | `compiler/src/sema/checker.rs` | 848-896 |
| value class 不可继承 | `compiler/src/sema/checker.rs` | 563-571 |

### 附录 B：与遗留问题的关联

| 遗留问题编号 | 描述 | 与 Any 基类的关系 |
|-------------|------|---------------------|
| #6 | when 穷举性检查仅支持 enum | Any 基类引入后同步修复（sealed class 穷举） |
| #7 | JIT 白名单仅覆盖叶子指令 | Any 基类新增的 InstanceOf/CheckCast 加入白名单 |
| #14 | AOT emit_call 返回值硬编码 i32 | Any 基类的 AOT vtable 调用需先修复此问题 |
| #20 | 泄漏检测不检测运行时堆 | Any 基类重构 HeapData 后需更新泄漏检测 |

### 附录 C：测试用例设计

```aura
// tests/any_hierarchy_test.aura

// ① 基本类型 is 检查
fun test_primitive_is() {
    assert(1 is Int)
    assert(!1 is String)
    assert(1 is Any)
    assert("hello" is String)
    assert(true is Boolean)
    assert(null is Any)
}

// ② 堆对象 is 检查
open class Animal {
    open fun name(): String = "animal"
}
class Dog : Animal() {
    override fun name(): String = "dog"
}
class Cat : Animal() {
    override fun name(): String = "cat"
}

fun test_object_is() {
    val d: Animal = Dog()
    assert(d is Dog)       // 自身类 → true
    assert(d is Animal)    // 父类 → true
    assert(d is Cat)       // 兄弟类 → false
    assert(d is Any)       // 顶级类型 → true
    assert(d is Int)       // 基本类型 → false
}

// ③ as 类型转换
fun test_object_as() {
    val a: Animal = Dog()
    val d: Dog? = a as? Dog      // 安全转换 → Dog 实例
    val c: Cat? = a as? Cat      // 安全转换 → null
    // val x: Cat = a as Cat     // 严格转换 → 抛异常（暂不测试）
}

// ④ toString 虚方法
class Vec2 {
    val x: Int = 0
    val y: Int = 0
    override fun toString(): String = "Vec2($x, $y)"
}
fun test_tostring() {
    val v = Vec2()
    assert(v.toString() == "Vec2(0, 0)")
    assert("prefix " + v == "prefix Vec2(0, 0)")  // String + T 虚分派
}

// ⑤ sealed class 穷举
sealed class Shape {
    open fun area(): Float = 0.0f
}
class Circle : Shape() {
    override fun area(): Float = 1.0f
}
class Square : Shape() {
    override fun area(): Float = 2.0f
}
fun computeArea(s: Shape): Float {
    when (s) {
        is Circle -> s.area()
        is Square -> s.area()
        // 无 else 分支——编译器验证穷举性 ✅
    }
}
```

### 附录 D：字节码格式变更（.auc v6）

```
当前 v5 格式：
  header (64 bytes)
  consts 段
  natives 段
  functions 段
  closures 段
  vtables 段
  [aot 段]

新 v6 格式（向后兼容 v5）：
  header (64 bytes, 新增 HEADER_HAS_CLASS_DEFS 标志)
  consts 段
  natives 段
  functions 段
  closures 段
  vtables 段（保留，向后兼容）
  [class_defs 段]  ← 新增
  [aot 段]

class_defs 段格式：
  u16 count          // 类数量
  for each class:
    u16 type_id      // 类 ID（0-based）
    u16 name_offset  // 类名在 string_pool 中的偏移
    u16 name_len
    u16 parent_id    // 父类 ID（0xFFFF = 无父类）
    u16 field_count  // 字段数量
    u16 vtable_idx   // vtable 索引（到 vtables 段）
    u16 interface_count
    u16 interfaces[] // 接口 ID 列表
    u16 field_names_offset[] // 字段名偏移
    u16 field_slots[]        // 字段槽位索引
```



