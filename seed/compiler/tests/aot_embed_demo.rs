//! Phase 2/3 Demo: 端到端 AOT 机器码嵌入演示
//!
//! 演示完整流程：源码 → 字节码 → AOT 嵌入 → Ed25519 签名 → VM 加载执行
//! 需要 `cargo test --features llvm`。

#![cfg(feature = "llvm")]

use compiler::codegen::aot::{AotOptions, OptimizationLevel};
use compiler::codegen::aot_embed::embed_aot;
use compiler::codegen::hir::desugar_program;
use compiler::codegen::serialize;
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
        if std::path::Path::new(dir).join("llc.exe").is_file() {
            return true;
        }
        if std::path::Path::new(dir).join("llc").is_file() {
            return true;
        }
    }
    false
}

/// 完整端到端演示：源码 → AOT 嵌入 → 签名 → VM 执行
#[test]
fn demo_end_to_end_aot_embed() {
    if !llc_available() {
        eprintln!("skipped: LLVM not available");
        return;
    }

    println!("=== Aura AOT machine code embedding demo ===\n");

    // Step 1: 编译源码
    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun multiply(a: Int, b: Int): Int { return a * b }
        fun main(): Int { return add(multiply(6, 7), 2) }
    "#;

    println!("Step 1: Compile source -> bytecode...");
    let module = compile_source(src).expect("compilation failed");
    println!(
        "  * bytecode compilation succeeded: {} functions",
        module.functions.len()
    );

    // Step 2: 重新解析得到 HIR
    println!("\nStep 2: Parse HIR...");
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    let hir = desugar_program(&program);
    println!("  * HIR parsed successfully");

    // Step 3: AOT 嵌入
    println!("\nStep 3: AOT embedding (LLVM IR -> machine code blob)...");
    let options = AotOptions {
        opt_level: OptimizationLevel::default(),
        ..Default::default()
    };
    let work_dir = std::env::temp_dir().join(format!(
        "aura_demo_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let result = embed_aot(module, &hir, options, &work_dir).expect("AOT embedding failed");
    let _ = std::fs::remove_dir_all(&work_dir);

    println!(
        "  * AOT embedding succeeded: machine code {} bytes, {} function descriptors",
        result.machine_size, result.desc_count
    );
    println!(
        "  * segment table: {} segments (incl. string pool)",
        result.module.aot_segments.len()
    );

    // Step 4: 基线执行（纯字节码解释）
    println!("\nStep 4: Baseline execution (pure bytecode interpreter)...");
    let plain = compile_source(src).unwrap();
    let mut vm = Vm::new(&plain, VmOptions::default()).unwrap();
    let baseline = vm.run().unwrap();
    println!("  * baseline result: {}", format_value(&baseline));

    // Step 5: AOT 执行
    println!("\nStep 5: AOT machine code execution...");
    let mut vm = Vm::new(&result.module, VmOptions::default()).expect("VM initialization failed");
    let aot_result = vm.run().expect("AOT execution failed");
    println!("  * AOT result: {}", format_value(&aot_result));
    assert_eq!(aot_result, baseline, "AOT result must match interpreter");
    println!("  * Results match!");

    // Step 6: 签名
    println!("\nStep 6: Ed25519 signing...");
    let auc_bytes = serialize::to_bytes(&result.module);
    println!("  Original .auc size: {} bytes", auc_bytes.len());

    use compiler::codegen::serialize::Ed25519Keypair;
    let keypair = Ed25519Keypair::generate();
    let signed_bytes = serialize::sign_auc(&auc_bytes, &keypair);
    println!(
        "  Signed size: {} bytes (added 96 bytes: 64 signature + 32 public key)",
        signed_bytes.len()
    );

    // Step 7: 验证签名
    println!("\nStep 7: Verify signature...");
    let pk = keypair.public_key.as_bytes().to_vec();
    let pk_bytes: [u8; 32] = pk.as_slice().try_into().unwrap();
    let valid = serialize::verify_auc_signature(&signed_bytes, &pk_bytes).unwrap();
    println!(
        "  * Signature verification: {}",
        if valid { "passed" } else { "failed" }
    );
    assert!(valid);

    // Step 8: 签名保护下 VM 加载
    println!("\nStep 8: VM load and execute under signature protection...");
    let loaded = serialize::from_bytes(&signed_bytes[..signed_bytes.len() - 96]).unwrap();
    let mut vm = Vm::new(&loaded, VmOptions::default()).unwrap();
    let result = vm.run().unwrap();
    println!("  * Signed execution result: {}", format_value(&result));
    assert_eq!(result, baseline);

    // Step 9: 诊断信息
    println!("\nStep 9: AOT module diagnostics...");
    if let Some(diag) = vm.aot_runtime.all_diagnostics().first() {
        println!("  Module ID: {}", diag.module_id);
        println!("  Module name: {}", diag.name);
        println!("  Code base address: 0x{:x}", diag.code_base);
        println!("  Function count: {}", diag.func_count);
        println!("  Dispatch table entries: {}", diag.dispatch_count);
    }

    println!("\n=== Demo complete ===");
}

fn format_value(v: &Value) -> String {
    match v {
        Value::Int(i) => format!("Int({})", i),
        Value::Float(f) => format!("Float({})", f),
        Value::Bool(b) => format!("Bool({})", b),
        Value::Null => "Null".to_string(),
        _ => "Other".to_string(),
    }
}
