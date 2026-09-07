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

use compiler::codegen::{compile_source, from_bytes, to_bytes};
use compiler::vm::{Value, Vm, VmOptions};

fn run_main(source: &str) -> Value {
    let module = compile_source(source).expect("编译应成功");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    vm.run().expect("运行应成功")
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
    // Aura 既有惯例：init(params) { ... } 作为构造函数
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
    let module = compile_source(src).expect("编译应成功");
    assert!(!module.vtables.is_empty(), "应生成虚方法表");
    let bytes = to_bytes(&module);
    let loaded = from_bytes(&bytes).expect("反序列化应成功");
    assert_eq!(loaded.vtables.len(), module.vtables.len());
    let mut vm = Vm::new(&loaded, VmOptions::default()).expect("VM 初始化");
    let r = vm.run().expect("运行应成功");
    assert_eq!(r, Value::Int(1));
}
