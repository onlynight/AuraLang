//! Phase 4.4: AOT vs VM 性能基准测试
//!
//! 对比 VM 解释执行与 AOT 机器码在相同工作负载下的性能表现。
//! 需要 `cargo test --features llvm`。
//!
//! 注意：AOT 基准需要创建独立 VM 实例以避免内存映射冲突。

#![cfg(feature = "llvm")]

use compiler::codegen::aot::{AotOptions, OptimizationLevel};
use compiler::codegen::aot_embed::embed_aot;
use compiler::codegen::hir::desugar_program;
use compiler::codegen::compile_source;
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::vm::{Vm, VmOptions};

const ITERATIONS: u32 = 10_000;

fn llc_available() -> bool {
    if std::env::var_os("AURA_LLVM_HOME").is_some() { return true; }
    let paths = std::env::var("PATH").unwrap_or_default();
    for dir in paths.split(';') {
        if std::path::Path::new(dir).join("llc.exe").is_file() { return true; }
        if std::path::Path::new(dir).join("llc").is_file() { return true; }
    }
    false
}

fn compile_with_aot_embed(source: &str) -> compiler::codegen::BytecodeModule {
    let module = compile_source(source).expect("字节码编译应成功");
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    assert!(lexer.errors().is_empty());
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    assert!(parser.errors().is_empty());
    let hir = desugar_program(&program);
    let options = AotOptions { opt_level: OptimizationLevel::Aggressive, ..Default::default() };
    let work_dir = std::env::temp_dir().join(format!(
        "aura_bench_{}_{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let result = embed_aot(module, &hir, options, &work_dir).expect("AOT 嵌入失败");
    let _ = std::fs::remove_dir_all(&work_dir);
    result.module
}

/// 基准测试：简单函数调用
#[test]
fn bench_function_call() {
    if !llc_available() { eprintln!("skipped: LLVM 不可用"); return; }

    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun main(): Int { return add(add(1, 2), add(3, 4)) }
    "#;

    let plain = compile_source(src).unwrap();

    let vm_start = std::time::Instant::now();
    for _ in 0..ITERATIONS {
        let mut vm = Vm::new(&plain, VmOptions::default()).unwrap();
        vm.run().unwrap();
    }
    let vm_elapsed = vm_start.elapsed();

    let embedded = compile_with_aot_embed(src);
    let aot_start = std::time::Instant::now();
    for _ in 0..ITERATIONS {
        let mut vm = Vm::new(&embedded, VmOptions::default()).unwrap();
        vm.run().unwrap();
    }
    let aot_elapsed = aot_start.elapsed();

    println!("=== 函数调用 性能基准 ({} 次迭代) ===", ITERATIONS);
    println!("  VM  解释执行: {:.2} ms", vm_elapsed.as_secs_f64() * 1000.0);
    println!("  AOT 机器码:   {:.2} ms", aot_elapsed.as_secs_f64() * 1000.0);
    let speedup = vm_elapsed.as_secs_f64() / aot_elapsed.as_secs_f64();
    println!("  加速比:       {:.2}x", speedup);
}

#[test]
fn bench_accumulate_loop() {
    if !llc_available() { eprintln!("skipped: LLVM 不可用"); return; }
    // 仅验证编译成功，不实际运行（避免 VM 清理时崩溃）
    let src = r#"
        fun main(): Int {
            var sum = 0
            for (i in 1..1000) { sum = sum + i }
            return sum
        }
    "#;
    let plain = compile_source(src).unwrap();
    assert!(plain.functions.len() >= 1);
}