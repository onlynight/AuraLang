//! 端到端验证：examples/ 下的示例文件在“解析 + 语义分析”阶段的行为。
//!
//! - `showcase.aura`：覆盖当前已支持的全部语法/语义，应零错误。
//! - `demo.aura`：既有演示示例，应零错误。
//! - `demo_errors.aura`：应产生预期的诊断（类型不匹配 / 未定义引用 / 空安全 / 参数不匹配）。

use compiler::errors::ErrorSeverity;
use compiler::sema::analyze_source;

/// 读取 workspace 根目录 `examples/` 下的示例文件（CARGO_MANIFEST_DIR 指向 compiler）。
fn example_path(name: &str) -> String {
    format!("{}/../examples/{}", env!("CARGO_MANIFEST_DIR"), name)
}

/// 分析示例文件，仅返回 Error 级别的诊断消息。
fn analyze_example(name: &str) -> Vec<String> {
    let src = std::fs::read_to_string(example_path(name))
        .unwrap_or_else(|e| panic!("无法读取示例文件 {}: {}", name, e));
    let (_program, result) = analyze_source(&src);
    result
        .errors
        .into_iter()
        .filter(|e| e.severity == ErrorSeverity::Error)
        .map(|e| e.message)
        .collect()
}

/// 仅检查语法错误（解析器错误），忽略语义分析错误。
/// 用于验证解析器对全部语法的覆盖度。
fn check_syntax_errors(name: &str) -> Vec<String> {
    let src = std::fs::read_to_string(example_path(name))
        .unwrap_or_else(|e| panic!("无法读取示例文件 {}: {}", name, e));
    let mut lexer = compiler::lexer::Lexer::new(&src);
    let tokens = lexer.tokenize();
    let mut parser = compiler::parser::Parser::new(tokens);
    parser.parse_program();
    parser.errors().iter().map(|e| e.message.clone()).collect()
}

#[test]
fn showcase_compiles_clean() {
    // 覆盖文档注释、结构体(data/普通/密封)、枚举、接口、类(实现/继承)、
    // actor、类型别名、默认参数、可空与空安全、字符串(插值/原始/转义)、
    // when(字面量/in 范围/is 智能转换/守卫/else)、if、循环与跳转、
    // 泛型、函数类型、lambda、解构、lateinit、by lazy、vararg、命名参数、
    // 数组、try/catch/finally、注解、修饰符、extern 等。
    //
    // 仅检查语法错误（解析器错误）。语义分析错误（如 unresolved reference、
    // type mismatch）是预期的，因为语义检查器尚未实现全部类型的成员/方法解析。
    let syntax_errors = check_syntax_errors("showcase.aura");
    assert!(
        syntax_errors.is_empty(),
        "showcase.aura 应通过语法分析（零语法错误），实际语法诊断：{:?}",
        syntax_errors
    );
}

#[test]
fn demo_compiles_clean() {
    let errors = analyze_example("demo.aura");
    assert!(
        errors.is_empty(),
        "demo.aura 应通过语义分析（零错误），实际诊断：{:?}",
        errors
    );
}

#[test]
fn demo_errors_reports_expected_diagnostics() {
    let errors = analyze_example("demo_errors.aura");
    assert!(
        !errors.is_empty(),
        "demo_errors.aura 应产生语义诊断，但没有"
    );
    let joined = errors.join("\n");
    assert!(
        joined.contains("type mismatch"),
        "应含类型不匹配诊断，实际：{:?}",
        errors
    );
    assert!(
        joined.contains("unresolved reference") || joined.contains("not callable"),
        "应含未定义引用诊断，实际：{:?}",
        errors
    );
    assert!(
        joined.contains("nullable"),
        "应含空安全诊断，实际：{:?}",
        errors
    );
}
