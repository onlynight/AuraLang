//! [Phase B6.2] 本地注册表管理

use crate::error::LoomError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 本地索引条目
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CacheEntry {
    pub name: String,
    pub version: String,
    pub checksum: Option<String>,
    pub installed_at: Option<String>,
}

/// 本地索引（单包）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CacheIndex {
    pub name: String,
    #[serde(default)]
    pub versions: Vec<String>,
    #[serde(default)]
    pub entries: Vec<CacheEntry>,
}

/// 全局索引
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct GlobalIndex {
    #[serde(default)]
    pub packages: Vec<String>,
    pub updated_at: Option<String>,
}

/// 注册表统计信息
#[derive(Debug, Clone, Default)]
pub struct RegistryStats {
    pub total_packages: usize,
    pub total_versions: usize,
    pub total_size: u64,
    pub root_dir: String,
}

/// 本地注册表管理器
pub struct LocalRegistry {
    root_dir: PathBuf,
}

impl LocalRegistry {
    pub fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    pub fn from_default() -> Result<Self, LoomError> {
        let home = std::env::var("AURA_HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .or_else(|_| std::env::var("HOME"))
            .map_err(|e| LoomError::Config(format!("无法获取主目录: {}", e)))?;
        let root = PathBuf::from(home).join(".aura").join("registry");
        Ok(Self::new(root))
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    fn ensure_dir(&self) -> Result<(), LoomError> {
        std::fs::create_dir_all(&self.root_dir)
            .map_err(|e| LoomError::Registry(format!("创建注册表目录失败: {}", e)))
    }

    pub fn install(
        &self,
        name: &str,
        version: &str,
        artifact_path: &Path,
        checksum: Option<&str>,
    ) -> Result<CacheEntry, LoomError> {
        self.ensure_dir()?;
        let package_dir = self.root_dir.join(name);
        let version_dir = package_dir.join(version);
        std::fs::create_dir_all(&version_dir)
            .map_err(|e| LoomError::Registry(format!("创建版本目录失败: {}", e)))?;

        let dest_artifact = version_dir.join(format!("{}.auz", name));
        std::fs::copy(artifact_path, &dest_artifact).map_err(|e| {
            LoomError::Registry(format!(
                "复制制品文件失败 {}: {}",
                artifact_path.display(),
                e
            ))
        })?;

        let checksum = if let Some(c) = checksum {
            Some(c.to_string())
        } else {
            let content = std::fs::read(&dest_artifact)
                .map_err(|e| LoomError::Registry(format!("读取制品文件失败: {}", e)))?;
            let hash = sha256_hash(&content);
            Some(format!("sha256:{}", hash))
        };

        let metadata = CacheEntry {
            name: name.to_string(),
            version: version.to_string(),
            checksum: checksum.clone(),
            installed_at: Some(current_timestamp()),
        };
        let metadata_path = version_dir.join("metadata.json");
        let json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| LoomError::Registry(format!("序列化元数据失败: {}", e)))?;
        std::fs::write(&metadata_path, json)
            .map_err(|e| LoomError::Registry(format!("写入元数据失败: {}", e)))?;

        self.update_index(name, version, &metadata)?;
        Ok(metadata)
    }

    pub fn find(&self, name: &str, version: &str) -> Option<PathBuf> {
        let version_dir = self.root_dir.join(name).join(version);
        if version_dir.exists() { Some(version_dir.join(format!("{}.auz", name))) } else { None }
    }

    pub fn find_latest(&self, name: &str) -> Option<(String, PathBuf)> {
        let index_path = self.root_dir.join(name).join("index.json");
        if !index_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&index_path).ok()?;
        let index: CacheIndex = serde_json::from_str(&content).ok()?;
        index.versions.last().map(|v| {
            let path = self.root_dir.join(name).join(v).join(format!("{}.auz", name));
            (v.clone(), path)
        })
    }

    pub fn versions(&self, name: &str) -> Result<Vec<CacheEntry>, LoomError> {
        let index_path = self.root_dir.join(name).join("index.json");
        if !index_path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&index_path)
            .map_err(|e| LoomError::Registry(format!("读取索引失败: {}", e)))?;
        let index: CacheIndex = serde_json::from_str(&content)
            .map_err(|e| LoomError::Registry(format!("解析索引失败: {}", e)))?;
        Ok(index.entries)
    }

    pub fn list_packages(&self) -> Result<Vec<String>, LoomError> {
        self.ensure_dir()?;
        let mut packages = Vec::new();
        let entries = std::fs::read_dir(&self.root_dir)
            .map_err(|e| LoomError::Registry(format!("读取注册表目录失败: {}", e)))?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                packages.push(name);
            }
        }
        packages.sort();
        Ok(packages)
    }

    pub fn remove(&self, name: &str, version: &str) -> Result<(), LoomError> {
        let version_dir = self.root_dir.join(name).join(version);
        if !version_dir.exists() {
            return Err(LoomError::Registry(format!(
                "包 {}@{} 未安装",
                name, version
            )));
        }
        std::fs::remove_dir_all(&version_dir)
            .map_err(|e| LoomError::Registry(format!("删除版本目录失败: {}", e)))?;
        self.update_index(name, version, &CacheEntry::default())?;

        let package_dir = self.root_dir.join(name);
        if package_dir.exists() {
            let has_entries = std::fs::read_dir(&package_dir)
                .map(|mut d| {
                    d.any(|e| {
                        e.ok()
                            .map(|e| e.file_name().to_string_lossy() != "index.json")
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            if !has_entries {
                std::fs::remove_dir_all(&package_dir)
                    .map_err(|e| LoomError::Registry(format!("删除包目录失败: {}", e)))?;
            }
        }
        self.update_global_index()?;
        Ok(())
    }

    pub fn stats(&self) -> Result<RegistryStats, LoomError> {
        self.ensure_dir()?;
        let mut total_packages = 0;
        let mut total_versions = 0;
        let mut total_size = 0u64;
        let packages = self.list_packages()?;
        for name in &packages {
            total_packages += 1;
            let entries = self.versions(name)?;
            total_versions += entries.len();
            for entry in &entries {
                let artifact_path =
                    self.root_dir.join(name).join(&entry.version).join(format!("{}.auz", name));
                if artifact_path.exists() {
                    if let Ok(metadata) = std::fs::metadata(&artifact_path) {
                        total_size += metadata.len();
                    }
                }
            }
        }
        Ok(RegistryStats {
            total_packages,
            total_versions,
            total_size,
            root_dir: self.root_dir.to_string_lossy().to_string(),
        })
    }

    pub fn cleanup(&self) -> Result<u64, LoomError> {
        let mut removed = 0u64;
        self.ensure_dir()?;
        let entries = std::fs::read_dir(&self.root_dir)
            .map_err(|e| LoomError::Registry(format!("读取注册表目录失败: {}", e)))?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let package_dir = self.root_dir.join(&name);
                let version_entries = std::fs::read_dir(&package_dir)
                    .map_err(|e| LoomError::Registry(format!("读取包目录失败: {}", e)))?;
                for version_entry in version_entries {
                    let version_entry = match version_entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    if version_entry.file_type()?.is_dir() {
                        let version_dir = version_entry.path();
                        let has_files = std::fs::read_dir(&version_dir)
                            .map(|mut d| {
                                d.any(|e| {
                                    e.ok()
                                        .map(|e| {
                                            e.file_name()
                                                .to_string_lossy()
                                                .to_string()
                                                .ends_with(".auz")
                                        })
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(false);
                        if !has_files {
                            std::fs::remove_dir_all(&version_dir)
                                .map_err(|e| LoomError::Registry(format!("清理失败: {}", e)))?;
                            removed += 1;
                        }
                    }
                }
            }
        }
        Ok(removed)
    }

    fn update_index(&self, name: &str, version: &str, entry: &CacheEntry) -> Result<(), LoomError> {
        let package_dir = self.root_dir.join(name);
        std::fs::create_dir_all(&package_dir)
            .map_err(|e| LoomError::Registry(format!("创建包目录失败: {}", e)))?;
        let index_path = package_dir.join("index.json");
        let mut index = if index_path.exists() {
            let content = std::fs::read_to_string(&index_path)
                .map_err(|e| LoomError::Registry(format!("读取索引失败: {}", e)))?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            CacheIndex {
                name: name.to_string(),
                ..Default::default()
            }
        };
        if index.name.is_empty() {
            index.name = name.to_string();
        }
        if !entry.version.is_empty() {
            if !index.versions.contains(&entry.version) {
                index.versions.push(entry.version.clone());
                index.versions.sort();
            }
            let existing = index.entries.iter().position(|e| e.version == entry.version);
            if let Some(pos) = existing {
                index.entries[pos] = entry.clone();
            } else {
                index.entries.push(entry.clone());
            }
        }
        let json = serde_json::to_string_pretty(&index)
            .map_err(|e| LoomError::Registry(format!("序列化索引失败: {}", e)))?;
        std::fs::write(&index_path, json)
            .map_err(|e| LoomError::Registry(format!("写入索引失败: {}", e)))?;
        self.update_global_index()?;
        Ok(())
    }

    fn update_global_index(&self) -> Result<(), LoomError> {
        let index_path = self.root_dir.join("index.json");
        let mut index = if index_path.exists() {
            let content = std::fs::read_to_string(&index_path)
                .map_err(|e| LoomError::Registry(format!("读取全局索引失败: {}", e)))?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            GlobalIndex::default()
        };
        index.packages = self.list_packages()?;
        index.updated_at = Some(current_timestamp());
        let json = serde_json::to_string_pretty(&index)
            .map_err(|e| LoomError::Registry(format!("序列化全局索引失败: {}", e)))?;
        std::fs::write(&index_path, json)
            .map_err(|e| LoomError::Registry(format!("写入全局索引失败: {}", e)))?;
        Ok(())
    }
}

fn sha256_hash(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn current_timestamp() -> String {
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}", now.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_registry() -> (LocalRegistry, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let registry = LocalRegistry::new(dir.path().to_path_buf());
        (registry, dir)
    }

    #[test]
    fn test_local_registry_new() {
        let registry = LocalRegistry::new(PathBuf::from("/tmp/test-registry"));
        assert_eq!(registry.root_dir(), Path::new("/tmp/test-registry"));
    }

    #[test]
    fn test_install_and_find() {
        let (registry, _dir) = temp_registry();
        let artifact = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(artifact.path(), b"test artifact").unwrap();
        let entry = registry.install("test-pkg", "1.0.0", artifact.path(), None).unwrap();
        assert_eq!(entry.name, "test-pkg");
        assert_eq!(entry.version, "1.0.0");
        assert!(entry.checksum.is_some());
        let found = registry.find("test-pkg", "1.0.0");
        assert!(found.is_some());
        assert!(found.unwrap().exists());
    }

    #[test]
    fn test_find_latest() {
        let (registry, _dir) = temp_registry();
        for version in &[
            "1.0.0", "1.1.0",
        ] {
            let artifact = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(artifact.path(), format!("artifact {}", version)).unwrap();
            registry.install("test-pkg", version, artifact.path(), None).unwrap();
        }
        let result = registry.find_latest("test-pkg");
        assert!(result.is_some());
        let (ver, _path) = result.unwrap();
        assert_eq!(ver, "1.1.0");
    }

    #[test]
    fn test_versions() {
        let (registry, _dir) = temp_registry();
        let artifact = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(artifact.path(), b"artifact").unwrap();
        registry.install("test-pkg", "1.0.0", artifact.path(), None).unwrap();
        let versions = registry.versions("test-pkg").unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, "1.0.0");
    }

    #[test]
    fn test_list_packages() {
        let (registry, _dir) = temp_registry();
        for name in &[
            "pkg1", "pkg2",
        ] {
            let artifact = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(artifact.path(), format!("artifact {}", name)).unwrap();
            registry.install(name, "1.0.0", artifact.path(), None).unwrap();
        }
        let packages = registry.list_packages().unwrap();
        assert_eq!(packages.len(), 2);
        assert!(packages.contains(&"pkg1".to_string()));
        assert!(packages.contains(&"pkg2".to_string()));
    }

    #[test]
    fn test_remove() {
        let (registry, _dir) = temp_registry();
        let artifact = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(artifact.path(), b"artifact").unwrap();
        registry.install("test-pkg", "1.0.0", artifact.path(), None).unwrap();
        assert!(registry.find("test-pkg", "1.0.0").is_some());
        registry.remove("test-pkg", "1.0.0").unwrap();
        assert!(registry.find("test-pkg", "1.0.0").is_none());
    }

    #[test]
    fn test_remove_not_installed() {
        let (registry, _dir) = temp_registry();
        let result = registry.remove("nonexistent", "1.0.0");
        assert!(result.is_err());
    }

    #[test]
    fn test_stats() {
        let (registry, _dir) = temp_registry();
        let artifact = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(artifact.path(), b"artifact").unwrap();
        registry.install("test-pkg", "1.0.0", artifact.path(), None).unwrap();
        let stats = registry.stats().unwrap();
        assert_eq!(stats.total_packages, 1);
        assert_eq!(stats.total_versions, 1);
        assert!(stats.total_size > 0);
    }

    #[test]
    fn test_install_with_checksum() {
        let (registry, _dir) = temp_registry();
        let artifact = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(artifact.path(), b"test artifact").unwrap();
        let entry = registry
            .install(
                "test-pkg",
                "1.0.0",
                artifact.path(),
                Some("sha256:custom-hash"),
            )
            .unwrap();
        assert_eq!(entry.checksum, Some("sha256:custom-hash".to_string()));
    }

    #[test]
    fn test_cache_index_serialize() {
        let index = CacheIndex {
            name: "test-pkg".to_string(),
            versions: vec![
                "1.0.0".to_string(),
                "1.1.0".to_string(),
            ],
            entries: vec![
                CacheEntry {
                    name: "test-pkg".to_string(),
                    version: "1.0.0".to_string(),
                    checksum: Some("sha256:abc".to_string()),
                    installed_at: Some("1234567890".to_string()),
                },
            ],
        };
        let json = serde_json::to_string(&index).unwrap();
        let parsed: CacheIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "test-pkg");
        assert_eq!(parsed.versions.len(), 2);
    }

    #[test]
    fn test_sha256_hash() {
        let hash = sha256_hash(b"hello");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
