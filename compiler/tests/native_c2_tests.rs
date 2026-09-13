//! Phase C.2 测试：Rust @native 发射器三缺口验证
//!
//! 对应改造方案 C.2「Rust @native 发射器三缺口」：
//!   缺口 1: @native(asm) 内联汇编泛化发射
//!   缺口 2: Memory.aura builtin load/store 类型修正
//!   缺口 3: void 返回 ret 指令格式
//!
//! 运行：cargo test -p compiler --test native_c2_tests
//!
//! 需要 `--features llvm` 才能执行完整编译测试。

#![cfg(feature = "llvm")]

use compiler::codegen::aot::{AotCodeGenerator, AotOptions, aot_compile};
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use std::fs;
use std::path::PathBuf;

/// 解析源码为 AST
fn parse(src: &str) -> compiler::ast::Program {
    let mut lexer = Lexer::new(src);
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
        "parse error: {:?}",
        parser.errors().first()
    );
    program
}

/// 生成 LLVM IR 文本
fn gen_ir(src: &str) -> String {
    let program = parse(src);
    let hir = desugar_program(&program);
    let codegen = AotCodeGenerator::new(AotOptions::default());
    codegen.generate_ir(&hir).expect("IR generation failed")
}

/// 读取测试 .aura 文件
fn read_test_file(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/pure_aura")
        .join(name);
    fs::read_to_string(&path)
        .expect(format!("Failed to read test file: {}", path.display()).as_str())
}

/// 检查 LLVM 工具链是否可用
fn llvm_available() -> bool {
    if std::env::var_os("AURA_LLVM_HOME").is_some() {
        return true;
    }
    let paths = std::env::var("PATH").unwrap_or_default();
    for dir in paths.split(';') {
        if PathBuf::from(dir).join("llc.exe").is_file() {
            return true;
        }
        if PathBuf::from(dir).join("llc").is_file() {
            return true;
        }
    }
    false
}

// ═══════════════════════════════════════════════════════════════════
// 缺口 1：@native(asm) 内联汇编泛化发射
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_asm_rdtsc_uses_c_wrapper() {
    // 已知指令 rdtsc 应走 C 封装路径
    let src = r#"
        @native(asm = "rdtsc")
        fun rdtsc(): Long { }
        fun main(): Int { rdtsc(); return 0 }
    "#;
    let program = parse(src);
    let hir = desugar_program(&program);

    // Debug: check where the function ended up
    eprintln!("=== HIR debug ===");
    eprintln!("natives count: {}", hir.natives.len());
    for f in &hir.natives {
        eprintln!(
            "  native: name={}, is_native={}, native_attr={:?}",
            f.name, f.is_native, f.native_attr
        );
    }
    eprintln!("functions count: {}", hir.functions.len());
    for f in &hir.functions {
        eprintln!(
            "  func: name={}, is_native={}, native_attr={:?}",
            f.name, f.is_native, f.native_attr
        );
    }
    eprintln!("=== End debug ===");

    let ir = gen_ir(src);
    let rdtsc_lines: Vec<&str> = ir
        .lines()
        .filter(|l| {
            l.contains("rdtsc")
                || l.contains("asm sideeffect")
                || l.contains("aura_cpu")
                || l.contains("define")
        })
        .collect();
    eprintln!("=== IR debug ===");
    eprintln!("{}", rdtsc_lines.join("\n"));
    eprintln!("=== End debug ===");
    assert!(
        ir.contains("call i64 @aura_cpu_rdtsc"),
        "rdtsc should call C wrapper aura_cpu_rdtsc"
    );
}

#[test]
fn test_asm_fence_uses_c_wrapper() {
    // 已知指令 mfence 应走 C 封装路径
    let src = r#"
        @native(asm = "mfence")
        fun memFence(): Void { }
        fun main(): Int { memFence(); return 0 }
    "#;
    let ir = gen_ir(src);
    assert!(
        ir.contains("call void @aura_cpu_mem_fence"),
        "mfence should call C wrapper aura_cpu_mem_fence"
    );
}

#[test]
fn test_asm_generic_uses_inline_asm() {
    // 通用内联汇编应走 LLVM inline asm 路径
    let src = r#"
        @native(asm = "cpuid")
        fun cpuid(): Long { }
        fun main(): Int { cpuid(); return 0 }
    "#;
    let ir = gen_ir(src);
    assert!(
        ir.contains("asm sideeffect"),
        "generic asm should use LLVM inline asm, got IR excerpt:\n{}",
        ir.lines()
            .filter(|l| l.contains("cpuid") || l.contains("asm"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        ir.contains("cpuid"),
        "inline asm should contain the asm code string"
    );
    // x86 目标的内联汇编按 Intel 语法解析（仓库自带 FFI 片段即 Intel 写法，
    // 如 `call qword [rip + WriteFile]`），llc 为输出前缀 `.intel_syntax noprefix`。
    assert!(
        ir.contains(".intel_syntax noprefix"),
        "x86 inline asm should be parsed as Intel syntax"
    );
    // LLVM asm-call 必须带参数表 `()`，且约束数要与返回值一致
    assert!(
        ir.contains("asm sideeffect") && ir.contains("\"=r\"()"),
        "inline asm call should carry an empty argument list with '=r' constraint"
    );
}

#[test]
fn test_asm_generic_void_return() {
    // void 返回的内联汇编不应有 %result
    let src = r#"
        @native(asm = "pause")
        fun cpuPause(): Void { }
        fun main(): Int { cpuPause(); return 0 }
    "#;
    let ir = gen_ir(src);
    assert!(
        ir.contains("call void asm sideeffect"),
        "void asm should use 'call void asm sideeffect'"
    );
    // void 返回不应有 ret i64
    let pause_fn = ir.lines().filter(|l| l.contains("@cpuPause")).collect::<Vec<_>>().join("\n");
    assert!(
        !pause_fn.contains("ret i64"),
        "void asm function should not return i64, got:\n{}",
        pause_fn
    );
}

#[test]
fn test_asm_atomic_uses_c_wrapper() {
    // atomic 指令应走 C 封装路径
    let src = r#"
        @native(asm = "lock xadd [rax], rdx")
        fun atomicAdd(addr: Long, delta: Long): Long { }
        fun main(): Int { atomicAdd(0, 1); return 0 }
    "#;
    let ir = gen_ir(src);
    assert!(
        ir.contains("call i64 @aura_cpu_atomic_add"),
        "atomic asm should call C wrapper aura_cpu_atomic_add"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 缺口 2：Memory builtin load/store 类型修正
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_memory_alloc_uses_c_wrapper() {
    let src = r#"
        @native fun MemoryAlloc(n: Long): Long { }
        fun main(): Int { MemoryAlloc(64); return 0 }
    "#;
    let program = parse(src);
    let hir = desugar_program(&program);

    // Debug: check HIR
    eprintln!("=== HIR debug ===");
    eprintln!("functions count: {}", hir.functions.len());
    for f in &hir.functions {
        eprintln!(
            "  func: name={}, is_native={}, native_attr={:?}",
            f.name, f.is_native, f.native_attr
        );
    }
    eprintln!("=== End debug ===");

    let ir = gen_ir(src);
    assert!(
        ir.contains("call i64 @aura_memory_alloc"),
        "MemoryAlloc should call C wrapper aura_memory_alloc"
    );
}

#[test]
fn test_memory_free_uses_c_wrapper() {
    let src = r#"
        @native fun MemoryFree(addr: Long): Void { }
        fun main(): Int { MemoryFree(0); return 0 }
    "#;
    let ir = gen_ir(src);
    assert!(
        ir.contains("call void @aura_memory_free"),
        "MemoryFree should call C wrapper aura_memory_free"
    );
}

#[test]
fn test_memory_read_uses_inttoptr() {
    // Memory.read 应使用 inttoptr 转换地址，再 load
    let src = r#"
        @native fun MemoryRead(addr: Long): Byte { }
        fun main(): Int { MemoryRead(0); return 0 }
    "#;
    let ir = gen_ir(src);
    assert!(
        ir.contains("inttoptr i64"),
        "MemoryRead should use inttoptr to convert i64 to i8*"
    );
    assert!(
        ir.contains("load i8, i8*"),
        "MemoryRead should load i8 from i8*"
    );
}

#[test]
fn test_memory_write_uses_inttoptr() {
    // Memory.write 应使用 inttoptr 转换地址，再 store
    let src = r#"
        @native fun MemoryWrite(addr: Long, value: Byte): Void { }
        fun main(): Int { MemoryWrite(0, 42); return 0 }
    "#;
    let ir = gen_ir(src);
    assert!(
        ir.contains("inttoptr i64"),
        "MemoryWrite should use inttoptr to convert i64 to i8*"
    );
    assert!(ir.contains("store i8"), "MemoryWrite should store i8");
}

// ═══════════════════════════════════════════════════════════════════
// 缺口 3：void 返回 ret 指令格式
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_void_return_format() {
    // void 返回的 @native 函数应生成正确的 ret void
    let src = r#"
        @native fun MemoryFree(addr: Long): Void { }
        fun main(): Int { MemoryFree(0); return 0 }
    "#;
    let ir = gen_ir(src);
    // 检查 MemoryFree 函数定义包含 ret void
    let free_lines: Vec<&str> =
        ir.lines().filter(|l| l.contains("@MemoryFree") || l.contains("ret void")).collect();
    assert!(
        free_lines.iter().any(|l| l.contains("ret void")),
        "MemoryFree should contain 'ret void', got:\n{}",
        free_lines.join("\n")
    );
}

// ═══════════════════════════════════════════════════════════════════
// 综合测试：从 .aura 文件加载
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_asm_aura_file_ir_generation() {
    let src = read_test_file("native_c2_asm.aura");
    let ir = gen_ir(&src);
    // 应包含 C 封装调用
    assert!(
        ir.contains("@aura_cpu_rdtsc"),
        "should contain aura_cpu_rdtsc"
    );
    assert!(
        ir.contains("@aura_cpu_mem_fence"),
        "should contain aura_cpu_mem_fence"
    );
    // 应包含 inline asm
    assert!(ir.contains("asm sideeffect"), "should contain inline asm");
}

#[test]
fn test_memory_aura_file_ir_generation() {
    let src = read_test_file("native_c2_memory.aura");
    let ir = gen_ir(&src);
    // 应包含 C 封装调用
    assert!(
        ir.contains("@aura_memory_alloc"),
        "should contain aura_memory_alloc"
    );
    assert!(
        ir.contains("@aura_memory_free"),
        "should contain aura_memory_free"
    );
    // 应包含 inttoptr + load/store
    assert!(
        ir.contains("inttoptr i64"),
        "should contain inttoptr for address conversion"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 完整编译验证（需要 LLVM 工具链）
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_asm_aura_file_aot_compile() {
    if !llvm_available() {
        eprintln!("SKIP: LLVM not available");
        return;
    }
    let src = read_test_file("native_c2_asm.aura");
    let output = std::env::temp_dir().join("aura_c2_asm_test");
    let result = aot_compile(&src, &output, AotOptions::default());
    match result {
        Ok(aot_output) => {
            let out = aot_output.exe_path.unwrap_or(
                output.with_extension(if cfg!(target_os = "windows") { "exe" } else { "" }),
            );
            let output = std::process::Command::new(&out).output().expect("failed to run asm test");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success(),
                "asm test should succeed. stdout: {} stderr: {}",
                stdout,
                stderr
            );
            assert!(
                stdout.contains("RESULT: PASS"),
                "asm test should print RESULT: PASS, got: {}",
                stdout
            );
            assert!(
                stdout.contains("fence=ok"),
                "asm test should print fence=ok"
            );
            let _ = fs::remove_file(&out);
        }
        Err(e) => {
            let _ = fs::remove_file(&output);
            panic!("AOT compilation failed: {}", e);
        }
    }
}

#[test]
fn test_memory_aura_file_aot_compile() {
    if !llvm_available() {
        eprintln!("SKIP: LLVM not available");
        return;
    }
    let src = read_test_file("native_c2_memory.aura");
    let output = std::env::temp_dir().join("aura_c2_mem_test");
    let result = aot_compile(&src, &output, AotOptions::default());
    match result {
        Ok(aot_output) => {
            let out = aot_output.exe_path.unwrap_or(
                output.with_extension(if cfg!(target_os = "windows") { "exe" } else { "" }),
            );
            let output =
                std::process::Command::new(&out).output().expect("failed to run memory test");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success(),
                "memory test should succeed. stdout: {} stderr: {}",
                stdout,
                stderr
            );
            assert!(
                stdout.contains("RESULT: PASS"),
                "memory test should print RESULT: PASS, got: {}",
                stdout
            );
            assert!(
                stdout.contains("write=ok"),
                "memory test should print write=ok"
            );
            assert!(
                stdout.contains("free=ok"),
                "memory test should print free=ok"
            );
            let _ = fs::remove_file(&out);
        }
        Err(e) => {
            let _ = fs::remove_file(&output);
            panic!("AOT compilation failed: {}", e);
        }
    }
}
