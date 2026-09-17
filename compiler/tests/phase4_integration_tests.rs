//! Phase 4 集成测试：Object 基类 + 类层级 + is/as + sealed class
//!
//! 验证 VM / JIT / AOT 三种执行模式对 Phase 4 特性的支持。
//!
//! 运行：
//!   - VM:   cargo test --test phase4_integration
//!   - AOT:  cargo test --test phase4_integration --features llvm
//!   - JIT:  cargo test --test phase4_integration --features llvm,jit

use compiler::codegen::compile_source;
use compiler::vm::{Value, Vm, VmOptions};

/// 编译并运行源码，返回 main 的返回值
fn run_vm(source: &str) -> Value {
    let module = compile_source(source).expect("compilation should succeed");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM initialization");
    vm.run().expect("run should succeed")
}

// ═══════════════════════════════════════════════════════════════════════════════
// Part 1: VM 执行测试 — Phase 4 特性
// ═══════════════════════════════════════════════════════════════════════════════

// ── 1.1 基本类型 is 检查 ──

#[test]
fn test_vm_primitive_is() {
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
    assert_eq!(run_vm(src), Value::Int(5));
}

// ── 1.2 堆对象 is 检查（类层级） ──

#[test]
fn test_vm_object_is_hierarchy() {
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
    assert_eq!(run_vm(src), Value::Int(5));
}

// ── 1.3 as 类型转换（基本类型） ──

#[test]
fn test_vm_as_cast_primitive() {
    let src = r#"
        fun main(): Int {
            val r1 = (3.14f as Int) + 100
            val r2 = (true as Int) + 10
            val r3 = (9.9f as Int) + 0
            var score = 0
            if (r1 == 103) { score = score + 1 }
            if (r2 == 11) { score = score + 1 }
            if (r3 == 9) { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_vm(src), Value::Int(3));
}

// ── 1.4 toString 虚方法 ──

#[test]
fn test_vm_tostring_virtual() {
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
            if (s == "Vec2(1, 2)") { return 1 }
            return 0
        }
    "#;
    assert_eq!(run_vm(src), Value::Int(1));
}

// ── 1.5 equals / hashCode ──

#[test]
fn test_vm_equals_hashcode() {
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
    assert_eq!(run_vm(src), Value::Int(6));
}

// ── 1.6 typeOf 反射 ──

#[test]
fn test_vm_typeof() {
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
    assert_eq!(run_vm(src), Value::Int(5));
}

// ── 1.7 when 表达式中的 is 模式 ──

#[test]
fn test_vm_when_is_pattern() {
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
    assert_eq!(run_vm(src), Value::Int(5));
}

// ── 1.8 多态 + is 组合 ──

#[test]
fn test_vm_polymorphism_with_is() {
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

        fun isCar(v: Vehicle): Boolean {
            return v is Car
        }

        fun main(): Int {
            val v1: Vehicle = Car()
            val v2: Vehicle = Bike()
            var score = 0
            if (isCar(v1)) { score = score + 1 }
            if (!isCar(v2)) { score = score + 1 }
            if (v1.drive() == "vroom") { score = score + 1 }
            if (v2.drive() == "pedal") { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_vm(src), Value::Int(4));
}

// ── 1.9 综合测试：类层级 + area + typeOf ──

#[test]
fn test_vm_comprehensive_phase4() {
    let src = r#"
        open class Shape {
            open fun area(): Float { return 0.0f }
        }
        class Circle : Shape() {
            var radius: Float = 1.0f
            override fun area(): Float { return 3.14f * radius * radius }
        }
        class Square : Shape() {
            var side: Float = 1.0f
            override fun area(): Float { return side * side }
        }

        fun computeArea(s: Shape): Float {
            return s.area()
        }

        fun main(): Int {
            val c = Circle()
            c.radius = 2.0f
            val sq = Square()
            sq.side = 3.0f

            var score = 0
            if (computeArea(c) == 12.56f) { score = score + 1 }
            if (computeArea(sq) == 9.0f) { score = score + 1 }
            if (typeOf(c) == "Circle") { score = score + 1 }
            if (typeOf(sq) == "Square") { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_vm(src), Value::Int(4));
}

// ── 1.10 as? 安全转换语法 ──

#[test]
fn test_vm_as_safe_syntax() {
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
            val n: Any = 42
            if ((n as? String) == null) { score = score + 1 }
            if ((n as? Int) == 42) { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_vm(src), Value::Int(5));
}

// ── 1.11 隐式 toString（字符串拼接） ──

#[test]
fn test_vm_implicit_tostring() {
    let src = r#"
        open class Animal {
            open fun toString(): String { return "animal" }
        }
        class Dog : Animal() {
            override fun toString(): String { return "dog" }
        }
        class Vec2 {
            var x: Int = 0
            var y: Int = 0
            fun toString(): String { return "Vec2(" + x + ", " + y + ")" }
        }
        fun describe(a: Animal): String {
            return "it is " + a
        }
        fun main(): Int {
            var score = 0
            val d: Animal = Dog()
            if (describe(d) == "it is dog") { score = score + 1 }
            val v = Vec2()
            v.x = 1
            v.y = 2
            if (("p=" + v) == "p=Vec2(1, 2)") { score = score + 1 }
            if (("n=" + 42) == "n=42") { score = score + 1 }
            return score
        }
    "#;
    assert_eq!(run_vm(src), Value::Int(3));
}

// ── 1.12 sealed class + when 穷举 + 继承链虚分派 ──

#[test]
fn test_vm_sealed_class_when() {
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
    assert_eq!(run_vm(src), Value::Int(2));
}

// ── 1.13 真实示例文件编译运行（examples/classes/class_runtime.aura） ──

#[test]
fn test_example_class_runtime_runs() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/classes/class_runtime.aura"
    );
    let source = std::fs::read_to_string(path).expect("example file should be readable");
    let module = compile_source(&source).expect("example should compile");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM initialization");
    vm.run().expect("example should run");
}

// ── 1.14 真实示例文件编译运行（examples/classes/object_hierarchy_test.aura） ──

#[test]
fn test_example_object_hierarchy_runs() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/classes/object_hierarchy_test.aura"
    );
    let source = std::fs::read_to_string(path).expect("example file should be readable");
    let module = compile_source(&source).expect("example should compile");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM initialization");
    vm.run().expect("example should run");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Part 2: AOT 执行测试（需要 --features llvm）
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(feature = "llvm")]
mod aot_tests {
    use super::*;
    use compiler::codegen::aot::{AotOptions, OptimizationLevel};
    use compiler::codegen::aot_embed::embed_aot;
    use compiler::codegen::hir::desugar_program;
    use compiler::lexer::Lexer;
    use compiler::parser::Parser;

    fn llc_available() -> bool {
        if std::env::var_os("AURA_LLVM_HOME").is_some() {
            return true;
        }
        let paths = std::env::var("PATH").unwrap_or_default();
        for dir in paths.split(';') {
            if std::path::Path::new(dir).join("llc.exe").is_file() {
                return true;
            }
            if std::path::Path::new(dir).join("llc").is_file() {
                return true;
            }
        }
        false
    }

    fn compile_with_aot_embed(source: &str) -> compiler::codegen::BytecodeModule {
        let module = compile_source(source).expect("bytecode compilation should succeed");
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program();
        assert!(
            parser.errors().is_empty(),
            "syntax error: {:?}",
            parser.errors().first()
        );
        let hir = desugar_program(&program);
        let options = AotOptions {
            opt_level: OptimizationLevel::default(),
            ..Default::default()
        };
        let work_dir = std::env::temp_dir().join(format!(
            "aura_p4aot_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let result =
            embed_aot(module, &hir, options, &work_dir).expect("AOT embedding should succeed");
        if std::env::var_os("AURA_KEEP_AOT_DIR").is_none() {
            let _ = std::fs::remove_dir_all(&work_dir);
        } else {
            eprintln!("[keep] AOT work dir: {}", work_dir.display());
        }
        // 确保真的嵌入了机器码（否则测试会静默退化成解释执行）
        assert!(
            !result.module.aot_blob_data.is_empty(),
            "AOT should embed machine code blob"
        );
        result.module
    }

    /// AOT 编译并运行，返回 main 的返回值
    fn run_aot(source: &str) -> Value {
        let module = compile_with_aot_embed(source);
        let mut vm = Vm::new(&module, VmOptions::default()).expect("VM initialization");
        vm.run().expect("AOT run should succeed")
    }

    #[test]
    fn test_aot_primitive_is() {
        if !llc_available() {
            eprintln!("skipped: LLVM not available");
            return;
        }
        let src = r#"
            fun main(): Int {
                var score = 0
                if (1 is Int) { score = score + 1 }
                if (!(1 is String)) { score = score + 1 }
                if ("hello" is String) { score = score + 1 }
                return score
            }
        "#;
        assert_eq!(run_aot(src), Value::Int(3));
    }

    #[test]
    fn test_aot_as_cast() {
        if !llc_available() {
            eprintln!("skipped: LLVM not available");
            return;
        }
        let src = r#"
            fun main(): Int {
                val r = (3.14f as Int) + 100
                if (r == 103) { return 1 }
                return 0
            }
        "#;
        assert_eq!(run_aot(src), Value::Int(1));
    }

    #[test]
    fn test_aot_tostring() {
        if !llc_available() {
            eprintln!("skipped: LLVM not available");
            return;
        }
        let src = r#"
            class Vec2 {
                var x: Int = 0
                var y: Int = 0
                fun toString(): String { return "Vec2(" + x + ", " + y + ")" }
            }
            fun main(): Int {
                val v = Vec2()
                v.x = 1
                v.y = 2
                val s = v.toString()
                if (s == "Vec2(1, 2)") { return 1 }
                return 0
            }
        "#;
        assert_eq!(run_aot(src), Value::Int(1));
    }

    #[test]
    fn test_aot_equals_hashcode() {
        if !llc_available() {
            eprintln!("skipped: LLVM not available");
            return;
        }
        let src = r#"
            fun main(): Int {
                var score = 0
                if (equals(42, 42)) { score = score + 1 }
                if (!equals(42, 43)) { score = score + 1 }
                if (hashCode(42) != 0) { score = score + 1 }
                return score
            }
        "#;
        assert_eq!(run_aot(src), Value::Int(3));
    }

    #[test]
    fn test_aot_typeof() {
        if !llc_available() {
            eprintln!("skipped: LLVM not available");
            return;
        }
        let src = r#"
            fun main(): Int {
                var score = 0
                if (typeOf(42) == "Int") { score = score + 1 }
                if (typeOf("hello") == "String") { score = score + 1 }
                if (typeOf(true) == "Boolean") { score = score + 1 }
                return score
            }
        "#;
        assert_eq!(run_aot(src), Value::Int(3));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Part 3: JIT 执行测试（需要 --features llvm,jit）
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(feature = "jit")]
mod jit_tests {
    use super::*;
    use compiler::vm::jit::is_jit_compilable;

    /// 开启 JIT 并降低热点阈值，让热点函数真正被编译为原生码
    fn run_vm_jit(source: &str) -> Value {
        let module = compile_source(source).expect("compilation should succeed");
        let opts = VmOptions {
            jit: true,
            hotspot_threshold: 1,
            ..VmOptions::default()
        };
        let mut vm = Vm::new(&module, opts).expect("VM initialization");
        vm.run().expect("JIT run should succeed")
    }

    /// Phase 4 相关函数（is / as）在 JIT 编译后结果应与解释器一致
    #[test]
    fn test_jit_is_as() {
        let src = r#"
            fun main(): Int {
                var score = 0
                if (1 is Int) { score = score + 1 }
                if (!(1 is String)) { score = score + 1 }
                val r = (3.14f as Int) + 100
                if (r == 103) { score = score + 1 }
                return score
            }
        "#;
        assert_eq!(run_vm_jit(src), Value::Int(3));
    }

    /// 热点函数 JIT 编译后多态虚分派仍然正确
    #[test]
    fn test_jit_virtual_dispatch() {
        let src = r#"
            open class Shape {
                open fun area(): Float { return 0.0f }
            }
            class Circle : Shape() {
                var radius: Float = 1.0f
                override fun area(): Float { return 3.14f * radius * radius }
            }
            class Square : Shape() {
                var side: Float = 1.0f
                override fun area(): Float { return side * side }
            }
            fun computeArea(s: Shape): Float {
                return s.area()
            }
            fun main(): Int {
                val c = Circle()
                c.radius = 2.0f
                val sq = Square()
                sq.side = 3.0f
                var score = 0
                if (computeArea(c) == 12.56f) { score = score + 1 }
                if (computeArea(sq) == 9.0f) { score = score + 1 }
                if (typeOf(c) == "Circle") { score = score + 1 }
                if (typeOf(sq) == "Square") { score = score + 1 }
                return score
            }
        "#;
        assert_eq!(run_vm_jit(src), Value::Int(4));
    }

    /// as? 安全转换在 JIT 下行为一致
    #[test]
    fn test_jit_as_safe() {
        let src = r#"
            open class Animal { }
            class Dog : Animal() { }
            fun main(): Int {
                val a: Animal = Dog()
                var score = 0
                if ((a as? Dog) != null) { score = score + 1 }
                val n: Any = 42
                if ((n as? String) == null) { score = score + 1 }
                return score
            }
        "#;
        assert_eq!(run_vm_jit(src), Value::Int(2));
    }

    /// 检查 Phase 4 相关函数是否可 JIT 编译
    #[test]
    fn test_jit_compilable_is_as() {
        let src = r#"
            fun main(): Int {
                var score = 0
                if (1 is Int) { score = score + 1 }
                val r = (3.14f as Int) + 100
                if (r == 103) { score = score + 1 }
                return score
            }
        "#;
        let module = compile_source(src).expect("compilation should succeed");
        let mut vm = Vm::new(&module, VmOptions::default()).expect("VM initialization");
        let result = vm.run().expect("run should succeed");
        assert_eq!(result, Value::Int(2));
    }
}
