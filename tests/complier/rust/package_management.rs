//! Package management unit tests — Version, VersionConstraint, Dependency, LockFile, PackageManifest

use compiler::package::*;

// ─── Version parsing ─────────────────────────────────────────────────────────

#[test]
fn test_version_parse_basic() {
    let v = Version::parse("1.2.3").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 2);
    assert_eq!(v.patch, 3);
    assert!(v.prerelease.is_empty());
    assert!(v.build.is_empty());
}

#[test]
fn test_version_parse_two_parts() {
    let v = Version::parse("1.2").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 2);
    assert_eq!(v.patch, 0);
}

#[test]
fn test_version_parse_prerelease() {
    let v = Version::parse("1.2.3-rc.1").unwrap();
    assert_eq!(
        v.prerelease,
        vec![
            "rc".to_string(),
            "1".to_string()
        ]
    );
}

#[test]
fn test_version_parse_build() {
    let v = Version::parse("1.2.3+build.123").unwrap();
    assert_eq!(
        v.build,
        vec![
            "build".to_string(),
            "123".to_string()
        ]
    );
}

#[test]
fn test_version_parse_prerelease_and_build() {
    let v = Version::parse("1.0.0-alpha.1+build.456").unwrap();
    assert_eq!(
        v.prerelease,
        vec![
            "alpha".to_string(),
            "1".to_string()
        ]
    );
    assert_eq!(
        v.build,
        vec![
            "build".to_string(),
            "456".to_string()
        ]
    );
}

#[test]
fn test_version_parse_invalid() {
    assert!(Version::parse("invalid").is_err());
    assert!(Version::parse("1").is_err());
    assert!(Version::parse("").is_err());
}

#[test]
fn test_version_to_string_roundtrip() {
    let v = Version::parse("1.2.3").unwrap();
    assert_eq!(v.to_string(), "1.2.3");

    let v = Version::parse("1.2.3-rc.1+build.456").unwrap();
    assert_eq!(v.to_string(), "1.2.3-rc.1+build.456");
}

#[test]
fn test_version_ordering() {
    let v1 = Version::new(1, 0, 0);
    let v2 = Version::new(1, 1, 0);
    let v3 = Version::new(2, 0, 0);
    let v4 = Version::new(1, 0, 0);
    assert!(v1 < v2);
    assert!(v2 < v3);
    assert!(v1 == v4);
}

#[test]
fn test_version_prerelease_ordering() {
    let stable = Version::new(1, 0, 0);
    let mut rc = Version::new(1, 0, 0);
    rc.prerelease = vec![
        "rc".to_string(),
        "1".to_string(),
    ];
    assert!(rc < stable);
}

// ─── VersionConstraint parsing and matching ──────────────────────────────────

#[test]
fn test_constraint_parse_exact() {
    let c = VersionConstraint::parse("== 1.2.3").unwrap();
    assert!(matches!(c, VersionConstraint::Exact(_)));
    assert!(Version::new(1, 2, 3).matches_constraint(&c));
    assert!(!Version::new(1, 2, 4).matches_constraint(&c));
}

#[test]
fn test_constraint_parse_equal_single() {
    let c = VersionConstraint::parse("= 1.2.3").unwrap();
    assert!(matches!(c, VersionConstraint::Exact(_)));
}

#[test]
fn test_constraint_parse_no_operator_is_exact() {
    let c = VersionConstraint::parse("1.2.3").unwrap();
    assert!(matches!(c, VersionConstraint::Exact(_)));
}

#[test]
fn test_constraint_parse_gte() {
    let c = VersionConstraint::parse(">= 1.0.0").unwrap();
    assert!(Version::new(1, 0, 0).matches_constraint(&c));
    assert!(Version::new(1, 1, 0).matches_constraint(&c));
    assert!(Version::new(2, 0, 0).matches_constraint(&c));
    assert!(!Version::new(0, 9, 0).matches_constraint(&c));
}

#[test]
fn test_constraint_parse_gt() {
    let c = VersionConstraint::parse("> 1.0.0").unwrap();
    assert!(Version::new(1, 0, 1).matches_constraint(&c));
    assert!(!Version::new(1, 0, 0).matches_constraint(&c));
}

#[test]
fn test_constraint_parse_lt() {
    let c = VersionConstraint::parse("< 2.0.0").unwrap();
    assert!(Version::new(1, 9, 0).matches_constraint(&c));
    assert!(!Version::new(2, 0, 0).matches_constraint(&c));
}

#[test]
fn test_constraint_parse_tilde() {
    let c = VersionConstraint::parse("~> 1.0.0").unwrap();
    // ~1.0 means >=1.0, <2.0
    assert!(Version::new(1, 0, 0).matches_constraint(&c));
    assert!(Version::new(1, 5, 0).matches_constraint(&c));
    assert!(!Version::new(2, 0, 0).matches_constraint(&c));
}

#[test]
fn test_constraint_parse_compatible() {
    let c = VersionConstraint::parse("^1.0.0").unwrap();
    // ^1.0 means >=1.0, <2.0
    assert!(Version::new(1, 0, 0).matches_constraint(&c));
    assert!(Version::new(1, 5, 0).matches_constraint(&c));
    assert!(!Version::new(2, 0, 0).matches_constraint(&c));
}

#[test]
fn test_constraint_parse_compatible_zero_major() {
    let c = VersionConstraint::parse("^0.5.0").unwrap();
    // ^0.5 means >=0.5, <0.6
    assert!(Version::new(0, 5, 0).matches_constraint(&c));
    assert!(Version::new(0, 5, 9).matches_constraint(&c));
    assert!(!Version::new(0, 6, 0).matches_constraint(&c));
}

#[test]
fn test_constraint_parse_compatible_zero_patch() {
    let c = VersionConstraint::parse("^0.0.5").unwrap();
    // ^0.0.5 means exactly 0.0.5
    assert!(Version::new(0, 0, 5).matches_constraint(&c));
    assert!(!Version::new(0, 0, 6).matches_constraint(&c));
}

#[test]
fn test_constraint_parse_any() {
    let c = VersionConstraint::parse("*").unwrap();
    assert!(matches!(c, VersionConstraint::Any));
    assert!(Version::new(99, 99, 99).matches_constraint(&c));
}

#[test]
fn test_constraint_parse_latest() {
    let c = VersionConstraint::parse("latest").unwrap();
    assert!(matches!(c, VersionConstraint::Any));
}

#[test]
fn test_constraint_to_string() {
    let cases = vec![
        (VersionConstraint::Exact(Version::new(2, 1, 0)), "== 2.1.0"),
        (
            VersionConstraint::GreaterThanEqual(Version::new(1, 0, 0)),
            ">= 1.0.0",
        ),
        (
            VersionConstraint::GreaterThan(Version::new(1, 0, 0)),
            "> 1.0.0",
        ),
        (
            VersionConstraint::LessThan(Version::new(1, 0, 0)),
            "< 1.0.0",
        ),
        (VersionConstraint::Tilde(Version::new(3, 0, 0)), "~> 3.0.0"),
        (
            VersionConstraint::Compatible(Version::new(4, 5, 0)),
            "^4.5.0",
        ),
        (VersionConstraint::Any, "*"),
    ];
    for (c, expected) in cases {
        assert_eq!(c.to_string(), expected, "constraint: {}", c.to_string());
    }
}

// ─── Dependency parsing ──────────────────────────────────────────────────────

#[test]
fn test_parse_depends_basic() {
    let source = r#"
// @depends aura-json >= 1.0
// @depends aura-http == 2.1
import json
"#;
    let deps = parse_depends(source);
    assert_eq!(deps.len(), 2);
    assert_eq!(deps[0].name, "aura-json");
    assert_eq!(deps[1].name, "aura-http");
}

#[test]
fn test_parse_depends_with_git_source() {
    let source = r#"
// @depends aura-http from git@github.com:user/http.git
// @depends aura-raylib == 5.0
"#;
    let deps = parse_depends(source);
    assert_eq!(deps.len(), 2);
    assert!(matches!(deps[0].source, DependencySource::Git(_)));
    // aura-raylib has no explicit source, should default to Git
    assert!(matches!(deps[1].source, DependencySource::Git(_)));
}

#[test]
fn test_parse_depends_with_https_source() {
    let source = r#"
// @depends aura-http from https://github.com/user/http.git
"#;
    let deps = parse_depends(source);
    assert_eq!(deps.len(), 1);
    assert!(matches!(deps[0].source, DependencySource::Git(_)));
}

#[test]
fn test_parse_depends_with_local_path() {
    let source = r#"
// @depends local-lib from ../local-lib
"#;
    let deps = parse_depends(source);
    assert_eq!(deps.len(), 1);
    assert!(matches!(deps[0].source, DependencySource::Path(_)));
}

#[test]
fn test_parse_depends_with_version_and_source() {
    let source = r#"
// @depends aura-http == 2.1 from git@github.com:user/http.git
"#;
    let deps = parse_depends(source);
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].name, "aura-http");
    assert!(matches!(deps[0].version, VersionConstraint::Exact(_)));
    assert!(matches!(deps[0].source, DependencySource::Git(_)));
}

#[test]
fn test_parse_depends_no_version_uses_any() {
    let source = r#"
// @depends aura-json
"#;
    let deps = parse_depends(source);
    assert_eq!(deps.len(), 1);
    assert!(matches!(deps[0].version, VersionConstraint::Any));
}

#[test]
fn test_parse_depends_ignores_comments() {
    let source = r#"
// This is a normal comment
// @notdepends aura-json >= 1.0
import json
"#;
    let deps = parse_depends(source);
    assert_eq!(deps.len(), 0);
}

#[test]
fn test_parse_depends_mixed_content() {
    let source = r#"
fun main() {
    // @depends aura-math ~> 2.0
    println("hello")
}
// @depends aura-string >= 1.0 from git@github.com/aura/string.git
"#;
    let deps = parse_depends(source);
    assert_eq!(deps.len(), 2);
    assert_eq!(deps[0].name, "aura-math");
    assert_eq!(deps[1].name, "aura-string");
}

// ─── PackageKind parsing ─────────────────────────────────────────────────────

#[test]
fn test_package_kind_parse() {
    assert_eq!(
        PackageKind::parse("bytecode").unwrap(),
        PackageKind::Bytecode
    );
    assert_eq!(PackageKind::parse("bc").unwrap(), PackageKind::Bytecode);
    assert_eq!(PackageKind::parse("hybrid").unwrap(), PackageKind::Hybrid);
    assert_eq!(PackageKind::parse("mixed").unwrap(), PackageKind::Hybrid);
    assert_eq!(PackageKind::parse("native").unwrap(), PackageKind::Native);
    assert_eq!(PackageKind::parse("aot").unwrap(), PackageKind::Native);
}

#[test]
fn test_package_kind_parse_invalid() {
    assert!(PackageKind::parse("unknown").is_err());
    assert!(PackageKind::parse("").is_err());
}

#[test]
fn test_package_kind_as_str() {
    assert_eq!(PackageKind::Bytecode.as_str(), "bytecode");
    assert_eq!(PackageKind::Hybrid.as_str(), "hybrid");
    assert_eq!(PackageKind::Native.as_str(), "native");
}

// ─── PackageManifest TOML roundtrip ─────────────────────────────────────────

#[test]
fn test_manifest_toml_roundtrip() {
    let manifest = PackageManifest {
        schema_version: "1.0".to_string(),
        name: "test-package".to_string(),
        version: "0.1.0".to_string(),
        description: Some("A test package".to_string()),
        authors: vec!["Test Author".to_string()],
        license: Some("MIT".to_string()),
        repository: Some("https://github.com/test/test-package".to_string()),
        entry: "main.aura".to_string(),
        dependencies: vec![],
        dev_dependencies: vec![],
        exports: vec!["main".to_string()],
        platforms: vec!["x86_64-pc-windows-msvc".to_string()],
        library: false,
        kind: PackageKind::Hybrid,
        compiler_min_version: Some("0.1.0".to_string()),
        compiler_max_version: None,
        package: PackageOptions::default(),
        resources: ResourceConfig::default(),
    };

    let toml_str = manifest.to_toml().unwrap();
    assert!(toml_str.contains("test-package"));
    assert!(toml_str.contains("0.1.0"));
    assert!(toml_str.contains("hybrid"));

    let parsed = PackageManifest::from_toml(&toml_str).unwrap();
    assert_eq!(parsed.name, "test-package");
    assert_eq!(parsed.version, "0.1.0");
    assert_eq!(parsed.kind, PackageKind::Hybrid);
    assert_eq!(parsed.entry, "main.aura");
    assert!(!parsed.library);
}

#[test]
fn test_manifest_is_library() {
    let mut manifest = PackageManifest {
        schema_version: "1.0".to_string(),
        name: "test-lib".to_string(),
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

    // Not a library by default
    assert!(!manifest.is_library());

    // Is a library when library = true
    manifest.library = true;
    assert!(manifest.is_library());

    // Is a library when entry is empty
    manifest.library = false;
    manifest.entry = String::new();
    assert!(manifest.is_library());
}

#[test]
fn test_manifest_minimal_toml() {
    let toml_str = r#"
name = "minimal-pkg"
version = "1.0.0"
"#;
    let manifest = PackageManifest::from_toml(toml_str).unwrap();
    assert_eq!(manifest.name, "minimal-pkg");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.entry, "main.aura");
    assert!(manifest.description.is_none());
}

// ─── LockFile ────────────────────────────────────────────────────────────────

#[test]
fn test_lock_file_write_and_read() {
    let lock = LockFile {
        version: 1,
        dependencies: vec![
            LockEntry {
                name: "aura-json".to_string(),
                version: Version::new(1, 0, 0),
                source: DependencySource::Git("https://github.com/aura/json".to_string()),
                rev: Some("abc123".to_string()),
                checksum: Some("deadbeef".to_string()),
            },
            LockEntry {
                name: "aura-string".to_string(),
                version: Version::new(2, 1, 0),
                source: DependencySource::Git("https://github.com/aura/string".to_string()),
                rev: None,
                checksum: None,
            },
        ],
    };

    let tmp = tempfile::tempdir().unwrap();
    let lock_path = tmp.path().join("aura.lock");
    lock.write_to_file(&lock_path).unwrap();

    let loaded = LockFile::from_file(&lock_path).unwrap();
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.dependencies.len(), 2);
    assert_eq!(loaded.dependencies[0].name, "aura-json");
    assert_eq!(loaded.dependencies[1].name, "aura-string");
}

#[test]
fn test_lock_file_find() {
    let lock = LockFile {
        version: 1,
        dependencies: vec![
            LockEntry {
                name: "aura-json".to_string(),
                version: Version::new(1, 0, 0),
                source: DependencySource::Git("url".to_string()),
                rev: None,
                checksum: None,
            },
        ],
    };
    assert!(lock.find("aura-json").is_some());
    assert!(lock.find("nonexistent").is_none());
}

// ─── PackageManifest write_to_file ──────────────────────────────────────────

#[test]
fn test_manifest_write_to_file() {
    let manifest = PackageManifest {
        schema_version: "1.0".to_string(),
        name: "test-pkg".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Test".to_string()),
        authors: vec!["Author".to_string()],
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

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("aura.toml");
    manifest.write_to_file(&path).unwrap();

    let loaded = PackageManifest::from_toml_file(&path).unwrap();
    assert_eq!(loaded.name, "test-pkg");
    assert_eq!(loaded.version, "1.0.0");
}
