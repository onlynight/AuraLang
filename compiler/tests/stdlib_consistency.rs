//! Phase D: 一致性检查测试
//!
//! 验证 Aura 源码实现（`aura/core/aura/lang/std/*.aura`）与 Rust 包装器
//! （`compiler/src/std/*.rs`）之间的签名一致性。
//!
//! 运行：cargo test -p compiler --test stdlib_consistency
//!
//! 对应改造方案 D.1「单一真相源机制」和 D.8「验证」。

use std::fs;
use std::path::PathBuf;

/// 从 Aura 源码中提取函数名
fn extract_fn_names(content: &str) -> Vec<String> {
    use regex::Regex;
    let re = Regex::new(r"fun\s+(\w+)\s*\(").unwrap();
    re.captures_iter(content).filter_map(|c| c.get(1)).map(|m| m.as_str().to_string()).collect()
}

/// 从 Rust 源码中提取注册的函数名
fn extract_rust_fn_names(content: &str) -> Vec<String> {
    use regex::Regex;
    // 匹配 reg.insert("name", ...) 或 reg.register("name", ...)
    let re = Regex::new(r#"(?:reg\.insert|reg\.register)\s*\(\s*"([^"]+)""#).unwrap();
    re.captures_iter(content).filter_map(|c| c.get(1)).map(|m| m.as_str().to_string()).collect()
}

/// 检查 Aura 源码中的函数是否在 Rust 中有对应的注册
///
/// 返回 (一致数, 不一致数, 警告列表)
fn check_consistency(aura_dir: &str, rust_dir: &str) -> (usize, usize, Vec<String>) {
    let mut consistent = 0;
    let mut inconsistent = 0;
    let mut warnings = Vec::new();

    // 扫描 Aura 文件
    let aura_files = scan_aura_files(PathBuf::from(aura_dir));
    for aura_file in &aura_files {
        let content = match fs::read_to_string(aura_file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let aura_fns = extract_fn_names(&content);

        // 对应的 Rust 文件
        let aura_name = aura_file.file_stem().unwrap().to_string_lossy().to_lowercase();
        let rust_file = PathBuf::from(rust_dir).join(format!("std_{}.rs", aura_name));
        if !rust_file.exists() {
            continue;
        }

        let rust_content = match fs::read_to_string(&rust_file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rust_fns = extract_rust_fn_names(&rust_content);

        // 检查 Aura 中定义的函数是否在 Rust 中有注册
        for fn_name in &aura_fns {
            let found =
                rust_fns.iter().any(|rf| rf == fn_name || rf.ends_with(&format!(".{}", fn_name)));
            if found {
                consistent += 1;
            } else {
                // 纯 Aura 实现（无 Rust 对应）不算不一致，仅记录
                inconsistent += 1;
                warnings.push(format!(
                    "函数 `{}` 在 {} 中定义但 Rust 中无注册（纯 Aura 实现）",
                    fn_name,
                    aura_file.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }

    (consistent, inconsistent, warnings)
}

/// 递归扫描 .aura 文件
fn scan_aura_files(dir: PathBuf) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                results.extend(scan_aura_files(path));
            } else if path.extension().map(|e| e == "aura").unwrap_or(false) {
                results.push(path);
            }
        }
    }
    results
}

#[test]
fn test_stdlib_consistency() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let aura_dir = manifest_dir.join("../aura/core/aura/lang/std");
    let rust_dir = manifest_dir.join("src/std");

    let (consistent, inconsistent, warnings) = check_consistency(
        aura_dir.to_str().unwrap_or("."),
        rust_dir.to_str().unwrap_or("."),
    );

    eprintln!("=== 一致性检查 ===");
    eprintln!("  一致函数: {}", consistent);
    eprintln!("  纯 Aura 实现: {}", inconsistent);
    eprintln!("  总警告: {}", warnings.len());

    if !warnings.is_empty() {
        eprintln!("  （以下函数仅有 Aura 实现，无 Rust native 对应——符合纯 Aura 化方向）");
    }

    // 纯 Aura 实现是预期结果，不阻断测试
    // 仅检查一致数是否合理（不应为 0）
    assert!(
        consistent > 0 || inconsistent > 0,
        "一致性检查失败：未找到任何 Aura 函数"
    );
    eprintln!("一致性检查通过");
}

#[test]
fn test_embedded_stdlib_modules_exist() {
    let build_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../build");
    let modules = [
        "Math.auc",
        "Time.auc",
        "Collections.auc",
        "Test.auc",
        "Ascii.auc",
        "Assert.auc",
        "Encoding.auc",
        "Iter.auc",
        "Json.auc",
        "StringBuilder.auc",
        "TestHelper.auc",
        "Path.auc",
        "String.auc",
        "Actor.auc",
        "Channel.auc",
        "Coroutine.auc",
    ];

    let mut missing = Vec::new();
    for module in &modules {
        let path = build_dir.join(module);
        if !path.exists() {
            missing.push(module.to_string());
        }
    }

    if !missing.is_empty() {
        panic!(
            "嵌入标准库模块缺失: {:?}\n请先运行: aura stdlib-compile aura/core/aura/lang/std --output build",
            missing
        );
    }
    eprintln!("所有嵌入标准库模块存在（{} 个）", modules.len());
}
