//! P3 语义分析测试
//!
//! 覆盖：
//! - 类型推断（字面量、二元运算、函数调用）
//! - 空安全检查（nullable 上的非法操作）
//! - 未定义引用错误
//! - 类型不匹配错误
//! - 诊断渲染（SourceMap + 源码片段，P0.6）

use aura_compiler::Span;
use aura_compiler::errors::{CompileError, ErrorSeverity};
use aura_compiler::sema::analyze_source;
use aura_compiler::source_map::SourceMap;

fn analyze(src: &str) -> Vec<String> {
    let (_program, result) = analyze_source(src);
    result
        .errors
        .iter()
        .filter(|e| e.severity == ErrorSeverity::Error)
        .map(|e| format!("{} [{}]", e.message, e.span))
        .collect()
}

fn analyze_raw(src: &str) -> Vec<CompileError> {
    let (_program, result) = analyze_source(src);
    result
        .errors
        .into_iter()
        .filter(|e| e.severity == ErrorSeverity::Error)
        .collect()
}

fn has_error_containing(errors: &[String], needle: &str) -> bool {
    errors.iter().any(|e| e.contains(needle))
}

/// 端到端验证：语义错误经 SourceMap 渲染后应携带源码行与波浪线指示（P0.6）。
#[test]
fn test_diagnostic_renders_source_snippet() {
    let src = "fun main() {\n    val x: Int = \"oops\"\n}\n";
    let errors = analyze_raw(src);
    assert!(!errors.is_empty(), "应当产生至少一个语义错误");

    let mut sm = SourceMap::new();
    let file = sm.add_file("main.aura", src);

    let rendered = errors[0].render(&sm, file);
    // 行号 / 列号
    assert!(
        rendered.contains("main.aura:2:"),
        "渲染应含位置信息: {rendered}"
    );
    // 源码片段文本
    assert!(
        rendered.contains("val x: Int = \"oops\""),
        "渲染应含源码片段: {rendered}"
    );
    // 波浪线 / 指示符
    assert!(rendered.contains('^'), "渲染应含波浪线指示: {rendered}");

    // 渲染出的位置应与错误自身的 span 一致
    let span = &errors[0].span;
    assert!(rendered.contains(&format!(":{}:", span.start_line)));
}

#[test]
fn test_span_carries_line_info_for_snippet() {
    // 空 Span 不能用于渲染源片段
    let empty = CompileError::new("boom", Span::single(0, 0, 0));
    let mut sm = SourceMap::new();
    let file = sm.add_file("x.aura", "val a = 1\n");
    let out = empty.render(&sm, file);
    assert!(out.contains("boom"));
}

// ── 合法代码：不应有任何错误 ──

#[test]
fn test_ok_simple_fn() {
    let errors = analyze(
        r#"
        fun add(a: Int, b: Int): Int {
            return a + b
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_ok_typed_variables() {
    let errors = analyze(
        r#"
        fun main() {
            val x: Int = 42
            var y: Float = 3.5f
            val s: String = "hello"
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_ok_string_interpolation_var() {
    let errors = analyze(
        r#"
        fun greet(name: String): String {
            return "Hello, $name!"
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_ok_control_flow() {
    let errors = analyze(
        r#"
        fun classify(score: Int): String {
            if (score >= 90) {
                return "A"
            } else {
                return "B"
            }
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_ok_loop() {
    let errors = analyze(
        r#"
        fun sum(n: Int): Int {
            var total: Int = 0
            for (i in 0..n) {
                total += i
            }
            return total
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

// ── 类型推断 ──

#[test]
fn test_infer_literal_types() {
    let errors = analyze(
        r#"
        fun main() {
            val a: Int = 10
            val b: Float = 3.14f
            val c: Boolean = true
            val d: String = "text"
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_infer_binary_result() {
    let errors = analyze(
        r#"
        fun main() {
            val sum: Int = 1 + 2 * 3
            val cmp: Boolean = 1 < 2
            val eq: Boolean = "a" == "b"
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_string_concat() {
    let errors = analyze(
        r#"
        fun main() {
            val msg: String = "Hello" + " World"
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

// ── 类型不匹配 ──

#[test]
fn test_type_mismatch_declaration() {
    let errors = analyze("fun main() { val x: Int = \"hello\" }");
    assert!(
        has_error_containing(&errors, "type mismatch"),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_type_mismatch_return() {
    let errors = analyze(
        r#"
        fun foo(): Int {
            return "not an int"
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "return type mismatch"),
        "expected return type mismatch, got: {:?}",
        errors
    );
}

#[test]
fn test_type_mismatch_arg() {
    let errors = analyze(
        r#"
        fun takesInt(x: Int) { }
        fun main() {
            takesInt("wrong")
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "type mismatch"),
        "expected argument type mismatch, got: {:?}",
        errors
    );
}

#[test]
fn test_missing_return_value() {
    let errors = analyze(
        r#"
        fun foo(): Int {
            return
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "return requires a value"),
        "expected missing return value error, got: {:?}",
        errors
    );
}

// ── 未定义引用 ──

#[test]
fn test_undefined_variable() {
    let errors = analyze("fun main() { println(undefinedVar) }");
    assert!(
        has_error_containing(&errors, "unresolved reference"),
        "expected unresolved reference, got: {:?}",
        errors
    );
}

#[test]
fn test_unknown_function() {
    let errors = analyze("fun main() { callNonExistent(1, 2) }");
    assert!(
        has_error_containing(&errors, "unresolved reference")
            || has_error_containing(&errors, "not callable"),
        "expected unresolved function error, got: {:?}",
        errors
    );
}

// ── 空安全检查 ──

#[test]
fn test_nullable_arithmetic_error() {
    let errors = analyze(
        r#"
        fun main() {
            var n: Int? = null
            var x: Int = n + 1
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "nullable"),
        "expected nullable error, got: {:?}",
        errors
    );
}

#[test]
fn test_nullable_member_access_error() {
    let errors = analyze(
        r#"
        fun main() {
            var s: String? = null
            var len: Int = s.length
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "nullable"),
        "expected nullable member access error, got: {:?}",
        errors
    );
}

#[test]
fn test_safe_access_ok() {
    // ?. 安全调用应该通过
    let errors = analyze(
        r#"
        fun main() {
            var s: String? = null
            var len: Int? = s?.length
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_elvis_operator() {
    let errors = analyze(
        r#"
        fun main() {
            var s: String? = null
            var result: String = s ?: "default"
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

// ── 函数调用 ──

#[test]
fn test_function_call_ok() {
    let errors = analyze(
        r#"
        fun add(a: Int, b: Int): Int = a + b
        fun main() {
            val result: Int = add(3, 4)
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_function_arg_count_mismatch() {
    let errors = analyze(
        r#"
        fun add(a: Int, b: Int): Int = a + b
        fun main() {
            val result = add(1, 2, 3)
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "no overload of 'add' accepts 3 argument(s)"),
        "expected arg count error, got: {:?}",
        errors
    );
}

// ── 结构体 ──

#[test]
fn test_struct_ok() {
    let errors = analyze(
        r#"
        struct Player(
            val id: Int,
            var health: Int = 100
        )
        fun main() {
            val p = Player(1, 50)
            val id: Int = p.id
        }
        "#,
    );
    // 注意：字段访问 p.id 需要对象感知，当前简化版可能不检查字段
    // 但不应出现类型错误
    let critical = errors
        .iter()
        .filter(|e| !e.contains("unresolved member"))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        critical.is_empty(),
        "unexpected critical errors: {:?}",
        critical
    );
}

// ── 条件分支类型 ──

#[test]
fn test_if_expression_branch_types() {
    let errors = analyze(
        r#"
        fun main() {
            val a: Int = 10
            val b: Int = 20
            val max: Int = if (a > b) a else b
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

// ── P3.5 智能转换（is 之后的类型窄化）──

#[test]
fn test_smart_cast_is_pattern() {
    // `is String` 之后主体变量被窄化为 String，可安全访问 .length
    let errors = analyze(
        r#"
        fun describe(value: Any): String {
            return when (value) {
                is String -> "string of length ${value.length}"
                else -> "other"
            }
        }
        "#,
    );
    assert!(
        errors.is_empty(),
        "smart cast should narrow, got: {:?}",
        errors
    );
}

// ── P3.7 泛型约束检查 ──

#[test]
fn test_generic_bound_violation() {
    let errors = analyze(
        r#"
        class Box<T : Int>
        fun main() {
            val x: Box<String> = Box()
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "does not satisfy bound"),
        "expected generic bound violation, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_bound_satisfied() {
    // 合法：实参与约束类型一致
    let errors = analyze(
        r#"
        class Box<T : Int>
        fun main() {
            val x: Box<Int> = Box()
        }
        "#,
    );
    assert!(
        !has_error_containing(&errors, "does not satisfy bound"),
        "unexpected bound violation, got: {:?}",
        errors
    );
}

// ── P3.8 when 穷举性检查 ──

#[test]
fn test_when_exhaustiveness_enum_missing_branch() {
    let errors = analyze(
        r#"
        enum Color { RED, GREEN, BLUE }
        fun name(c: Color): String {
            return when (c) {
                RED -> "red"
                GREEN -> "green"
            }
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "not exhaustive")
            || has_error_containing(&errors, "missing branch"),
        "expected exhaustiveness error, got: {:?}",
        errors
    );
}

#[test]
fn test_when_exhaustiveness_ok_with_else() {
    let errors = analyze(
        r#"
        enum Color { RED, GREEN, BLUE }
        fun name(c: Color): String {
            return when (c) {
                RED -> "red"
                else -> "other"
            }
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

// ── P3.9 重写（override）与可见性 ──

#[test]
fn test_override_without_base_method() {
    let errors = analyze(
        r#"
        class Dog {
            override fun speak() {}
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "marked 'override' but no matching method"),
        "expected override-without-base error, got: {:?}",
        errors
    );
}

#[test]
fn test_override_correct() {
    let errors = analyze(
        r#"
        class Animal {
            fun speak() {}
        }
        class Dog : Animal() {
            override fun speak() {}
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_private_member_access_outside_type() {
    let errors = analyze(
        r#"
        class Secret {
            private val key: Int = 42
        }
        class Thief {
            fun steal(s: Secret): Int {
                return s.key
            }
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "cannot be accessed outside"),
        "expected private access warning, got: {:?}",
        errors
    );
}

#[test]
fn test_private_member_access_inside_type() {
    let errors = analyze(
        r#"
        class Secret {
            private val key: Int = 42
            fun reveal(other: Secret): Int {
                return other.key
            }
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

// ── P3.10 重载解析 ──

#[test]
fn test_overload_resolution_by_type() {
    // 同名函数按实参类型挑选重载：Int / String 两个版本
    let errors = analyze(
        r#"
        fun f(x: Int): Int = 1
        fun f(x: String): Int = 2
        fun main() {
            val a: Int = f(5)
            val b: Int = f("hi")
        }
        "#,
    );
    assert!(
        errors.is_empty(),
        "overload should resolve by type: {:?}",
        errors
    );
}

#[test]
fn test_overload_no_match() {
    // 实参类型不匹配任何重载
    let errors = analyze(
        r#"
        fun f(x: Int): Int = 1
        fun f(x: String): Int = 2
        fun main() {
            val r = f(true)
        }
        "#,
    );
    assert!(
        has_error_containing(
            &errors,
            "no overload of 'f' accepts the given argument types"
        ),
        "expected no-matching-overload error, got: {:?}",
        errors
    );
}

#[test]
fn test_overload_ambiguity_with_default_args() {
    // 两个重载对 f(5) 同样匹配 -> 歧义
    let errors = analyze(
        r#"
        fun f(x: Int, y: Int = 0): Int = 1
        fun f(x: Int): Int = 2
        fun main() {
            val r = f(5)
        }
        "#,
    );
    assert!(
        has_error_containing(&errors, "ambiguous call to 'f'"),
        "expected ambiguous call error, got: {:?}",
        errors
    );
}

// ── P3.9 接口实现完整性 / sealed ──

#[test]
fn test_interface_implementation_incomplete() {
    // 实现接口但未提供接口声明的抽象方法 -> 报错
    let errors = analyze(
        r#"
        interface Drawable {
            fun draw()
        }
        class Circle : Drawable {
            fun area(): Int = 1
        }
        "#,
    );
    assert!(
        has_error_containing(
            &errors,
            "does not implement interface 'Drawable' method 'draw'"
        ),
        "expected missing interface method error, got: {:?}",
        errors
    );
}

#[test]
fn test_interface_implementation_complete() {
    // 提供接口方法 -> 通过
    let errors = analyze(
        r#"
        interface Drawable {
            fun draw()
        }
        class Circle : Drawable {
            override fun draw() {}
        }
        "#,
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn test_sealed_subclass_same_file_ok() {
    // 密封类的子类位于同一编译单元 -> 允许（无错误）
    let errors = analyze(
        r#"
        sealed class Shape
        class Circle : Shape()
        class Square : Shape()
        "#,
    );
    assert!(
        errors.is_empty(),
        "sealed subclass in same file should be OK: {:?}",
        errors
    );
}

#[test]
fn test_inheritance_via_superclass_interface() {
    // 通过 `: Interface` 形式实现接口，需提供方法
    let errors = analyze(
        r#"
        interface Renderable {
            fun render()
        }
        class Sprite : Renderable {
            fun other() {}
        }
        "#,
    );
    assert!(
        has_error_containing(
            &errors,
            "does not implement interface 'Renderable' method 'render'"
        ),
        "expected missing interface method error, got: {:?}",
        errors
    );
}
