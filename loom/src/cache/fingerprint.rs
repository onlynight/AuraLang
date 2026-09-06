//! [Phase B2] Fingerprint 计算（SHA-256）
//!
//! fingerprint(task) = SHA256(
//!   任务名称
//!   + 源文件内容哈希
//!   + 编译选项
//!   + 依赖任务的 fingerprint
//!   + 编译器版本
//!   + 插件版本
//! )

use std::collections::HashMap;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::LoomError;
use crate::task::{TaskDefinition, TaskKind};

/// 编译器版本标识
pub const COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Fingerprint 计算结果
#[derive(Debug, Clone)]
pub struct Fingerprint {
    /// SHA-256 哈希（十六进制字符串）
    pub hash: String,
    /// 关联的任务名
    pub task_name: String,
    /// 参与计算的文件列表
    pub files: Vec<String>,
    /// 参与计算的选项
    pub options: Vec<(String, String)>,
}

impl Fingerprint {
    /// 从任务定义计算 fingerprint
    pub fn compute(task: &TaskDefinition) -> Result<String, LoomError> {
        let mut hasher = Sha256::new();

        // 1. 任务名称
        hasher.update(task.name.as_bytes());

        // 2. 源文件内容哈希
        for file in &task.inputs.files {
            if file.exists() {
                let content = std::fs::read(file).map_err(|e| {
                    LoomError::Cache(format!("无法读取文件 {}: {}", file.display(), e))
                })?;
                let file_hash = hash_bytes(&content);
                hasher.update(file_hash);
            } else {
                // 文件不存在时，用路径的哈希作为占位
                let path_str = file.to_string_lossy().to_string();
                hasher.update(hash_bytes(path_str.as_bytes()));
            }
        }

        // 3. 编译选项（排序后写入以保证确定性）
        let mut sorted_options: Vec<_> = task.inputs.options.iter().collect();
        sorted_options.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in sorted_options {
            hasher.update(k.as_bytes());
            hasher.update(v.as_bytes());
        }

        // 4. 依赖任务的 fingerprint（排序后写入）
        let mut sorted_deps: Vec<_> = task.inputs.dep_fingerprints.iter().collect();
        sorted_deps.sort_by(|a, b| a.0.cmp(b.0));
        for (name, fp) in sorted_deps {
            hasher.update(name.as_bytes());
            hasher.update(fp.as_bytes());
        }

        // 5. 编译器版本
        hasher.update(COMPILER_VERSION.as_bytes());

        // 6. 任务类型标识
        let kind_tag = match &task.kind {
            TaskKind::Clean => "clean".to_string(),
            TaskKind::Resolve => "resolve".to_string(),
            TaskKind::Compile(ss) => format!("compile:{}", ss),
            TaskKind::Test => "test".to_string(),
            TaskKind::Package => "package".to_string(),
            TaskKind::Verify => "verify".to_string(),
            TaskKind::Check => "check".to_string(),
            TaskKind::Install => "install".to_string(),
            TaskKind::Deploy => "deploy".to_string(),
            TaskKind::Execute => "execute".to_string(),
            TaskKind::Watch => "watch".to_string(),
            TaskKind::Plugin(name) => format!("plugin:{}", name),
        };
        hasher.update(kind_tag.as_bytes());

        Ok(hex::encode(hasher.finalize()))
    }

    /// 计算带详细信息的 fingerprint
    pub fn compute_detailed(task: &TaskDefinition) -> Result<Fingerprint, LoomError> {
        let hash = Self::compute(task)?;
        let files = task.inputs.files.iter().map(|f| f.to_string_lossy().to_string()).collect();
        let options: Vec<(String, String)> =
            task.inputs.options.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        Ok(Fingerprint {
            hash,
            task_name: task.name.clone(),
            files,
            options,
        })
    }

    /// 比较两个 fingerprint 是否相同
    pub fn equals(&self, other: &str) -> bool {
        self.hash == other
    }

    /// 从缓存的 fingerprint 字符串解析
    pub fn from_cache(hash: &str) -> Self {
        Self {
            hash: hash.to_string(),
            task_name: String::new(),
            files: Vec::new(),
            options: Vec::new(),
        }
    }
}

/// 计算字节的 SHA-256 哈希
pub fn hash_bytes(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

/// 计算文件的 SHA-256 哈希
pub fn hash_file(path: &Path) -> Result<String, LoomError> {
    let data = std::fs::read(path)
        .map_err(|e| LoomError::Cache(format!("无法读取文件 {}: {}", path.display(), e)))?;
    Ok(hex::encode(hash_bytes(&data)))
}

/// 计算目录中所有文件的哈希（递归）
pub fn hash_directory(
    dir: &Path,
    include_ext: Option<&str>,
) -> Result<HashMap<String, String>, LoomError> {
    let mut hashes = HashMap::new();

    let walker = walkdir::WalkDir::new(dir).into_iter();
    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = include_ext {
                if path.extension().map(|e| e.to_string_lossy() == ext).unwrap_or(false) {
                    let hash = hash_file(path)?;
                    let rel = path.strip_prefix(dir).unwrap_or(path);
                    hashes.insert(rel.to_string_lossy().to_string(), hash);
                }
            } else {
                let hash = hash_file(path)?;
                let rel = path.strip_prefix(dir).unwrap_or(path);
                hashes.insert(rel.to_string_lossy().to_string(), hash);
            }
        }
    }

    Ok(hashes)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs};
    use std::collections::HashMap;

    fn make_task(
        name: &str,
        kind: TaskKind,
        files: &[&str],
        options: &[(&str, &str)],
    ) -> TaskDefinition {
        TaskDefinition {
            name: name.to_string(),
            description: format!("test task {}", name),
            kind,
            depends_on: Vec::new(),
            inputs: TaskInputs {
                files: files.iter().map(|f| std::path::PathBuf::from(f)).collect(),
                options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                dep_fingerprints: HashMap::new(),
            },
            outputs: TaskOutputs::default(),
        }
    }

    #[test]
    fn test_compute_fingerprint_basic() {
        let task = make_task(
            "compile-main",
            TaskKind::Compile("main".to_string()),
            &[],
            &[("opt", "2")],
        );
        let fp = Fingerprint::compute(&task).unwrap();
        assert_eq!(fp.len(), 64); // SHA-256 = 64 hex chars
    }

    #[test]
    fn test_fingerprint_deterministic() {
        // Same task should produce same fingerprint
        let task = make_task(
            "compile-main",
            TaskKind::Compile("main".to_string()),
            &[],
            &[("opt", "2")],
        );
        let fp1 = Fingerprint::compute(&task).unwrap();
        let fp2 = Fingerprint::compute(&task).unwrap();
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_fingerprint_changes_with_options() {
        let task1 = make_task(
            "compile",
            TaskKind::Compile("main".to_string()),
            &[],
            &[("opt", "2")],
        );
        let task2 = make_task(
            "compile",
            TaskKind::Compile("main".to_string()),
            &[],
            &[("opt", "3")],
        );
        let fp1 = Fingerprint::compute(&task1).unwrap();
        let fp2 = Fingerprint::compute(&task2).unwrap();
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_fingerprint_changes_with_files() {
        // Create temp files with different content
        let tmp_dir = std::env::temp_dir();
        let file1 = tmp_dir.join("loom_test_fp1.aura");
        let file2 = tmp_dir.join("loom_test_fp2.aura");

        std::fs::write(&file1, "fun main() {}").unwrap();
        std::fs::write(&file2, "fun main() { println(\"different\") }").unwrap();

        let task1 = make_task(
            "compile",
            TaskKind::Compile("main".to_string()),
            &[file1.to_str().unwrap()],
            &[],
        );
        let task2 = make_task(
            "compile",
            TaskKind::Compile("main".to_string()),
            &[file2.to_str().unwrap()],
            &[],
        );

        let fp1 = Fingerprint::compute(&task1).unwrap();
        let fp2 = Fingerprint::compute(&task2).unwrap();
        assert_ne!(fp1, fp2);

        // Cleanup
        let _ = std::fs::remove_file(&file1);
        let _ = std::fs::remove_file(&file2);
    }

    #[test]
    fn test_fingerprint_changes_with_kind() {
        let task1 = make_task("compile", TaskKind::Compile("main".to_string()), &[], &[]);
        let task2 = make_task("compile", TaskKind::Clean, &[], &[]);
        let fp1 = Fingerprint::compute(&task1).unwrap();
        let fp2 = Fingerprint::compute(&task2).unwrap();
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_fingerprint_dep_fingerprints() {
        let mut deps1 = HashMap::new();
        deps1.insert("compile-main".to_string(), "abc123".to_string());

        let mut deps2 = HashMap::new();
        deps2.insert("compile-main".to_string(), "def456".to_string());

        let task1 = TaskDefinition {
            name: "test".to_string(),
            description: String::new(),
            kind: TaskKind::Test,
            depends_on: vec!["compile-main".to_string()],
            inputs: TaskInputs {
                files: Vec::new(),
                options: HashMap::new(),
                dep_fingerprints: deps1,
            },
            outputs: TaskOutputs::default(),
        };

        let task2 = TaskDefinition {
            name: "test".to_string(),
            description: String::new(),
            kind: TaskKind::Test,
            depends_on: vec!["compile-main".to_string()],
            inputs: TaskInputs {
                files: Vec::new(),
                options: HashMap::new(),
                dep_fingerprints: deps2,
            },
            outputs: TaskOutputs::default(),
        };

        let fp1 = Fingerprint::compute(&task1).unwrap();
        let fp2 = Fingerprint::compute(&task2).unwrap();
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_hash_bytes() {
        let hash = hash_bytes(b"hello");
        assert_eq!(hash.len(), 32); // SHA-256 = 32 bytes
    }

    #[test]
    fn test_hash_bytes_deterministic() {
        let h1 = hash_bytes(b"test data");
        let h2 = hash_bytes(b"test data");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_bytes_different() {
        let h1 = hash_bytes(b"test data 1");
        let h2 = hash_bytes(b"test data 2");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_compute_detailed() {
        let task = make_task(
            "compile",
            TaskKind::Compile("main".to_string()),
            &[],
            &[("opt", "2")],
        );
        let fp = Fingerprint::compute_detailed(&task).unwrap();
        assert_eq!(fp.task_name, "compile");
        assert_eq!(fp.options.len(), 1);
        assert_eq!(fp.options[0], ("opt".to_string(), "2".to_string()));
        assert_eq!(fp.hash.len(), 64);
    }

    #[test]
    fn test_fingerprint_equals() {
        let fp = Fingerprint::from_cache("abc123def456");
        assert!(fp.equals("abc123def456"));
        assert!(!fp.equals("other"));
    }

    #[test]
    fn test_options_sorted_deterministic() {
        // Options should be sorted before hashing to ensure determinism
        let task1 = make_task(
            "compile",
            TaskKind::Compile("main".to_string()),
            &[],
            &[
                ("b", "2"),
                ("a", "1"),
            ],
        );
        let task2 = make_task(
            "compile",
            TaskKind::Compile("main".to_string()),
            &[],
            &[
                ("a", "1"),
                ("b", "2"),
            ],
        );
        let fp1 = Fingerprint::compute(&task1).unwrap();
        let fp2 = Fingerprint::compute(&task2).unwrap();
        assert_eq!(fp1, fp2); // Should be the same since options are sorted
    }
}
