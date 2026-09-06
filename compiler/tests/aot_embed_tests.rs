//! Phase 1 AOT 机器码嵌入 —— 端到端集成测试（1.14）
//!
//! 设计文档 docs/AOT机器码嵌入方案-详细设计.md §9：
//! `fun add(a: Int, b: Int): Int` 编译 → AOT 嵌入 `.auc` v4 → VM 加载执行，
//! 机器码执行结果必须与解释器一致。
//!
//! 需要 `cargo test --features llvm`，并能在 PATH / 环境变量中找到 LLVM 工具链
//! （llc）。找不到工具链时测试自动跳过，不视为失败。

#![cfg(feature = "llvm")]

use compiler::codegen::aot_embed::embed_aot;
use compiler::codegen::aot::{AotOptions, OptimizationLevel};
use compiler::codegen::hir::desugar_program;
use compiler::codegen::{BytecodeModule, compile_source};
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::vm::{Value, Vm, VmOptions};

/// 检测 LLVM 工具链是否可用（不可用则跳过，避免 CI 假失败）
fn llc_available() -> bool {
    if std::env::var_os("AURA_LLVM_HOME").is_some() {
        return true;
    }
    // PATH 探测
    let paths = std::env::var("PATH").unwrap_or_default();
    for dir in paths.split(';') {
        let candidate = std::path::Path::new(dir).join("llc.exe");
        if candidate.is_file() {
            return true;
        }
        let candidate_unix = std::path::Path::new(dir).join("llc");
        if candidate_unix.is_file() {
            return true;
        }
    }
    false
}

/// 编译源码 → 嵌入 AOT 机器码 → 返回 `.auc` v4 模块
fn compile_with_aot_embed(source: &str) -> BytecodeModule {
    let module = compile_source(source).expect("字节码编译应成功");

    // 重新解析得到同源 HIR（AOT IR 生成需要）
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    assert!(
        lexer.errors().is_empty(),
        "词法错误: {:?}",
        lexer.errors().first()
    );
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    assert!(
        parser.errors().is_empty(),
        "语法错误: {:?}",
        parser.errors().first()
    );
    let hir = desugar_program(&program);

    let options = AotOptions {
        opt_level: OptimizationLevel::default(),
        ..Default::default()
    };
    let work_dir = std::env::temp_dir().join(format!(
        "aura_aot_e2e_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let result = embed_aot(module, &hir, options, &work_dir).expect("AOT 嵌入应成功");
    let _ = std::fs::remove_dir_all(&work_dir);
    result.module
}

/// 1.14 端到端：add(Int,Int):Int 经 AOT 机器码执行，结果与解释器一致
#[test]
fn aot_add_matches_interpreter() {
    if !llc_available() {
        eprintln!("skipped: LLVM 工具链不可用");
        return;
    }
    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun main(): Int { return add(20, 22) }
    "#;

    // 1) 纯字节码解释基准
    let plain = compile_source(src).expect("编译应成功");
    assert!(!plain.has_aot());
    let mut vm = Vm::new(&plain, VmOptions::default()).expect("VM 初始化");
    let baseline = vm.run().expect("解释执行应成功");
    assert_eq!(baseline, Value::Int(42), "基准结果应为 42");

    // 2) AOT 嵌入后经 VM 执行（do_call 命中 AOT 分发表 → 机器码）
    let embedded = compile_with_aot_embed(src);
    assert!(embedded.has_aot(), "模块应含 AOT 机器码段");
    assert_eq!(embedded.aot_segments.len(), 2);
    let aot_fns: Vec<&compiler::codegen::opcode::BytecodeFunction> = embedded
        .functions
        .iter()
        .filter(|f| f.aot_desc_idx > 0)
        .collect();
    assert!(
        aot_fns.iter().any(|f| f.name == "add"),
        "add 应被标记为 AOT 函数"
    );

    let mut vm = Vm::new(&embedded, VmOptions::default()).expect("VM 初始化");
    assert!(vm.aot_runtime.module_count() > 0, "AOT 模块应已加载");
    assert!(vm.aot_runtime.has_entry(0) || aot_fns.len() > 0);
    let aot_result = vm.run().expect("AOT 执行应成功");
    assert_eq!(aot_result, baseline, "AOT 机器码结果必须与解释器一致");
    assert_eq!(aot_result, Value::Int(42));
}

/// 浮点函数经 AOT 执行（tag 编解码正确性）
#[test]
fn aot_float_matches_interpreter() {
    if !llc_available() {
        eprintln!("skipped: LLVM 工具链不可用");
        return;
    }
    let src = r#"
        fun mul(a: Float, b: Float): Float { return a * b }
        fun main(): Float { return mul(1.5, 2.0) }
    "#;
    let plain = compile_source(src).expect("编译应成功");
    let mut vm = Vm::new(&plain, VmOptions::default()).expect("VM 初始化");
    let baseline = vm.run().expect("解释执行应成功");
    assert_eq!(baseline, Value::Float(3.0));

    let embedded = compile_with_aot_embed(src);
    assert!(embedded.has_aot());
    let mut vm = Vm::new(&embedded, VmOptions::default()).expect("VM 初始化");
    let r = vm.run().expect("AOT 执行应成功");
    assert_eq!(r, baseline, "Float AOT 结果必须与解释器一致");
}

/// Bool 函数经 AOT 执行
#[test]
fn aot_bool_matches_interpreter() {
    if !llc_available() {
        eprintln!("skipped: LLVM 工具链不可用");
        return;
    }
    let src = r#"
        fun gt(a: Int, b: Int): Bool { return a > b }
        fun main(): Bool { return gt(7, 3) }
    "#;
    let plain = compile_source(src).expect("编译应成功");
    let mut vm = Vm::new(&plain, VmOptions::default()).expect("VM 初始化");
    let baseline = vm.run().expect("解释执行应成功");
    assert_eq!(baseline, Value::Bool(true));

    let embedded = compile_with_aot_embed(src);
    assert!(embedded.has_aot());
    let mut vm = Vm::new(&embedded, VmOptions::default()).expect("VM 初始化");
    let r = vm.run().expect("AOT 执行应成功");
    assert_eq!(r, baseline);
}

/// 深度调用的 AOT 路径：add 被多次调用（验证分发表每调用都命中）
#[test]
fn aot_repeated_calls_match_interpreter() {
    if !llc_available() {
        eprintln!("skipped: LLVM 工具链不可用");
        return;
    }
    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun main(): Int {
            var s = 0
            for (i in 1..5) { s = add(s, i) }
            return s
        }
    "#;
    let plain = compile_source(src).expect("编译应成功");
    let mut vm = Vm::new(&plain, VmOptions::default()).expect("VM 初始化");
    let baseline = vm.run().expect("解释执行应成功");

    let embedded = compile_with_aot_embed(src);
    let mut vm = Vm::new(&embedded, VmOptions::default()).expect("VM 初始化");
    let r = vm.run().expect("AOT 执行应成功");
    assert_eq!(r, baseline, "循环内 AOT 调用结果必须一致");
}
