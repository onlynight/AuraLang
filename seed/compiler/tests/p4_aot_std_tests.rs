//! Phase 4 AOT std 支持测试
//!
//! 验证：
//! 1. AOT 编译可生成 LLVM IR 文本
//! 2. std C FFI 源文件存在
//! 3. AOT 选项包含 link_std_cffi 字段

// 该测试文件直接依赖 compiler 的 llvm feature（codegen::aot），
// 必须在启用 llvm 时才编译；否则会报 unresolved import。
#![cfg(feature = "llvm")]

use compiler::codegen::aot::{AotOptions, OutputFormat, aot_compile};
use std::fs;

// ─────────────────────────────────────────────────────────────────────────────
// 测试 1: AOT 编译生成 LLVM IR
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_aot_compile_generates_ir() {
    let src = r#"
        fun main(): Int {
            return 42
        }
    "#;

    let tmp = std::env::temp_dir().join("aura_aot_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("create temp directory");

    let options = AotOptions {
        link_std_cffi: true,
        ..Default::default()
    };

    let output = aot_compile(src, &tmp.join("test.ll"), options);

    assert!(
        output.is_ok(),
        "AOT compilation should succeed: {:?}",
        output.err()
    );
    if let Ok(out) = output {
        assert!(out.ll_path.is_some(), "should generate .ll file");
        assert!(!out.ir_text.is_empty(), "LLVM IR text should be non-empty");
    }

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 2: std C FFI 源文件存在
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_std_cffi_source_exists() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let cffi_dir = std::path::Path::new(manifest_dir).join("src/std/cffi");

    assert!(
        cffi_dir.exists(),
        "std C FFI 目录应存在: {}",
        cffi_dir.display()
    );

    let header = cffi_dir.join("aura_std_cffi.h");
    assert!(
        header.exists(),
        "C header file should exist: {}",
        header.display()
    );

    let source = cffi_dir.join("aura_std_cffi.c");
    assert!(
        source.exists(),
        "C source file should exist: {}",
        source.display()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 3: C FFI header 包含核心函数声明
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cffi_header_declarations() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let header_path = std::path::Path::new(manifest_dir).join("src/std/cffi/aura_std_cffi.h");

    let content = fs::read_to_string(&header_path).expect("read header file");

    // Prelude 函数
    assert!(
        content.contains("aura_println"),
        "should contain aura_println declaration"
    );
    assert!(
        content.contains("aura_sqrt"),
        "should contain aura_sqrt declaration"
    );
    assert!(
        content.contains("aura_abs"),
        "should contain aura_abs declaration"
    );

    // IO 函数
    assert!(
        content.contains("aura_io_readLine"),
        "应包含 aura_io_readLine 声明"
    );
    assert!(
        content.contains("aura_io_fileExists"),
        "应包含 aura_io_fileExists 声明"
    );

    // Math 函数
    assert!(
        content.contains("aura_math_sin"),
        "应包含 aura_math_sin 声明"
    );
    assert!(
        content.contains("aura_math_PI"),
        "should contain aura_math_PI constant"
    );

    // String 函数
    assert!(
        content.contains("aura_string_contains"),
        "应包含 aura_string_contains 声明"
    );
    assert!(
        content.contains("aura_string_trim"),
        "应包含 aura_string_trim 声明"
    );

    // Time 函数
    assert!(
        content.contains("aura_time_epoch"),
        "应包含 aura_time_epoch 声明"
    );

    // Random 函数
    assert!(
        content.contains("aura_random_nextInt"),
        "应包含 aura_random_nextInt 声明"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 4: C FFI source 包含实现
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cffi_source_implementation() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source_path = std::path::Path::new(manifest_dir).join("src/std/cffi/aura_std_cffi.c");

    let content = fs::read_to_string(&source_path).expect("read source file");

    // 检查关键实现
    assert!(
        content.contains("void aura_println"),
        "应包含 aura_println 实现"
    );
    assert!(
        content.contains("double aura_sqrt"),
        "应包含 aura_sqrt 实现"
    );
    assert!(
        content.contains("aura_io_readLine"),
        "应包含 aura_io_readLine 实现"
    );
    assert!(
        content.contains("aura_math_sin"),
        "应包含 aura_math_sin 实现"
    );
    assert!(
        content.contains("aura_string_contains"),
        "应包含 aura_string_contains 实现"
    );

    // 检查包含标准头文件
    assert!(
        content.contains("#include <stdio.h>"),
        "should contain stdio.h"
    );
    assert!(
        content.contains("#include <math.h>"),
        "should contain math.h"
    );
    assert!(
        content.contains("#include <string.h>"),
        "should contain string.h"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 5: AotOptions 包含 link_std_cffi 字段
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_aot_options_has_link_std_cffi() {
    let options = AotOptions::default();
    // 默认应启用 std C FFI
    assert!(options.link_std_cffi, "default should enable link_std_cffi");

    // 可以关闭
    let options_no_cffi = AotOptions {
        link_std_cffi: false,
        ..Default::default()
    };
    assert!(!options_no_cffi.link_std_cffi, "can disable link_std_cffi");
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 6: AOT 编译包含 std 调用的程序
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_aot_compile_with_std_calls() {
    let src = r#"
        fun main(): Float {
            return sqrt(16.0)
        }
    "#;

    let tmp = std::env::temp_dir().join("aura_aot_std_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("create temp directory");

    let options = AotOptions {
        link_std_cffi: true,
        ..Default::default()
    };

    let output = aot_compile(src, &tmp.join("test.ll"), options);

    assert!(
        output.is_ok(),
        "AOT compilation should succeed: {:?}",
        output.err()
    );
    if let Ok(out) = output {
        // 检查生成的 IR 包含 sqrt 调用
        assert!(
            out.ir_text.contains("sqrt") || out.ir_text.contains("aura_sqrt"),
            "生成的 IR 应包含 sqrt 调用"
        );
    }

    let _ = fs::remove_dir_all(&tmp);
}
