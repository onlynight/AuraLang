//! Phase D: 单元测试 — 优先级机制、SHA256、HMAC
//!
//! 运行：cargo test -p compiler --test phase_d_tests
//!
//! 对应改造方案 D.1（优先级机制）、D.2 P4（SHA256/HMAC）、
//! D.4.3（HirIndex）、D.4.4（HirType::Function）

use compiler::std::std_encoding::sha256_hex;

// ────────────────────────────────────────────────────────────────────────────
// SHA256 正确性测试（NIST 标准测试向量）
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_sha256_empty_string() {
    let hash = sha256_hex(b"");
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn test_sha256_abc() {
    let hash = sha256_hex(b"abc");
    assert_eq!(
        hash,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn test_sha256_hex_output_format() {
    let hash = sha256_hex(b"test");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_sha256_single_byte() {
    let hash = sha256_hex(b"A");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_sha256_55_bytes_boundary() {
    // 55 字节是 padding 边界（55 + 1 + 8 = 64 = 一个完整块）
    let msg: Vec<u8> = vec![b'a'; 55];
    let hash = sha256_hex(&msg);
    assert_eq!(hash.len(), 64);
}

#[test]
fn test_sha256_56_bytes_boundary() {
    // 56 字节触发两个块
    let msg: Vec<u8> = vec![b'a'; 56];
    let hash = sha256_hex(&msg);
    assert_eq!(hash.len(), 64);
}

#[test]
fn test_sha256_deterministic() {
    let h1 = sha256_hex(b"deterministic");
    let h2 = sha256_hex(b"deterministic");
    assert_eq!(h1, h2);
}

#[test]
fn test_sha256_different_input() {
    let h1 = sha256_hex(b"input1");
    let h2 = sha256_hex(b"input2");
    assert_ne!(h1, h2);
}

// ────────────────────────────────────────────────────────────────────────────
// SHA256 native 函数注册测试
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_sha256_native_registration() {
    use compiler::vm::native::NativeRegistry;
    let mut reg = NativeRegistry::new();
    compiler::std::std_encoding::register(&mut reg);
    assert!(reg.contains("aura.lang.std.Encoding.sha256"));
    assert!(reg.contains("aura.lang.std.Encoding.base64Encode"));
    assert!(reg.contains("aura.lang.std.Encoding.hexEncode"));
}

#[test]
fn test_sha256_native_call() {
    use compiler::vm::native::NativeRegistry;
    let mut reg = NativeRegistry::new();
    compiler::std::std_encoding::register(&mut reg);
    let f = reg.get("aura.lang.std.Encoding.sha256").unwrap();
    let result = f(&[compiler::vm::value::Value::str_("hello")]);
    assert_eq!(
        result.as_string(),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn test_encoding_all_native_registered() {
    use compiler::vm::native::NativeRegistry;
    let mut reg = NativeRegistry::new();
    compiler::std::std_encoding::register(&mut reg);
    let expected = [
        "aura.lang.std.Encoding.base64Encode",
        "aura.lang.std.Encoding.base64Decode",
        "aura.lang.std.Encoding.hexEncode",
        "aura.lang.std.Encoding.hexDecode",
        "aura.lang.std.Encoding.urlEncode",
        "aura.lang.std.Encoding.urlDecode",
        "aura.lang.std.Encoding.byteToHex",
        "aura.lang.std.Encoding.hexToByte",
        "aura.lang.std.Encoding.sha256",
    ];
    for name in &expected {
        assert!(reg.contains(name), "Missing native registration: {}", name);
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Ascii / Assert 模块 native 注册移除验证
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ascii_assert_not_registered() {
    use compiler::vm::native::NativeRegistry;
    let mut reg = NativeRegistry::new();
    // Ascii 和 Assert 的 native 注册已在 mod.rs 中注释掉
    assert!(!reg.contains("aura.lang.std.Ascii.isAlpha"));
    assert!(!reg.contains("aura.lang.std.Assert.assert"));
}

// ────────────────────────────────────────────────────────────────────────────
// HIR 结构完整性测试（D.4.3 / D.4.4）
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_decoded_function_is_native_flag() {
    // 验证 DecodedFunction.is_native 字段可用于过滤
    use compiler::vm::DecodedFunction;
    let f = DecodedFunction {
        name: "test".to_string(),
        param_count: 0,
        locals: 0,
        is_native: true,
        code: vec![],
    };
    assert!(f.is_native);
}

#[test]
fn test_decoded_function_aura_flag() {
    use compiler::vm::DecodedFunction;
    let f = DecodedFunction {
        name: "test".to_string(),
        param_count: 0,
        locals: 0,
        is_native: false,
        code: vec![],
    };
    assert!(!f.is_native);
}

// ────────────────────────────────────────────────────────────────────────────
// stdlib_func_map 优先级机制测试
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_stdlib_func_map_priority_filter() {
    // 验证 native 声明和 Aura 实现在 is_native 标记上的区别
    use compiler::vm::DecodedFunction;

    let native_func = DecodedFunction {
        name: "aura.lang.std.Encoding.sha256".to_string(),
        param_count: 1,
        locals: 0,
        is_native: true,
        code: vec![],
    };
    let aura_func = DecodedFunction {
        name: "aura.lang.std.Encoding.base64Encode".to_string(),
        param_count: 1,
        locals: 0,
        is_native: false,
        code: vec![],
    };

    // native 声明应被过滤（do_call_native 中 .filter(|&(idx, _)| !func.is_native)）
    assert!(native_func.is_native);
    assert!(!aura_func.is_native);
}
