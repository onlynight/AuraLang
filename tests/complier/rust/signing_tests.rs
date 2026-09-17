//! Signing unit tests — HMAC-SHA256 signing, verification, key config

use compiler::signing::*;

// ─── KeyConfig ──────────────────────────────────────────────────────────────

#[test]
fn test_key_config_from_file() {
    let tmp = tempfile::tempdir().unwrap();
    let key_path = tmp.path().join("key.txt");
    std::fs::write(&key_path, "deadbeefdeadbeefdeadbeefdeadbeef\ntest-signer\n").unwrap();

    let config = KeyConfig::from_file(key_path.to_str().unwrap()).unwrap();
    assert_eq!(config.hmac_key, "deadbeefdeadbeefdeadbeefdeadbeef");
    assert_eq!(config.signer, "test-signer");
}

#[test]
fn test_key_config_from_file_no_signer() {
    let tmp = tempfile::tempdir().unwrap();
    let key_path = tmp.path().join("key.txt");
    std::fs::write(&key_path, "deadbeefdeadbeefdeadbeefdeadbeef\n").unwrap();

    let config = KeyConfig::from_file(key_path.to_str().unwrap()).unwrap();
    assert_eq!(config.hmac_key, "deadbeefdeadbeefdeadbeefdeadbeef");
    assert_eq!(config.signer, "unknown");
}

#[test]
fn test_key_config_from_file_not_found() {
    let result = KeyConfig::from_file("/nonexistent/key.txt");
    assert!(result.is_err());
}

#[test]
fn test_key_config_from_file_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let key_path = tmp.path().join("empty_key.txt");
    std::fs::write(&key_path, "").unwrap();

    let result = KeyConfig::from_file(key_path.to_str().unwrap());
    assert!(result.is_err());
}

// ─── Signer: sign ──────────────────────────────────────────────────────────

#[test]
fn test_signer_sign_basic() {
    let key_config = KeyConfig {
        hmac_key: "deadbeefdeadbeefdeadbeefdeadbeef".to_string(),
        signer: "test".to_string(),
        timestamp: 1234567890,
    };
    let signer = Signer::new(key_config);

    let signature = signer.sign("hello world").unwrap();
    // HMAC-SHA256 produces 32 bytes = 64 hex chars
    assert_eq!(signature.len(), 64);
    assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_signer_sign_deterministic() {
    let key_config = KeyConfig {
        hmac_key: "aabbccdd".to_string(),
        signer: "test".to_string(),
        timestamp: 0,
    };
    let signer = Signer::new(key_config);

    let sig1 = signer.sign("hello").unwrap();
    let sig2 = signer.sign("hello").unwrap();
    assert_eq!(sig1, sig2);
}

#[test]
fn test_signer_sign_different_data() {
    let key_config = KeyConfig {
        hmac_key: "aabbccdd".to_string(),
        signer: "test".to_string(),
        timestamp: 0,
    };
    let signer = Signer::new(key_config);

    let sig1 = signer.sign("hello").unwrap();
    let sig2 = signer.sign("world").unwrap();
    assert_ne!(sig1, sig2);
}

#[test]
fn test_signer_sign_different_key() {
    let config1 = KeyConfig {
        hmac_key: "aabbccdd".to_string(),
        signer: "test".to_string(),
        timestamp: 0,
    };
    let config2 = KeyConfig {
        hmac_key: "deadbeef".to_string(),
        signer: "test".to_string(),
        timestamp: 0,
    };

    let signer1 = Signer::new(config1);
    let signer2 = Signer::new(config2);

    let sig1 = signer1.sign("hello").unwrap();
    let sig2 = signer2.sign("hello").unwrap();
    assert_ne!(sig1, sig2);
}

#[test]
fn test_signer_bad_key() {
    let key_config = KeyConfig {
        hmac_key: "not-hex".to_string(),
        signer: "test".to_string(),
        timestamp: 0,
    };
    let signer = Signer::new(key_config);
    assert!(signer.sign("hello").is_err());
}

// ─── Signer: generate_signature_file ───────────────────────────────────────

#[test]
fn test_generate_signature_file_format() {
    let key_config = KeyConfig {
        hmac_key: "deadbeefdeadbeefdeadbeefdeadbeef".to_string(),
        signer: "test-signer".to_string(),
        timestamp: 1234567890,
    };
    let signer = Signer::new(key_config);

    let checksum = "abc123  META-INF/aura.toml\ndef456  lib/test.auc\n";
    let sig_file = signer.generate_signature_file(checksum).unwrap();

    let lines: Vec<&str> = sig_file.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "test-signer");
    assert_eq!(lines[1], "1234567890");
    assert_eq!(lines[2].len(), 64); // hex signature
}

// ─── signing_utils: hmac_sha256 ─────────────────────────────────────────────

#[test]
fn test_hmac_sha256_basic() {
    let key = b"my-secret-key";
    let data = b"some data to sign";

    let sig = signing_utils::hmac_sha256(key, data).unwrap();
    assert_eq!(sig.len(), 32); // SHA-256 produces 32 bytes
}

#[test]
fn test_hmac_sha256_empty_data() {
    let key = b"key";
    let sig = signing_utils::hmac_sha256(key, b"").unwrap();
    assert_eq!(sig.len(), 32);
}

#[test]
fn test_hmac_sha256_empty_key() {
    let data = b"data";
    let sig = signing_utils::hmac_sha256(b"", data).unwrap();
    assert_eq!(sig.len(), 32);
}

// ─── signing_utils: verify_hmac ─────────────────────────────────────────────

#[test]
fn test_verify_hmac_valid() {
    let key = b"my-secret-key";
    let data = b"some data to sign";

    let sig = signing_utils::hmac_sha256(key, data).unwrap();
    assert!(signing_utils::verify_hmac(key, data, &sig).unwrap());
}

#[test]
fn test_verify_hmac_wrong_key() {
    let key = b"my-secret-key";
    let data = b"some data to sign";

    let sig = signing_utils::hmac_sha256(key, data).unwrap();
    assert!(!signing_utils::verify_hmac(b"wrong-key", data, &sig).unwrap());
}

#[test]
fn test_verify_hmac_wrong_data() {
    let key = b"my-secret-key";
    let data = b"some data to sign";

    let sig = signing_utils::hmac_sha256(key, data).unwrap();
    assert!(!signing_utils::verify_hmac(key, b"tampered", &sig).unwrap());
}

#[test]
fn test_verify_hmac_wrong_signature() {
    let key = b"my-secret-key";
    let data = b"some data to sign";

    let wrong_sig = vec![0u8; 32];
    assert!(!signing_utils::verify_hmac(key, data, &wrong_sig).unwrap());
}

// ─── Constants ──────────────────────────────────────────────────────────────

#[test]
fn test_signing_constants() {
    assert_eq!(SIGNATURE_PATH, "META-INF/signature.sig");
    assert_eq!(CHECKSUM_PATH, "META-INF/checksum.sha256");
}

// ─── SigningError Display ──────────────────────────────────────────────────

#[test]
fn test_signing_error_display_io() {
    let e = SigningError::Io("read failed".to_string());
    assert!(e.to_string().contains("read failed"));
}

#[test]
fn test_signing_error_display_key() {
    let e = SigningError::Key("bad key".to_string());
    assert!(e.to_string().contains("bad key"));
}

#[test]
fn test_signing_error_display_signature() {
    let e = SigningError::Signature("bad signature".to_string());
    assert!(e.to_string().contains("bad signature"));
}
