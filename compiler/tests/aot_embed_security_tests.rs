//! Phase 3.8: 安全与热重载集成测试
//! 需要 `cargo test --features llvm`。

#![cfg(feature = "llvm")]

use compiler::codegen::aot::{AotOptions, OptimizationLevel};
use compiler::codegen::aot_embed::embed_aot;
use compiler::codegen::hir::desugar_program;
use compiler::codegen::opcode::{
    AucSegment, AuraFuncDesc, BytecodeModule, SEG_DESC_TABLE, SEG_MACHINE, SEG_PROT_EXEC,
    SEG_PROT_READ,
};
use compiler::codegen::serialize;
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::vm::aot_runtime::{AotRuntime, AOT_MAX_CALL_DEPTH};
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

fn compile_with_aot_embed(source: &str) -> BytecodeModule {
    let module = compiler::codegen::compile_source(source).expect("bytecode compile failed");
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    let hir = desugar_program(&program);
    let options = AotOptions { opt_level: OptimizationLevel::default(), ..Default::default() };
    let work_dir = std::env::temp_dir().join(format!(
        "aura_aot_sec_{}_{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let result = embed_aot(module, &hir, options, &work_dir).expect("AOT embed failed");
    let _ = std::fs::remove_dir_all(&work_dir);
    result.module
}

#[test]
fn test_ed25519_sign_and_verify() {
    use compiler::codegen::serialize::Ed25519Keypair;
    let keypair = Ed25519Keypair::generate();
    let data = b"hello world .auc content";
    let signed = serialize::sign_auc(data, &keypair);
    assert_eq!(signed.len(), data.len() + 96);
    let pk = keypair.public_key.as_bytes().to_vec();
    let pk_bytes: [u8; 32] = pk.as_slice().try_into().unwrap();
    assert!(serialize::verify_auc_signature(&signed, &pk_bytes).unwrap());
    let mut tampered = signed.clone();
    tampered[0] ^= 0xFF;
    assert!(!serialize::verify_auc_signature(&tampered, &pk_bytes).unwrap());
    let key_bytes = keypair.to_bytes();
    assert_eq!(key_bytes.len(), 64);
    let keypair2 = Ed25519Keypair::from_bytes(&key_bytes).unwrap();
    let signed2 = serialize::sign_auc(data, &keypair2);
    assert!(serialize::verify_auc_signature(&signed2, &pk_bytes).unwrap());
}

#[test]
fn test_hot_reload_module() {
    let descs1 = vec![AuraFuncDesc { entry_offset: 0x100, num_args: 1, flags: 1, ..AuraFuncDesc::default() }];
    let (data1, segs1) = make_segment_data(256, &descs1);
    let mut rt = AotRuntime::new();
    let id1 = rt.load_module_from(&data1, &segs1, &[1u32], "module_v1".to_string()).unwrap();
    assert_eq!(rt.module_count(), 1);
    let descs2 = vec![AuraFuncDesc { entry_offset: 0x200, num_args: 2, flags: 1, ..AuraFuncDesc::default() }];
    let (data2, segs2) = make_segment_data(256, &descs2);
    let id2 = rt.hot_reload_module(id1, &data2, &segs2, &[1u32], "module_v2".to_string()).unwrap();
    assert_ne!(id1, id2);
    assert_eq!(rt.module_count(), 1);
    assert!(rt.has_entry(0));
    let modules = rt.list_modules();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].1, "module_v2");
}

#[test]
fn test_hot_reload_nonexistent() {
    let descs = vec![AuraFuncDesc { entry_offset: 0x100, ..AuraFuncDesc::default() }];
    let (data, segs) = make_segment_data(256, &descs);
    let mut rt = AotRuntime::new();
    let err = rt.hot_reload_module(999, &data, &segs, &[1u32], "test".to_string()).unwrap_err();
    assert!(err.contains("not loaded"));
}

#[test]
fn test_call_depth_limit() {
    let rt = AotRuntime::new();
    assert!(rt.check_call_depth(AOT_MAX_CALL_DEPTH));
    assert!(rt.check_call_depth(1));
    assert!(!rt.check_call_depth(AOT_MAX_CALL_DEPTH + 1));
    assert!(AOT_MAX_CALL_DEPTH >= 1024);
}

#[test]
fn test_entry_cache() {
    let descs = vec![AuraFuncDesc { entry_offset: 0x100, ..AuraFuncDesc::default() }];
    let (data, segs) = make_segment_data(256, &descs);
    let mut rt = AotRuntime::new();
    let id = rt.load_module_from(&data, &segs, &[1u32], "cache_test".to_string()).unwrap();
    let cached = rt.cached_find_entry(0);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().0, id);
    rt.clear_entry_cache();
    let cached2 = rt.cached_find_entry(0);
    assert!(cached2.is_some());
}

#[test]
fn test_module_diagnostics() {
    let descs = vec![
        AuraFuncDesc { entry_offset: 0x100, num_args: 2, flags: 1, ..AuraFuncDesc::default() },
        AuraFuncDesc { entry_offset: 0x200, num_args: 1, flags: 1, ..AuraFuncDesc::default() },
    ];
    let (data, segs) = make_segment_data(512, &descs);
    let mut rt = AotRuntime::new();
    let id = rt.load_module_from(&data, &segs, &[1u32, 2], "diag_test".to_string()).unwrap();
    let diag = rt.module_diagnostics(id).unwrap();
    assert_eq!(diag.module_id, id);
    assert_eq!(diag.name, "diag_test");
    assert!(diag.is_loaded);
    assert!(diag.code_base != 0);
    assert_eq!(diag.func_count, 2);
    assert_eq!(diag.dispatch_count, 2);
    let all_diag = rt.all_diagnostics();
    assert_eq!(all_diag.len(), 1);
}

#[test]
fn test_signed_auc_load() {
    if !llc_available() { eprintln!("skipped"); return; }
    use compiler::codegen::serialize::Ed25519Keypair;
    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun main(): Int { return add(3, 4) }
    "#;
    let embedded = compile_with_aot_embed(src);
    let auc_bytes = serialize::to_bytes(&embedded);
    let keypair = Ed25519Keypair::generate();
    let signed_bytes = serialize::sign_auc(&auc_bytes, &keypair);
    let pk = keypair.public_key.as_bytes().to_vec();
    let pk_bytes: [u8; 32] = pk.as_slice().try_into().unwrap();
    assert!(serialize::verify_auc_signature(&signed_bytes, &pk_bytes).unwrap());
    let loaded = serialize::from_bytes(&signed_bytes[..signed_bytes.len() - 96]).unwrap();
    assert!(loaded.has_aot());
    let mut vm = Vm::new(&loaded, VmOptions::default()).expect("VM init");
    let result = vm.run().expect("run");
    assert_eq!(result, Value::Int(7));
}

fn make_segment_data(machine_size: usize, descs: &[AuraFuncDesc]) -> (Vec<u8>, Vec<AucSegment>) {
    let mut data = Vec::new();
    data.resize(machine_size, 0x90);
    let machine = AucSegment { id: SEG_MACHINE, offset: 0, size: machine_size as u32, flags: SEG_PROT_READ | SEG_PROT_EXEC };
    let mut desc_bytes = Vec::new();
    for d in descs {
        let slice = unsafe { std::slice::from_raw_parts(d as *const AuraFuncDesc as *const u8, AuraFuncDesc::SIZE) };
        desc_bytes.extend_from_slice(slice);
    }
    let desc_seg = AucSegment { id: SEG_DESC_TABLE, offset: data.len() as u32, size: desc_bytes.len() as u32, flags: SEG_PROT_READ };
    data.extend_from_slice(&desc_bytes);
    (data, vec![machine, desc_seg])
}