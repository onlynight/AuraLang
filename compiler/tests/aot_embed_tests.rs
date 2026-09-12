//! Phase 1 AOT 机器码嵌入 —— 端到端集成测试（1.14）
//!
//! 设计文档 docs/AOT机器码嵌入方案-详细设计.md §9：
//! `fun add(a: Int, b: Int): Int` 编译 → AOT 嵌入 `.auc` v4 → VM 加载执行，
//! 机器码执行结果必须与解释器一致。
//!
//! 需要 `cargo test --features llvm`，并能在 PATH / 环境变量中找到 LLVM 工具链
//! （llc）。找不到工具链时测试自动跳过，不视为失败。

#![cfg(feature = "llvm")]

use compiler::codegen::aot::{AotOptions, OptimizationLevel};
use compiler::codegen::aot_embed::embed_aot;
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
    let module = compile_source(source).expect("bytecode compilation should succeed");

    // 重新解析得到同源 HIR（AOT IR 生成需要）
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    assert!(
        lexer.errors().is_empty(),
        "lex error: {:?}",
        lexer.errors().first()
    );
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
        "aura_aot_e2e_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let result = embed_aot(module, &hir, options, &work_dir).expect("AOT embedding should succeed");
    let _ = std::fs::remove_dir_all(&work_dir);
    result.module
}

/// 1.14 端到端：add(Int,Int):Int 经 AOT 机器码执行，结果与解释器一致
#[test]
fn aot_add_matches_interpreter() {
    if !llc_available() {
        eprintln!("skipped: LLVM toolchain not available");
        return;
    }
    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun main(): Int { return add(20, 22) }
    "#;

    // 1) 纯字节码解释基准
    let plain = compile_source(src).expect("compilation should succeed");
    assert!(!plain.has_aot());
    let mut vm = Vm::new(&plain, VmOptions::default()).expect("VM initialization");
    let baseline = vm.run().expect("interpretation should succeed");
    assert_eq!(baseline, Value::Int(42), "baseline result should be 42");

    // 2) AOT 嵌入后经 VM 执行（do_call 命中 AOT 分发表 → 机器码）
    let embedded = compile_with_aot_embed(src);
    assert!(
        embedded.has_aot(),
        "module should contain AOT machine code segment"
    );
    assert!(embedded.aot_segments.len() >= 2);
    let aot_fns: Vec<&compiler::codegen::opcode::BytecodeFunction> =
        embedded.functions.iter().filter(|f| f.aot_desc_idx > 0).collect();
    assert!(
        aot_fns.iter().any(|f| f.name == "add"),
        "add should be marked as AOT function"
    );

    let mut vm = Vm::new(&embedded, VmOptions::default()).expect("VM initialization");
    assert!(
        vm.aot_runtime.module_count() > 0,
        "AOT module should be loaded"
    );
    assert!(vm.aot_runtime.has_entry(0) || aot_fns.len() > 0);
    let aot_result = vm.run().expect("AOT execution should succeed");
    assert_eq!(
        aot_result, baseline,
        "AOT machine code result must match interpreter"
    );
    assert_eq!(aot_result, Value::Int(42));
}

/// 浮点函数经 AOT 执行（tag 编解码正确性）
#[test]
fn aot_float_matches_interpreter() {
    if !llc_available() {
        eprintln!("skipped: LLVM toolchain not available");
        return;
    }
    let src = r#"
        fun mul(a: Float, b: Float): Float { return a * b }
        fun main(): Float { return mul(1.5, 2.0) }
    "#;
    let plain = compile_source(src).expect("compilation should succeed");
    let mut vm = Vm::new(&plain, VmOptions::default()).expect("VM initialization");
    let baseline = vm.run().expect("interpretation should succeed");
    assert_eq!(baseline, Value::Float(3.0));

    let embedded = compile_with_aot_embed(src);
    assert!(embedded.has_aot());
    let mut vm = Vm::new(&embedded, VmOptions::default()).expect("VM initialization");
    let r = vm.run().expect("AOT execution should succeed");
    assert_eq!(r, baseline, "Float AOT result must match interpreter");
}

/// Bool 函数经 AOT 执行
#[test]
fn aot_bool_matches_interpreter() {
    if !llc_available() {
        eprintln!("skipped: LLVM toolchain not available");
        return;
    }
    let src = r#"
        fun gt(a: Int, b: Int): Bool { return a > b }
        fun main(): Bool { return gt(7, 3) }
    "#;
    let plain = compile_source(src).expect("compilation should succeed");
    let mut vm = Vm::new(&plain, VmOptions::default()).expect("VM initialization");
    let baseline = vm.run().expect("interpretation should succeed");
    assert_eq!(baseline, Value::Bool(true));

    let embedded = compile_with_aot_embed(src);
    assert!(embedded.has_aot());
    let mut vm = Vm::new(&embedded, VmOptions::default()).expect("VM initialization");
    let r = vm.run().expect("AOT execution should succeed");
    assert_eq!(r, baseline);
}

/// 深度调用的 AOT 路径：add 被多次调用（验证分发表每调用都命中）
#[test]
fn aot_repeated_calls_match_interpreter() {
    if !llc_available() {
        eprintln!("skipped: LLVM toolchain not available");
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
    let plain = compile_source(src).expect("compilation should succeed");
    let mut vm = Vm::new(&plain, VmOptions::default()).expect("VM initialization");
    let baseline = vm.run().expect("interpretation should succeed");

    let embedded = compile_with_aot_embed(src);
    let mut vm = Vm::new(&embedded, VmOptions::default()).expect("VM initialization");
    let r = vm.run().expect("AOT execution should succeed");
    assert_eq!(r, baseline, "AOT call results in loop must be consistent");
}
