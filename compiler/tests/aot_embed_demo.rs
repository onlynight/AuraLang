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
    if std::env::var_os("AURA_LLVM_HOME").is_some() { return true; }
    let paths = std::env::var("PATH").unwrap_or_default();
    for dir in paths.split(';') {
        if std::path::Path::new(dir).join("llc.exe").is_file() { return true; }
        if std::path::Path::new(dir).join("llc").is_file() { return true; }
    }
    false
}

/// 完整端到端演示：源码 → AOT 嵌入 → 签名 → VM 执行
#[test]
fn demo_end_to_end_aot_embed() {
    if !llc_available() { eprintln!("skipped: LLVM 不可用"); return; }

    println!("=== Aura AOT 机器码嵌入演示 ===\n");

    // Step 1: 编译源码
    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun multiply(a: Int, b: Int): Int { return a * b }
        fun main(): Int { return add(multiply(6, 7), 2) }
    "#;

    println!("Step 1: 编译源码 → 字节码...");
    let module = compile_source(src).expect("编译失败");
    println!("  ✓ 字节码编译成功: {} 个函数", module.functions.len());

    // Step 2: 重新解析得到 HIR
    println!("\nStep 2: 解析 HIR...");
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    let hir = desugar_program(&program);
    println!("  ✓ HIR 解析成功");

    // Step 3: AOT 嵌入
    println!("\nStep 3: AOT 嵌入（LLVM IR → 机器码 blob）...");
    let options = AotOptions { opt_level: OptimizationLevel::default(), ..Default::default() };
    let work_dir = std::env::temp_dir().join(format!(
        "aura_demo_{}_{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let result = embed_aot(module, &hir, options, &work_dir).expect("AOT 嵌入失败");
    let _ = std::fs::remove_dir_all(&work_dir);

    println!("  ✓ AOT 嵌入成功: 机器码 {} 字节, {} 个函数描述符",
        result.machine_size, result.desc_count);
    println!("  ✓ 段表: {} 个段 (含字符串池)", result.module.aot_segments.len());

    // Step 4: 基线执行（纯字节码解释）
    println!("\nStep 4: 基线执行（纯字节码解释器）...");
    let plain = compile_source(src).unwrap();
    let mut vm = Vm::new(&plain, VmOptions::default()).unwrap();
    let baseline = vm.run().unwrap();
    println!("  ✓ 基线结果: {}", format_value(&baseline));

    // Step 5: AOT 执行
    println!("\nStep 5: AOT 机器码执行...");
    let mut vm = Vm::new(&result.module, VmOptions::default()).expect("VM 初始化失败");
    let aot_result = vm.run().expect("AOT 执行失败");
    println!("  ✓ AOT 结果: {}", format_value(&aot_result));
    assert_eq!(aot_result, baseline, "AOT 结果必须与解释器一致");
    println!("  ✓ 结果一致!");

    // Step 6: 签名
    println!("\nStep 6: Ed25519 签名...");
    let auc_bytes = serialize::to_bytes(&result.module);
    println!("  原始 .auc 大小: {} 字节", auc_bytes.len());

    use compiler::codegen::serialize::Ed25519Keypair;
    let keypair = Ed25519Keypair::generate();
    let signed_bytes = serialize::sign_auc(&auc_bytes, &keypair);
    println!("  签名后大小: {} 字节 (增加 96 字节: 64 签名 + 32 公钥)", signed_bytes.len());

    // Step 7: 验证签名
    println!("\nStep 7: 验证签名...");
    let pk = keypair.public_key.as_bytes().to_vec();
    let pk_bytes: [u8; 32] = pk.as_slice().try_into().unwrap();
    let valid = serialize::verify_auc_signature(&signed_bytes, &pk_bytes).unwrap();
    println!("  ✓ 签名验证: {}", if valid { "通过" } else { "失败" });
    assert!(valid);

    // Step 8: 签名保护下 VM 加载
    println!("\nStep 8: 签名保护下 VM 加载执行...");
    let loaded = serialize::from_bytes(&signed_bytes[..signed_bytes.len() - 96]).unwrap();
    let mut vm = Vm::new(&loaded, VmOptions::default()).unwrap();
    let result = vm.run().unwrap();
    println!("  ✓ 签名后执行结果: {}", format_value(&result));
    assert_eq!(result, baseline);

    // Step 9: 诊断信息
    println!("\nStep 9: AOT 模块诊断...");
    if let Some(diag) = vm.aot_runtime.all_diagnostics().first() {
        println!("  模块 ID: {}", diag.module_id);
        println!("  模块名称: {}", diag.name);
        println!("  代码基地址: 0x{:x}", diag.code_base);
        println!("  函数数量: {}", diag.func_count);
        println!("  分发表条目: {}", diag.dispatch_count);
    }

    println!("\n=== 演示完成 ===");
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