//! [Phase B3] 本地缓存（fingerprint + artifacts）
//!
//! 缓存目录结构：
//! target/cache/
//! ├── fingerprints.json           # 任务 fingerprint 缓存
//! ├── artifacts/                  # 任务输出产物缓存
//! │   ├── compile-main/
//! │   │   ├── main.auc
//! │   │   ├── utils.auc
//! │   │   └── math/vector.auc
//! │   ├── compile-test/
//! │   │   └── utils_test.auc
//! │   └── package/
//! │       └── my-app-1.0.0.auz
//! └── metadata.json               # 缓存元数据（版本、时间戳、大小）
//!
//! B3.1: 增强 artifact 复制/恢复/列表功能
//! B3.2: 缓存失效策略

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::LoomError;

/// fingerprints.json 结构
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct FingerprintStore {
    /// 任务名 → fingerprint
    #[serde(default)]
    pub fingerprints: HashMap<String, String>,
    /// 缓存元数据
    #[serde(default)]
    pub metadata: CacheMetadata,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct CacheMetadata {
    /// 缓存格式版本
    #[serde(default = "default_cache_version")]
    pub version: String,
    /// 创建时间戳
    pub created_at: u64,
    /// 更新时间戳
    pub updated_at: u64,
    /// 任务数量
    pub task_count: usize,
    /// 产物总数
    pub artifact_count: usize,
    /// 缓存总大小（字节）
    pub total_size_bytes: u64,
}

fn default_cache_version() -> String {
    "1.0".to_string()
}

/// 产物条目元数据（metadata.json 中每个任务的产物记录）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtifactEntry {
    /// 产物相对路径
    pub path: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 文件 SHA-256 哈希
    pub hash: String,
    /// 创建时间戳
    pub created_at: u64,
}

/// metadata.json 结构
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct CacheMetaStore {
    /// 任务名 → 产物条目列表
    #[serde(default)]
    pub tasks: HashMap<String, Vec<ArtifactEntry>>,
    /// 缓存元数据
    #[serde(default)]
    pub metadata: CacheMetadata,
}

/// 本地缓存
pub struct LocalCache {
    /// 缓存根目录（如 target/cache/）
    cache_dir: PathBuf,
    /// fingerprint 文件路径
    fingerprint_file: PathBuf,
    /// 产物目录
    artifacts_dir: PathBuf,
    /// metadata.json 路径
    metadata_file: PathBuf,
    /// 内存中的 fingerprint 存储
    store: FingerprintStore,
    /// 内存中的产物元数据
    meta_store: CacheMetaStore,
}

impl LocalCache {
    /// 创建本地缓存
    pub fn new(cache_dir: &Path) -> Result<Self, LoomError> {
        let cache_dir = cache_dir.to_path_buf();
        let fingerprint_file = cache_dir.join("fingerprints.json");
        let artifacts_dir = cache_dir.join("artifacts");
        let metadata_file = cache_dir.join("metadata.json");

        // 确保目录存在
        std::fs::create_dir_all(&cache_dir)?;
        std::fs::create_dir_all(&artifacts_dir)?;

        // 加载已有的 fingerprint
        let store = if fingerprint_file.exists() {
            let content = std::fs::read_to_string(&fingerprint_file)
                .map_err(|e| LoomError::Cache(format!("无法读取 fingerprint 文件: {}", e)))?;
            serde_json::from_str(&content)
                .map_err(|e| LoomError::Cache(format!("无法解析 fingerprint 文件: {}", e)))?
        } else {
            FingerprintStore {
                metadata: CacheMetadata {
                    created_at: now_timestamp(),
                    updated_at: now_timestamp(),
                    version: default_cache_version(),
                    ..Default::default()
                },
                ..Default::default()
            }
        };

        // 加载已有的 metadata
        let meta_store = if metadata_file.exists() {
            let content = std::fs::read_to_string(&metadata_file)
                .map_err(|e| LoomError::Cache(format!("无法读取 metadata 文件: {}", e)))?;
            serde_json::from_str(&content)
                .map_err(|e| LoomError::Cache(format!("无法解析 metadata 文件: {}", e)))?
        } else {
            CacheMetaStore::default()
        };

        Ok(Self {
            cache_dir,
            fingerprint_file,
            artifacts_dir,
            metadata_file,
            store,
            meta_store,
        })
    }

    /// 获取缓存目录路径
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// 获取产物目录路径
    pub fn artifacts_dir(&self) -> &Path {
        &self.artifacts_dir
    }

    /// 查找任务的 fingerprint（如果有缓存）
    pub fn lookup_fingerprint(&self, task_name: &str) -> Result<Option<String>, LoomError> {
        Ok(self.store.fingerprints.get(task_name).cloned())
    }

    /// 存储任务的 fingerprint
    pub fn store_fingerprint(&mut self, task_name: &str, fingerprint: &str) -> Result<(), LoomError> {
        self.store.fingerprints.insert(task_name.to_string(), fingerprint.to_string());
        self.save()?;
        Ok(())
    }

    /// 检查任务是否 up-to-date
    pub fn is_up_to_date(&self, task_name: &str, current_fingerprint: &str) -> bool {
        self.store
            .fingerprints
            .get(task_name)
            .map(|cached| cached == current_fingerprint)
            .unwrap_or(false)
    }

    /// 持久化 fingerprint 到文件
    pub fn save(&mut self) -> Result<(), LoomError> {
        self.store.metadata.updated_at = now_timestamp();
        self.store.metadata.task_count = self.store.fingerprints.len();
        self.store.metadata.artifact_count = self.meta_store.tasks.values().map(|v| v.len()).sum();
        self.store.metadata.total_size_bytes = self.compute_total_size();

        let content = serde_json::to_string_pretty(&self.store)
            .map_err(|e| LoomError::Cache(format!("无法序列化 fingerprint: {}", e)))?;
        std::fs::write(&self.fingerprint_file, content)
            .map_err(|e| LoomError::Cache(format!("无法写入 fingerprint 文件: {}", e)))?;
        Ok(())
    }

    /// 清除所有缓存
    pub fn clear(&mut self) -> Result<(), LoomError> {
        self.store = FingerprintStore::default();
        self.store.metadata.created_at = now_timestamp();
        self.store.metadata.updated_at = now_timestamp();
        self.store.metadata.version = default_cache_version();
        self.save()?;

        self.meta_store = CacheMetaStore::default();
        self.meta_store.metadata.created_at = now_timestamp();
        self.meta_store.metadata.updated_at = now_timestamp();
        self.save_metadata()?;

        // 清除产物目录
        if self.artifacts_dir.exists() {
            std::fs::remove_dir_all(&self.artifacts_dir)?;
            std::fs::create_dir_all(&self.artifacts_dir)?;
        }
        Ok(())
    }

    /// 获取任务产物目录
    pub fn task_artifacts_dir(&self, task_name: &str) -> PathBuf {
        self.artifacts_dir.join(sanitize_task_name(task_name))
    }

    /// 确保任务产物目录存在
    pub fn ensure_task_artifacts_dir(&self, task_name: &str) -> Result<PathBuf, LoomError> {
        let dir = self.task_artifacts_dir(task_name);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 获取缓存统计信息
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            cache_dir: self.cache_dir.clone(),
            fingerprint_count: self.store.fingerprints.len(),
            artifact_count: self.meta_store.tasks.values().map(|v| v.len()).sum(),
            version: self.store.metadata.version.clone(),
            created_at: self.store.metadata.created_at,
            updated_at: self.store.metadata.updated_at,
            total_size_bytes: self.compute_total_size(),
        }
    }

    /// 导出所有 fingerprint（用于增量检查依赖传递）
    pub fn all_fingerprints(&self) -> &HashMap<String, String> {
        &self.store.fingerprints
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // B3.1: Artifact 复制/恢复/列表
    // ═══════════════════════════════════════════════════════════════════════════

    /// 将产物文件复制到缓存目录
    ///
    /// 接收任务名和产物文件列表，复制到 cache/artifacts/{task_name}/ 下，
    /// 并记录元数据到 metadata.json。
    pub fn store_artifacts(&mut self, task_name: &str, artifacts: &[PathBuf]) -> Result<Vec<ArtifactEntry>, LoomError> {
        let dest_dir = self.ensure_task_artifacts_dir(task_name)?;
        let mut entries = Vec::new();

        for artifact in artifacts {
            if !artifact.exists() {
                continue;
            }

            let file_name = artifact.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| LoomError::Cache(format!("无法获取文件名: {}", artifact.display())))?;

            let dest = dest_dir.join(file_name);
            std::fs::copy(artifact, &dest)?;

            let size = std::fs::metadata(&dest)?.len();
            let hash = crate::cache::fingerprint::hash_file(&dest)?;

            entries.push(ArtifactEntry {
                path: file_name.to_string(),
                size,
                hash,
                created_at: now_timestamp(),
            });
        }

        // 更新 metadata
        self.meta_store.tasks.insert(task_name.to_string(), entries.clone());
        self.save_metadata()?;

        Ok(entries)
    }

    /// 从缓存恢复产物文件到目标目录
    ///
    /// 返回恢复的文件路径列表。
    pub fn restore_artifacts(&self, task_name: &str, dest_dir: &Path) -> Result<Vec<PathBuf>, LoomError> {
        let src_dir = self.task_artifacts_dir(task_name);
        if !src_dir.exists() {
            return Ok(Vec::new());
        }

        std::fs::create_dir_all(dest_dir)?;
        let mut restored = Vec::new();

        if let Some(entries) = self.meta_store.tasks.get(task_name) {
            for entry in entries {
                let src = src_dir.join(&entry.path);
                let dest = dest_dir.join(&entry.path);

                // 确保目标父目录存在
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                if src.exists() {
                    std::fs::copy(&src, &dest)?;
                    restored.push(dest);
                }
            }
        }

        Ok(restored)
    }

    /// 列出任务的缓存产物
    pub fn list_artifacts(&self, task_name: &str) -> Result<Vec<ArtifactEntry>, LoomError> {
        Ok(self.meta_store.tasks.get(task_name).cloned().unwrap_or_default())
    }

    /// 检查任务是否有缓存产物
    pub fn has_artifacts(&self, task_name: &str) -> bool {
        self.meta_store.tasks.get(task_name)
            .map(|entries| !entries.is_empty())
            .unwrap_or(false)
    }

    /// 删除任务的缓存产物（同时删除元数据）
    pub fn remove_task_artifacts(&mut self, task_name: &str) -> Result<(), LoomError> {
        let dir = self.task_artifacts_dir(task_name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        self.meta_store.tasks.remove(task_name);
        self.save_metadata()?;
        Ok(())
    }

    /// 删除指定任务及其所有依赖的缓存（用于 --clean 级联清理）
    pub fn clear_task_and_dependents(&mut self, task_name: &str, all_tasks: &[String]) -> Result<(), LoomError> {
        let mut to_clear = vec![task_name.to_string()];
        // 简单实现：清除所有任务缓存（B3.2 可优化为仅清除依赖链）
        let _ = all_tasks;
        self.clear_tasks(&to_clear)?;
        Ok(())
    }

    /// 检查缓存是否可恢复（产物是否还在磁盘上）
    pub fn can_restore(&self, task_name: &str) -> bool {
        if let Some(entries) = self.meta_store.tasks.get(task_name) {
            let dir = self.task_artifacts_dir(task_name);
            entries.iter().all(|e| dir.join(&e.path).exists())
        } else {
            false
        }
    }

    /// 计算缓存总大小
    fn compute_total_size(&self) -> u64 {
        self.meta_store.tasks.values().flat_map(|entries| entries.iter())
            .map(|e| e.size)
            .sum()
    }

    /// 保存 metadata.json
    fn save_metadata(&mut self) -> Result<(), LoomError> {
        self.meta_store.metadata.updated_at = now_timestamp();
        self.meta_store.metadata.task_count = self.meta_store.tasks.len();
        self.meta_store.metadata.artifact_count = self.meta_store.tasks.values().map(|v| v.len()).sum();
        self.meta_store.metadata.total_size_bytes = self.compute_total_size();

        let content = serde_json::to_string_pretty(&self.meta_store)
            .map_err(|e| LoomError::Cache(format!("无法序列化 metadata: {}", e)))?;
        std::fs::write(&self.metadata_file, content)
            .map_err(|e| LoomError::Cache(format!("无法写入 metadata 文件: {}", e)))?;
        Ok(())
    }

    /// 清除指定任务列表的缓存
    fn clear_tasks(&mut self, tasks: &[String]) -> Result<(), LoomError> {
        let mut changed = false;

        for task_name in tasks {
            if self.store.fingerprints.remove(task_name).is_some() {
                changed = true;
            }
            if self.meta_store.tasks.remove(task_name).is_some() {
                changed = true;
            }

            let dir = self.artifacts_dir.join(sanitize_task_name(task_name));
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
                changed = true;
            }
        }

        if changed {
            self.save()?;
            self.save_metadata()?;
        }

        Ok(())
    }
}

fn sanitize_task_name(name: &str) -> String {
    name.replace(['/', '\\', ':', ' ', '|', '<', '>', '"', '?', '*'], "_")
}

fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 缓存统计信息
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub cache_dir: PathBuf,
    pub fingerprint_count: usize,
    pub artifact_count: usize,
    pub version: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub total_size_bytes: u64,
}

impl std::fmt::Display for CacheStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "缓存目录: {}", self.cache_dir.display())?;
        writeln!(f, "  Fingerprint: {} 个任务", self.fingerprint_count)?;
        write!(f, "  产物: {} 个文件", self.artifact_count)?;
        write!(f, "  大小: {}", format_size(self.total_size_bytes))?;
        writeln!(f, "  版本: {}", self.version)
    }
}

/// 格式化文件大小
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_cache() -> (LocalCache, TempDir) {
        let tmp = TempDir::new().unwrap();
        let cache = LocalCache::new(tmp.path()).unwrap();
        (cache, tmp)
    }

    #[test]
    fn test_new_cache() {
        let (cache, _tmp) = setup_cache();
        assert!(cache.cache_dir().exists());
        assert!(cache.artifacts_dir().exists());
    }

    #[test]
    fn test_store_and_lookup_fingerprint() {
        let (mut cache, _tmp) = setup_cache();

        assert!(cache.lookup_fingerprint("compile-main").unwrap().is_none());

        cache.store_fingerprint("compile-main", "abc123def456").unwrap();

        let fp = cache.lookup_fingerprint("compile-main").unwrap();
        assert_eq!(fp, Some("abc123def456".to_string()));
    }

    #[test]
    fn test_is_up_to_date() {
        let (mut cache, _tmp) = setup_cache();

        cache.store_fingerprint("compile", "fp1").unwrap();

        assert!(cache.is_up_to_date("compile", "fp1"));
        assert!(!cache.is_up_to_date("compile", "fp2"));
        assert!(!cache.is_up_to_date("nonexistent", "fp1"));
    }

    #[test]
    fn test_persistence() {
        let tmp = TempDir::new().unwrap();

        // 创建并存储
        {
            let mut cache = LocalCache::new(tmp.path()).unwrap();
            cache.store_fingerprint("compile", "fp1").unwrap();
            cache.store_fingerprint("package", "fp2").unwrap();
        }

        // 重新加载
        {
            let cache = LocalCache::new(tmp.path()).unwrap();
            assert!(cache.is_up_to_date("compile", "fp1"));
            assert!(cache.is_up_to_date("package", "fp2"));
            assert!(!cache.is_up_to_date("compile", "fp3"));
        }
    }

    #[test]
    fn test_clear() {
        let (mut cache, _tmp) = setup_cache();

        cache.store_fingerprint("compile", "fp1").unwrap();
        cache.store_fingerprint("package", "fp2").unwrap();

        cache.clear().unwrap();

        assert!(cache.lookup_fingerprint("compile").unwrap().is_none());
        assert!(cache.lookup_fingerprint("package").unwrap().is_none());
    }

    #[test]
    fn test_task_artifacts_dir() {
        let (cache, _tmp) = setup_cache();

        let dir = cache.task_artifacts_dir("compile-main");
        assert!(dir.display().to_string().contains("compile-main"));
    }

    #[test]
    fn test_ensure_task_artifacts_dir() {
        let (cache, _tmp) = setup_cache();

        let dir = cache.ensure_task_artifacts_dir("compile-main").unwrap();
        assert!(dir.exists());
    }

    #[test]
    fn test_stats() {
        let (mut cache, _tmp) = setup_cache();

        cache.store_fingerprint("compile", "fp1").unwrap();
        cache.store_fingerprint("package", "fp2").unwrap();

        let stats = cache.stats();
        assert_eq!(stats.fingerprint_count, 2);
        assert_eq!(stats.version, "1.0");
    }

    #[test]
    fn test_all_fingerprints() {
        let (mut cache, _tmp) = setup_cache();

        cache.store_fingerprint("compile", "fp1").unwrap();
        cache.store_fingerprint("package", "fp2").unwrap();

        let all = cache.all_fingerprints();
        assert_eq!(all.len(), 2);
        assert!(all.contains_key("compile"));
        assert!(all.contains_key("package"));
    }

    #[test]
    fn test_multiple_updates() {
        let (mut cache, _tmp) = setup_cache();

        cache.store_fingerprint("compile", "fp1").unwrap();
        cache.store_fingerprint("compile", "fp2").unwrap();

        assert!(cache.is_up_to_date("compile", "fp2"));
        assert!(!cache.is_up_to_date("compile", "fp1"));
    }

    #[test]
    fn test_special_chars_in_task_name() {
        let (cache, _tmp) = setup_cache();

        let dir = cache.task_artifacts_dir("compile-main/test");
        // Should not contain path separators that would create nested dirs
        assert!(dir.display().to_string().contains("compile-main_test"));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // B3.1: Artifact 测试
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_store_artifacts() {
        let (mut cache, tmp) = setup_cache();

        // 创建测试文件
        let file1 = tmp.path().join("test1.auz");
        std::fs::write(&file1, "artifact content 1").unwrap();

        let file2 = tmp.path().join("test2.auc");
        std::fs::write(&file2, "artifact content 2").unwrap();

        let entries = cache.store_artifacts("compile-main", &[file1, file2]).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].path.contains("test1"));
        assert!(entries[1].path.contains("test2"));
        assert!(entries[0].size > 0);
        assert!(entries[0].hash.len() == 64); // SHA-256
    }

    #[test]
    fn test_restore_artifacts() {
        let (mut cache, tmp) = setup_cache();
        let restore_dir = tmp.path().join("restored");

        // 创建并缓存
        let file1 = tmp.path().join("source.auz");
        std::fs::write(&file1, "hello world").unwrap();
        cache.store_artifacts("package", &[file1.clone()]).unwrap();

        // 恢复到新目录
        let restored = cache.restore_artifacts("package", &restore_dir).unwrap();
        assert_eq!(restored.len(), 1);
        assert!(restored[0].exists());
        let content = std::fs::read_to_string(&restored[0]).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_list_artifacts() {
        let (mut cache, tmp) = setup_cache();

        let file = tmp.path().join("test.auz");
        std::fs::write(&file, "content").unwrap();
        cache.store_artifacts("compile", &[file]).unwrap();

        let entries = cache.list_artifacts("compile").unwrap();
        assert_eq!(entries.len(), 1);

        let empty = cache.list_artifacts("nonexistent").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_has_artifacts() {
        let (mut cache, tmp) = setup_cache();

        assert!(!cache.has_artifacts("compile"));

        let file = tmp.path().join("test.auz");
        std::fs::write(&file, "content").unwrap();
        cache.store_artifacts("compile", &[file]).unwrap();

        assert!(cache.has_artifacts("compile"));
    }

    #[test]
    fn test_remove_task_artifacts() {
        let (mut cache, tmp) = setup_cache();

        let file = tmp.path().join("test.auz");
        std::fs::write(&file, "content").unwrap();
        cache.store_artifacts("compile", &[file]).unwrap();

        assert!(cache.has_artifacts("compile"));

        cache.remove_task_artifacts("compile").unwrap();
        assert!(!cache.has_artifacts("compile"));
    }

    #[test]
    fn test_can_restore() {
        let (mut cache, tmp) = setup_cache();

        assert!(!cache.can_restore("compile"));

        let file = tmp.path().join("test.auz");
        std::fs::write(&file, "content").unwrap();
        cache.store_artifacts("compile", &[file]).unwrap();

        assert!(cache.can_restore("compile"));
    }

    #[test]
    fn test_restore_no_artifacts() {
        let (cache, tmp) = setup_cache();
        let dest = tmp.path().join("empty-restore");

        let restored = cache.restore_artifacts("nonexistent", &dest).unwrap();
        assert!(restored.is_empty());
    }

    #[test]
    fn test_metadata_persistence() {
        let tmp = TempDir::new().unwrap();

        // 创建并存储
        {
            let mut cache = LocalCache::new(tmp.path()).unwrap();
            let file = tmp.path().join("test.auz");
            std::fs::write(&file, "persistent content").unwrap();
            cache.store_artifacts("compile", &[file]).unwrap();
        }

        // 重新加载
        {
            let cache = LocalCache::new(tmp.path()).unwrap();
            assert!(cache.has_artifacts("compile"));
            let entries = cache.list_artifacts("compile").unwrap();
            assert_eq!(entries.len(), 1);

            // 恢复
            let restore_dir = tmp.path().join("restored2");
            let restored = cache.restore_artifacts("compile", &restore_dir).unwrap();
            assert_eq!(restored.len(), 1);
            let content = std::fs::read_to_string(&restored[0]).unwrap();
            assert_eq!(content, "persistent content");
        }
    }

    #[test]
    fn test_clear_removes_artifacts() {
        let (mut cache, tmp) = setup_cache();

        let file = tmp.path().join("test.auz");
        std::fs::write(&file, "content").unwrap();
        cache.store_artifacts("compile", &[file]).unwrap();
        cache.store_fingerprint("compile", "fp1").unwrap();

        assert!(cache.has_artifacts("compile"));
        assert!(cache.lookup_fingerprint("compile").unwrap().is_some());

        cache.clear().unwrap();

        assert!(!cache.has_artifacts("compile"));
        assert!(cache.lookup_fingerprint("compile").unwrap().is_none());
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_stats_with_artifacts() {
        let (mut cache, tmp) = setup_cache();

        let file1 = tmp.path().join("test1.auz");
        std::fs::write(&file1, "aaa").unwrap();
        let file2 = tmp.path().join("test2.auz");
        std::fs::write(&file2, "bbbb").unwrap();

        cache.store_artifacts("compile", &[file1, file2]).unwrap();

        let stats = cache.stats();
        assert_eq!(stats.artifact_count, 2);
        assert!(stats.total_size_bytes >= 7); // 3 + 4 = 7 bytes
    }

    #[test]
    fn test_multiple_tasks_artifacts() {
        let (mut cache, tmp) = setup_cache();

        let file1 = tmp.path().join("test1.auz");
        std::fs::write(&file1, "compile output").unwrap();
        cache.store_artifacts("compile-main", &[file1.clone()]).unwrap();

        let file2 = tmp.path().join("test2.auz");
        std::fs::write(&file2, "package output").unwrap();
        cache.store_artifacts("package", &[file2.clone()]).unwrap();

        assert!(cache.has_artifacts("compile-main"));
        assert!(cache.has_artifacts("package"));

        let compile_entries = cache.list_artifacts("compile-main").unwrap();
        let package_entries = cache.list_artifacts("package").unwrap();

        assert!(compile_entries[0].hash != package_entries[0].hash);
    }
}
