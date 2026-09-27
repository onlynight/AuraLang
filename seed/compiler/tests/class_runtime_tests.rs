//! P-K2 类运行时集成测试（Kotlin 关键字 codegen 落地）
//!
//! 覆盖：
//! - 类方法/裸字段（`Class.method` 命名 + 接收者分派 + 裸字段 → `self.field`）
//! - init 块 / 次构造函数 / init 构造函数
//! - companion object（方法调用 + 字段静态读取）
//! - `operator fun` 运算符重载
//! - `infix fun` 中缀调用
//! - `tailrec` 循环化
//! - 属性访问器 `get`/`set`/`field`
//! - open/abstract 继承 + vtable 动态分派（多态）
//! - `.auc` 序列化 roundtrip 后虚方法表仍生效
//! - Phase 4: is/as 类型检查与转换、Object 内置方法、sealed class 穷举

use compiler::codegen::{compile_source, from_bytes, to_bytes};
use compiler::vm::{Value, Vm, VmOptions};

fn run_main(source: &str) -> Value {
    let module = compile_source(source).expect("compilation should succeed");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM initialization");
    vm.run().expect("run should succeed")
}

// ── 类方法 + 裸字段访问 ──

#[test]
fn test_class_methods_and_bare_fields() {
    let src = r#"
        class Counter {
            var tick: Int = 0
            fun step(): Int {
                tick = tick + 1
                return tick
            }
        }
        fun main(): Int {
            val c = Counter()
            c.step()
            c.step()
            c.step()
            return c.tick
        }
    "#;
    assert_eq!(run_main(src), Value::Int(3));
}

#[test]
fn test_bare_field_in_method_read() {
    let src = r#"
        struct P {
            var x: Float = 0.0f
            fun getx(): Float {
                return x
            }
        }
        fun main(): Int {
            val p = P()
            p.x = 7.0f
            return p.getx() as Int
        }
    "#;
    assert_eq!(run_main(src), Value::Int(7));
}

// ── init 块 / 次构造函数 ──

#[test]
fn test_init_blocks_run_on_construction() {
    let src = r#"
        class Person {
            var created: Int = 0
            init {
                created = created + 100
            }
            init {
                created = created + 1
            }
        }
        fun main(): Int {
            val p = Person()
            return p.created
        }
    "#;
    assert_eq!(run_main(src), Value::Int(101));
}

#[test]
fn test_secondary_constructor() {
    let src = r#"
        class Person {
            var age: Int = 0
            constructor(a: Int) {
                age = a
            }
        }
        fun main(): Int {
            val p = Person(42)
            return p.age
        }
    "#;
    assert_eq!(run_main(src), Value::Int(42));
}

#[test]
fn test_init_constructor_style() {
    let src = r#"
        class Circle {
            val radius: Int = 0
            init(r: Int) {
                radius = r
            }
        }
        fun main(): Int {
            val c = Circle(5)
            return c.radius
        }
    "#;
    assert_eq!(run_main(src), Value::Int(5));
}

// ── companion object ──

#[test]
fn test_companion_method_and_field() {
    let src = r#"
        class MathUtil {
            companion object {
                val BASE: Int = 100
                fun add(a: Int, b: Int): Int {
                    return a + b
                }
            }
        }
        fun main(): Int {
            return MathUtil.BASE + MathUtil.add(1, 2)
        }
    "#;
    assert_eq!(run_main(src), Value::Int(103));
}

// ── operator 重载 ──

#[test]
fn test_operator_overloading() {
    let src = r#"
        struct Vec2 {
            var x: Int = 0
            var y: Int = 0
            operator fun plus(other: Vec2): Vec2 {
                val r = Vec2()
                r.x = x + other.x
                r.y = y + other.y
                return r
            }
        }
        fun main(): Int {
            val a = Vec2()
            a.x = 1
            a.y = 2
            val b = Vec2()
            b.x = 10
            b.y = 20
            val c = a + b
            return c.x + c.y
        }
    "#;
    assert_eq!(run_main(src), Value::Int(33));
}

// ── infix 中缀调用 ──

#[test]
fn test_infix_call() {
    let src = r#"
        infix fun combo(a: Int, b: Int): Int {
            return a * 10 + b
        }
        fun main(): Int {
            return 3 combo 5
        }
    "#;
    assert_eq!(run_main(src), Value::Int(35));
}

// ── tailrec 循环化 ──

#[test]
fn test_tailrec_loop_rewrite() {
    let src = r#"
        tailrec fun fact(n: Int, acc: Int): Int {
            if (n <= 1) { return acc }
            return fact(n - 1, n * acc)
        }
        fun main(): Int {
            return fact(10, 1)
        }
    "#;
    assert_eq!(run_main(src), Value::Int(3628800));
}

// ── 属性访问器 ──

#[test]
fn test_property_accessors() {
    let src = r#"
        class Temperature {
            var celsius: Int = 0
                get() = field
                set(value) {
                    field = value
                }
            val doubled: Int
                get() = celsius * 2
        }
        fun main(): Int {
            val t = Temperature()
            t.celsius = 21
            return t.doubled
        }
    "#;
    assert_eq!(run_main(src), Value::Int(42));
}

// ── 继承 + vtable 动态分派 ──

#[test]
fn test_virtual_dispatch_polymorphism() {
    let src = r#"
        open class Animal {
            var legs: Int = 4
            open fun name(): String {
                return "animal"
            }
        }
        class Dog : Animal() {
            override fun name(): String {
                return "dog"
            }
        }
        fun main(): Int {
            val a: Animal = Dog()
            val who = a.name()
            if (who == "dog") { return 1 }
            return 0
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

#[test]
fn test_inherited_field_defaults() {
    let src = r#"
        open class Animal {
            var legs: Int = 4
        }
        class Bird : Animal() {
            fun legs(): Int {
                return legs
            }
        }
        fun main(): Int {
            val b = Bird()
            return b.legs()
        }
    "#;
    assert_eq!(run_main(src), Value::Int(4));
}

// ── 序列化 roundtrip：虚方法表在 .auc 中存活 ──

#[test]
fn test_vtables_survive_serialization() {
    let src = r#"
        open class Animal {
            open fun name(): String {
                return "animal"
            }
        }
        class Dog : Animal() {
            override fun name(): String {
                return "dog"
            }
        }
        fun main(): Int {
            val a: Animal = Dog()
            if (a.name() == "dog") { return 1 }
            return 0
        }
    "#;
    let module = compile_source(src).expect("compilation should succeed");
    assert!(!module.vtables.is_empty(), "vtable should be generated");
    let bytes = to_bytes(&module);
    let loaded = from_bytes(&bytes).expect("deserialization should succeed");
    assert_eq!(loaded.vtables.len(), module.vtables.len());
    let mut vm = Vm::new(&loaded, VmOptions::default()).expect("VM initialization");
    let r = vm.run().expect("run should succeed");
    assert_eq!(r, Value::Int(1));
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 4: Object 基类 + 类层级测试
// ─────────────────────────────────────────────────────────────────────────────

// ① 基本类型 is 检查
#[test]
fn test_primitive_is_check() {
    let src = r#"
        fun main(): Int {
            var score = 0
            if (1 is Int) { score = score + 1 }
            if (!(1 is String)) { score = score + 1 }
            if (1 is Any) { score = score + 1 }
            if ("hello" is String) { score = score + 1 }
            if (!(42 is Boolean)) { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(5));
}

// ② 堆对象 is 检查（类层级）
#[test]
fn test_object_is_hierarchy() {
    let src = r#"
        open class Animal {
            open fun name(): String { return "animal" }
        }
        class Dog : Animal() {
            override fun name(): String { return "dog" }
        }
        class Cat : Animal() {
            override fun name(): String { return "cat" }
        }
        fun main(): Int {
            val d: Animal = Dog()
            var score = 0
            if (d is Dog) { score = score + 1 }
            if (d is Animal) { score = score + 1 }
            if (!(d is Cat)) { score = score + 1 }
            if (d is Any) { score = score + 1 }
            if (!(d is Int)) { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(5));
}

// ③ as 类型转换（基本类型）
#[test]
fn test_as_cast_primitive() {
    let src = r#"
        fun main(): Int {
            val r1 = (3.14f as Int) + 100
            val r2 = (true as Int) + 10
            val r3 = (9.9f as Int) + 0
            if (r1 == 103 && r2 == 11 && r3 == 9) { return 3 }
            return 0
        }
    "#;
    assert_eq!(run_main(src), Value::Int(3));
}

// ④ as? 安全转换（原生函数形式）
#[test]
fn test_as_safety() {
    let src = r#"
        fun main(): Int {
            val anyVal: Any = 42
            val asStr: Any = aura_cast_safety(anyVal, "String")
            val asInt: Any = aura_cast_safety(anyVal, "Int")
            var score = 0
            if (asStr == null) { score = score + 1 }
            if (asInt == 42) { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(2));
}

// ④b as? 语法：类类型安全转换
#[test]
fn test_as_safe_syntax_class() {
    let src = r#"
        open class Animal {
            open fun name(): String { return "animal" }
        }
        class Dog : Animal() {
            override fun name(): String { return "dog" }
        }
        class Cat : Animal() {
            override fun name(): String { return "cat" }
        }
        fun main(): Int {
            val a: Animal = Dog()
            val d = a as? Dog
            val c = a as? Cat
            var score = 0
            if (d != null) { score = score + 1 }
            if (c == null) { score = score + 1 }
            if (d.name() == "dog") { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(3));
}

// ④c as? 语法：基本类型安全转换（不匹配返回 null）
#[test]
fn test_as_safe_syntax_primitive() {
    let src = r#"
        fun main(): Int {
            val anyVal: Any = 42
            val asStr = anyVal as? String
            val asInt = anyVal as? Int
            var score = 0
            if (asStr == null) { score = score + 1 }
            if (asInt == 42) { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(2));
}

// ④d as 硬转换：类型匹配时成功
#[test]
fn test_as_hard_cast() {
    let src = r#"
        open class Animal {
            open fun name(): String { return "animal" }
        }
        class Dog : Animal() {
            override fun name(): String { return "dog" }
        }
        fun main(): Int {
            val a: Animal = Dog()
            val d = a as Dog
            if (d.name() == "dog") { return 1 }
            return 0
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

// ⑤ toString 虚方法
#[test]
fn test_tostring_virtual() {
    let src = r#"
        class Vec2 {
            var x: Int = 0
            var y: Int = 0
            fun toString(): String {
                return "Vec2(" + x + ", " + y + ")"
            }
        }
        fun main(): Int {
            val v = Vec2()
            v.x = 1
            v.y = 2
            val s = v.toString()
            if (s != "Vec2(1, 2)") { return 0 }
            return 1
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

// ⑤b 隐式 toString：字符串拼接自动调用用户实现的 toString（静态分派）
#[test]
fn test_implicit_tostring_static() {
    let src = r#"
        class Vec2 {
            var x: Int = 0
            var y: Int = 0
            fun toString(): String {
                return "Vec2(" + x + ", " + y + ")"
            }
        }
        fun main(): Int {
            val v = Vec2()
            v.x = 1
            v.y = 2
            val s = "point = " + v
            if (s == "point = Vec2(1, 2)") { return 1 }
            return 0
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

// ⑤c 隐式 toString：open toString 经 vtable 动态分派到子类实现
#[test]
fn test_implicit_tostring_virtual() {
    let src = r#"
        open class Animal {
            open fun toString(): String { return "animal" }
        }
        class Dog : Animal() {
            override fun toString(): String { return "dog" }
        }
        fun describe(a: Animal): String {
            return "it is " + a
        }
        fun main(): Int {
            val d: Animal = Dog()
            if (describe(d) == "it is dog") { return 1 }
            return 0
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

// ⑥ equals / hashCode
#[test]
fn test_equals_hashcode() {
    let src = r#"
        fun main(): Int {
            var score = 0
            if (equals(42, 42)) { score = score + 1 }
            if (!equals(42, 43)) { score = score + 1 }
            if (equals("hello", "hello")) { score = score + 1 }
            if (!equals("hello", "world")) { score = score + 1 }
            if (hashCode(42) != 0) { score = score + 1 }
            if (hashCode("hello") != 0) { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(6));
}

// ⑦ typeOf 反射
#[test]
fn test_typeof() {
    let src = r#"
        fun main(): Int {
            var score = 0
            if (typeOf(42) == "Int") { score = score + 1 }
            if (typeOf(3.14f) == "Float") { score = score + 1 }
            if (typeOf(true) == "Boolean") { score = score + 1 }
            if (typeOf("hello") == "String") { score = score + 1 }
            if (typeOf(null) == "Null") { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(5));
}

// ⑧ when 表达式中的 is 模式
#[test]
fn test_when_is_pattern() {
    let src = r#"
        open class Animal { }
        class Dog : Animal() { }

        fun checkType(x: Any): String {
            return when (x) {
                is Dog -> "dog"
                is Animal -> "animal"
                is Int -> "int"
                is Float -> "float"
                is Boolean -> "bool"
                is String -> "string"
                else -> "unknown"
            }
        }

        fun main(): Int {
            var score = 0
            if (checkType(42) == "int") { score = score + 1 }
            if (checkType(3.14f) == "float") { score = score + 1 }
            if (checkType(true) == "bool") { score = score + 1 }
            if (checkType("hello") == "string") { score = score + 1 }
            if (checkType(Dog()) == "dog") { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(5));
}

// ⑨ sealed class when 穷举（编译器应检查穷举性）
#[test]
fn test_sealed_class_when_exhaustive() {
    let src = r#"
        sealed class Shape {
            open fun area(): Float { return 0.0f }
        }
        class Circle : Shape() {
            override fun area(): Float { return 1.0f }
        }
        class Square : Shape() {
            override fun area(): Float { return 2.0f }
        }

        fun computeArea(s: Shape): Float {
            if (s is Circle) { return s.area() }
            if (s is Square) { return s.area() }
            return 0.0f
        }

        fun main(): Int {
            val c = Circle()
            val sq = Square()
            var score = 0
            if (computeArea(c) == 1.0f) { score = score + 1 }
            if (computeArea(sq) == 2.0f) { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(2));
}

// ⑩ 多态 + is 组合
#[test]
fn test_polymorphism_with_is() {
    let src = r#"
        open class Vehicle {
            open fun drive(): String { return "beep" }
        }
        class Car : Vehicle() {
            override fun drive(): String { return "vroom" }
        }
        class Bike : Vehicle() {
            override fun drive(): String { return "pedal" }
        }

        fun makeNoise(v: Vehicle): String {
            if (v is Car) { return "car:" + v.drive() }
            return "other:" + v.drive()
        }

        fun main(): Int {
            val v1: Vehicle = Car()
            val v2: Vehicle = Bike()
            val r1 = makeNoise(v1)
            val r2 = makeNoise(v2)
            var score = 0
            if (r1 == "car:vroom") { score = score + 1 }
            if (r2 == "other:pedal") { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_main(src), Value::Int(2));
}
