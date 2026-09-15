//! .auz package format integration tests — builder, reader, checksum, constants

use compiler::auz::builder::*;
use compiler::auz::checksum::*;
use compiler::auz::reader::*;
use compiler::auz::*;
use compiler::codegen::BytecodeModule;
use compiler::package::{PackageKind, PackageManifest, PackageOptions, ResourceConfig};
use std::path::Path;

// ─── Constants ──────────────────────────────────────────────────────────────

#[test]
fn test_auz_constants() {
    assert_eq!(
        ZSTD_MAGIC,
        [
            0x28, 0xB5, 0x2F, 0xFD
        ]
    );
    assert_eq!(APKG_FORMAT_VERSION, 1);
    assert_eq!(DEFAULT_COMPRESSION_LEVEL, 3);
    assert_eq!(META_INF_DIR, "META-INF");
    assert_eq!(MANIFEST_FILENAME, "META-INF/aura.toml");
    assert_eq!(CHECKSUM_FILENAME, "META-INF/checksum.sha256");
    assert_eq!(SIGNATURE_FILENAME, "META-INF/signature.sig");
    assert_eq!(LIB_DIR, "lib");
    assert_eq!(REF_DIR, "ref");
    assert_eq!(REF_INDEX_FILENAME, "ref/index.json");
    assert_eq!(NATIVE_DIR, "native");
    assert_eq!(SRC_DIR, "src");
    assert_eq!(DOCS_DIR, "docs");
    assert_eq!(RESOURCES_DIR, "resources");
    assert_eq!(TEST_DIR, "test");
}

#[test]
fn test_path_under() {
    assert!(path_under("META-INF", "META-INF/aura.toml"));
    assert!(path_under("META-INF", "META-INF/sub/file"));
    assert!(!path_under("META-INF", "META-INF2/file"));
    assert!(!path_under("META-INF", "lib/foo"));
    assert!(path_under("lib", "lib/name-version/entry.auc"));
}

// ─── Checksum ───────────────────────────────────────────────────────────────

#[test]
fn test_checksum_compute_sha256() {
    let hash = compute_sha256(b"");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

    let hash = compute_sha256(b"hello");
    assert_eq!(
        hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn test_checksum_entry_roundtrip() {
    let entry = ChecksumEntry {
        hash: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_string(),
        path: "META-INF/aura.toml".to_string(),
    };
    let line = entry.to_line();
    assert_eq!(
        line,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  META-INF/aura.toml"
    );
    let parsed = ChecksumEntry::parse_line(&line).unwrap();
    assert_eq!(parsed, entry);
}

#[test]
fn test_checksum_generate_and_parse() {
    let entries = vec![
        ChecksumEntry {
            hash: "a".repeat(64),
            path: "META-INF/aura.toml".to_string(),
        },
        ChecksumEntry {
            hash: "b".repeat(64),
            path: "lib/test.auc".to_string(),
        },
    ];
    let content = generate_checksum_file(&entries);
    assert!(content.ends_with('\n'));
    assert_eq!(content.lines().count(), 2);

    let parsed = parse_checksum_file(&content);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].path, "META-INF/aura.toml");
    assert_eq!(parsed[1].path, "lib/test.auc");
}

#[test]
fn test_checksum_verify_bytes() {
    let data = b"test data";
    let hash = compute_sha256(data);
    assert!(verify_bytes(&hash, data).is_ok());
    assert!(verify_bytes("wrong", data).is_err());
}

#[test]
fn test_checksum_parse_line_invalid() {
    assert!(ChecksumEntry::parse_line("abc  path").is_none());
    assert!(ChecksumEntry::parse_line("zzzz  path").is_none());
    assert!(ChecksumEntry::parse_line("").is_none());
    assert!(ChecksumEntry::parse_line("# comment").is_none());
}

// ─── ApkgError Display ──────────────────────────────────────────────────────

#[test]
fn test_apkg_error_display() {
    assert!(ApkgError::Io("msg".to_string()).to_string().contains("msg"));
    assert!(ApkgError::Format("msg".to_string()).to_string().contains("msg"));
    assert!(ApkgError::Checksum("msg".to_string()).to_string().contains("msg"));
    assert!(ApkgError::Manifest("msg".to_string()).to_string().contains("msg"));
    assert!(ApkgError::Compression("msg".to_string()).to_string().contains("msg"));
    assert!(ApkgError::Build("msg".to_string()).to_string().contains("msg"));
}

// ─── PackageBuildOptions ────────────────────────────────────────────────────

#[test]
fn test_build_options_minimal() {
    let opts = PackageBuildOptions::minimal();
    assert!(!opts.include_sources);
    assert!(!opts.include_resources);
    assert!(opts.include_ref_index);
    assert_eq!(opts.compression_level, DEFAULT_COMPRESSION_LEVEL);
}

#[test]
fn test_build_options_full() {
    let opts = PackageBuildOptions::full();
    assert!(opts.include_sources);
    assert!(!opts.include_resources);
    assert!(opts.include_ref_index);
}

// ─── PackageBuilder + PackageReader roundtrip ───────────────────────────────

#[test]
fn test_auz_build_and_read_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();

    let manifest = PackageManifest {
        schema_version: "1.0".to_string(),
        name: "test-pkg".to_string(),
        version: "0.1.0".to_string(),
        description: Some("Test package".to_string()),
        authors: vec!["Test Author".to_string()],
        license: Some("MIT".to_string()),
        repository: None,
        entry: "main.aura".to_string(),
        dependencies: vec![],
        dev_dependencies: vec![],
        exports: vec![],
        platforms: vec![],
        library: false,
        kind: PackageKind::Bytecode,
        compiler_min_version: None,
        compiler_max_version: None,
        package: PackageOptions::default(),
        resources: ResourceConfig::default(),
    };

    let module = BytecodeModule::default();

    let out_path = tmp.path().join("test-pkg.auz");
    let builder = PackageBuilder::new(&manifest, &module);
    let result = builder.build(&out_path).unwrap();

    assert!(out_path.exists());
    assert!(result.size_bytes > 0);

    let content = PackageReader::from_file(&out_path).unwrap();
    assert_eq!(content.manifest.name, "test-pkg");
    assert_eq!(content.manifest.version, "0.1.0");
    assert!(content.module.is_some());
    assert!(!content.files.is_empty());
}

#[test]
fn test_auz_build_with_sources() {
    let tmp = tempfile::tempdir().unwrap();

    let manifest = PackageManifest {
        schema_version: "1.0".to_string(),
        name: "test-pkg".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        authors: vec![],
        license: None,
        repository: None,
        entry: "main.aura".to_string(),
        dependencies: vec![],
        dev_dependencies: vec![],
        exports: vec![],
        platforms: vec![],
        library: false,
        kind: PackageKind::Bytecode,
        compiler_min_version: None,
        compiler_max_version: None,
        package: PackageOptions::default(),
        resources: ResourceConfig::default(),
    };

    let module = BytecodeModule::default();

    // Create a source directory with a .aura file
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("main.aura"), "fun main() { println(1) }").unwrap();

    let out_path = tmp.path().join("test-pkg.auz");
    let builder = PackageBuilder::new(&manifest, &module)
        .with_source_dir(src_dir.as_path())
        .with_options(PackageBuildOptions::full());
    let result = builder.build(&out_path).unwrap();

    assert!(out_path.exists());
    assert!(result.size_bytes > 0);

    let content = PackageReader::from_file(&out_path).unwrap();
    assert!(!content.source_files().is_empty());
}

#[test]
fn test_auz_read_bad_magic() {
    let tmp = tempfile::tempdir().unwrap();
    let bad_path = tmp.path().join("bad.auz");
    std::fs::write(&bad_path, b"not a zstd file").unwrap();

    let result = PackageReader::from_file(&bad_path);
    assert!(result.is_err());
}

#[test]
fn test_auz_read_missing_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let bad_path = tmp.path().join("bad.auz");
    create_invalid_auz(&bad_path);
    let result = PackageReader::from_file(&bad_path);
    assert!(result.is_err());
}

#[test]
fn test_package_content_module_not_found() {
    let manifest = PackageManifest {
        schema_version: "1.0".to_string(),
        name: "test".to_string(),
        version: "1.0".to_string(),
        description: None,
        authors: vec![],
        license: None,
        repository: None,
        entry: "main.aura".to_string(),
        dependencies: vec![],
        dev_dependencies: vec![],
        exports: vec![],
        platforms: vec![],
        library: false,
        kind: PackageKind::Bytecode,
        compiler_min_version: None,
        compiler_max_version: None,
        package: PackageOptions::default(),
        resources: ResourceConfig::default(),
    };
    let content = PackageContent {
        manifest,
        module: None,
        files: std::collections::BTreeMap::new(),
        checksum_entries: vec![],
        verified: false,
    };
    assert!(content.module().is_err());
}

#[test]
fn test_auz_verify_checksum() {
    let tmp = tempfile::tempdir().unwrap();

    let manifest = PackageManifest {
        schema_version: "1.0".to_string(),
        name: "verify-pkg".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        authors: vec![],
        license: None,
        repository: None,
        entry: "main.aura".to_string(),
        dependencies: vec![],
        dev_dependencies: vec![],
        exports: vec![],
        platforms: vec![],
        library: false,
        kind: PackageKind::Bytecode,
        compiler_min_version: None,
        compiler_max_version: None,
        package: PackageOptions::default(),
        resources: ResourceConfig::default(),
    };

    let module = BytecodeModule::default();

    let out_path = tmp.path().join("verify-pkg.auz");
    let builder = PackageBuilder::new(&manifest, &module);
    builder.build(&out_path).unwrap();

    // Verify should succeed
    let verify_result = PackageReader::verify(&out_path).unwrap();
    assert!(verify_result.failures.is_empty());
}

// ─── Helper functions ───────────────────────────────────────────────────────

fn create_invalid_auz(path: &Path) {
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_ustar();
    header.set_size(5);
    tar.append_data(&mut header, "nonexistent", &b"hello"[..]).unwrap();
    let tar_bytes = tar.into_inner().unwrap();

    let compressed =
        zstd::stream::encode_all(std::io::Cursor::new(tar_bytes), DEFAULT_COMPRESSION_LEVEL)
            .unwrap();

    std::fs::write(path, compressed).unwrap();
}
