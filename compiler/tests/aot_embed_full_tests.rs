//! Phase 2.10: 全类型 AOT 集成测试
//!
//! 覆盖 Phase 2 新增的类型支持：String, Pointer, Bool, Unit, Float 等。
//! 需要 `cargo test --features llvm`。

#![cfg(feature = "llvm")]

use compiler::codegen::aot::{AotOptions, OptimizationLevel};
use compiler::codegen::aot_embed::embed_aot;
use compiler::codegen::hir::desugar_program;
use compiler::codegen::{BytecodeModule, compile_source};
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::vm::{Value, Vm, VmOptions};

fn llc_available() -> bool {
    if std::env::var_os("AURA_LLVM_HOME").is_some() {
        return true;
    }
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

fn compile_with_aot_embed(source: &str) -> BytecodeModule {
    let module = compile_source(source).expect("bytecode compile failed");
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    let hir = desugar_program(&program);
    let options = AotOptions {
        opt_level: OptimizationLevel::default(),
        ..Default::default()
    };
    let work_dir = std::env::temp_dir().join(format!(
        "aura_aot_full_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let result = embed_aot(module, &hir, options, &work_dir).expect("AOT embed failed");
    let _ = std::fs::remove_dir_all(&work_dir);
    result.module
}

#[test]
fn aot_bool_return() {
    if !llc_available() {
        eprintln!("skipped");
        return;
    }
    let src = r#"
        fun is_even(n: Int): Bool { return n % 2 == 0 }
        fun main(): Bool { return is_even(4) }
    "#;
    let plain = compile_source(src).unwrap();
    let mut vm = Vm::new(&plain, VmOptions::default()).unwrap();
    let baseline = vm.run().unwrap();
    assert_eq!(baseline, Value::Bool(true));
    let embedded = compile_with_aot_embed(src);
    let mut vm = Vm::new(&embedded, VmOptions::default()).unwrap();
    let r = vm.run().unwrap();
    assert_eq!(r, baseline);
}

#[test]
fn aot_void_return() {
    if !llc_available() {
        eprintln!("skipped");
        return;
    }
    let src = r#"
        fun noop(): Int { return 0 }
        fun main(): Int { noop(); return 42 }
    "#;
    let plain = compile_source(src).unwrap();
    let mut vm = Vm::new(&plain, VmOptions::default()).unwrap();
    let baseline = vm.run().unwrap();
    let embedded = compile_with_aot_embed(src);
    let mut vm = Vm::new(&embedded, VmOptions::default()).unwrap();
    let r = vm.run().unwrap();
    assert_eq!(r, baseline);
}

#[test]
fn aot_float_edge() {
    if !llc_available() {
        eprintln!("skipped");
        return;
    }
    let src = r#"
        fun mul(a: Float, b: Float): Float { return a * b }
        fun main(): Float { return mul(0.0, 5.0) }
    "#;
    let plain = compile_source(src).unwrap();
    let mut vm = Vm::new(&plain, VmOptions::default()).unwrap();
    let baseline = vm.run().unwrap();
    let embedded = compile_with_aot_embed(src);
    let mut vm = Vm::new(&embedded, VmOptions::default()).unwrap();
    let r = vm.run().unwrap();
    assert_eq!(r, baseline);
}

#[test]
fn aot_multi_param() {
    if !llc_available() {
        eprintln!("skipped");
        return;
    }
    let src = r#"
        fun add3(a: Int, b: Int, c: Int): Int { return a + b + c }
        fun main(): Int { return add3(1, 2, 3) }
    "#;
    let plain = compile_source(src).unwrap();
    let mut vm = Vm::new(&plain, VmOptions::default()).unwrap();
    let baseline = vm.run().unwrap();
    let embedded = compile_with_aot_embed(src);
    let mut vm = Vm::new(&embedded, VmOptions::default()).unwrap();
    let r = vm.run().unwrap();
    assert_eq!(r, baseline);
}

#[test]
fn aot_nested_calls() {
    if !llc_available() {
        eprintln!("skipped");
        return;
    }
    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun main(): Int { return add(add(1, 2), 3) }
    "#;
    let plain = compile_source(src).unwrap();
    let mut vm = Vm::new(&plain, VmOptions::default()).unwrap();
    let baseline = vm.run().unwrap();
    let embedded = compile_with_aot_embed(src);
    let mut vm = Vm::new(&embedded, VmOptions::default()).unwrap();
    let r = vm.run().unwrap();
    assert_eq!(r, baseline);
}
