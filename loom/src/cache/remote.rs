//! [Phase B3] 远程缓存（HTTP Build Cache）
//!
//! 协议设计（类 Gradle Build Cache）：
//!
//!   缓存键 = SHA256(
//!     任务名称
//!     + 任务输入指纹（源文件哈希 + 选项）
//!     + 编译器版本
//!     + 目标平台
//!     + 插件版本
//!   )
//!
//!   HTTP API:
//!     HEAD  /v1/cache/{key}        → 200 (exists) / 404 (not found)
//!     GET   /v1/cache/{key}        → 200 + JSON body (artifact metadata + base64 data)
//!     PUT   /v1/cache/{key}        → 201 (created) / 200 (updated)
//!     DELETE /v1/cache/{key}       → 204 (deleted)
//!
//!   认证：Authorization: Bearer <token>
//!   共享模式：shared=true 时允许所有客户端写入（CI 用）
//!
//! B3.3: 远程缓存协议
//! B3.4: 上传/下载 + 共享策略

use std::path::PathBuf;
use std::time::Duration;

use crate::cache::fingerprint::Fingerprint;
use crate::cache::local::CacheStats;
use crate::error::LoomError;
use crate::manifest::priority::ResolvedBuildConfig;
use crate::task::TaskDefinition;

/// 远程缓存配置
#[derive(Debug, Clone)]
pub struct RemoteCacheConfig {
    /// 远程缓存服务器 URL
    pub url: String,
    /// 认证 token（Bearer token）
    pub token: Option<String>,
    /// 是否共享模式（CI 写入，本地读取）
    pub shared: bool,
    /// 请求超时（秒）
    pub timeout_secs: u64,
    /// 最大重试次数
    pub max_retries: u32,
    /// 最大产物大小（字节，0 = 不限制）
    pub max_artifact_size: u64,
}

impl Default for RemoteCacheConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            token: None,
            shared: false,
            timeout_secs: 30,
            max_retries: 3,
            max_artifact_size: 100 * 1024 * 1024, // 100 MB
        }
    }
}

impl RemoteCacheConfig {
    /// 从 ResolvedBuildConfig 构建
    pub fn from_build_config(config: &ResolvedBuildConfig) -> Option<Self> {
        config.cache_remote.as_ref().map(|url| Self {
            url: url.clone(),
            token: std::env::var("AURA_CACHE_TOKEN").ok(),
            shared: config.cache_remote_shared,
            timeout_secs: 30,
            max_retries: 3,
            max_artifact_size: 100 * 1024 * 1024,
        })
    }
}

/// 远程缓存响应
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RemoteCacheResponse {
    /// 缓存键
    key: String,
    /// 任务名
    task_name: String,
    /// 产物列表
    artifacts: Vec<RemoteArtifact>,
    /// 创建时间戳
    created_at: u64,
    /// 编译器版本
    compiler_version: String,
    /// 目标平台
    target: Option<String>,
}

/// 远程产物（base64 编码）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RemoteArtifact {
    /// 文件名
    name: String,
    /// 文件大小
    size: u64,
    /// 文件 SHA-256 哈希
    hash: String,
    /// base64 编码的文件内容
    data: String,
}

/// 远程缓存
pub struct RemoteCache {
    config: RemoteCacheConfig,
    /// HTTP 客户端
    client: reqwest::blocking::Client,
}

impl RemoteCache {
    /// 创建远程缓存客户端
    pub fn new(config: RemoteCacheConfig) -> Result<Self, LoomError> {
        if config.url.is_empty() {
            return Err(LoomError::Cache("远程缓存 URL 为空".to_string()));
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| LoomError::Cache(format!("无法创建 HTTP 客户端: {}", e)))?;

        Ok(Self { config, client })
    }

    /// 计算远程缓存键（SHA-256）
    ///
    /// cache_key = SHA256(
    ///   任务名称
    ///   + 任务输入指纹（源文件哈希 + 选项）
    ///   + 编译器版本
    ///   + 目标平台
    ///   + 插件版本
    /// )
    pub fn compute_cache_key(task: &TaskDefinition, config: &ResolvedBuildConfig) -> Result<String, LoomError> {
        let fingerprint = Fingerprint::compute(task)?;
        Ok(fingerprint)
    }

    /// 检查远程缓存是否存在
    ///
    /// HEAD /v1/cache/{key} → 200 (exists) / 404 (not found)
    pub fn lookup(&self, cache_key: &str) -> Result<bool, LoomError> {
        let url = format!("{}/v1/cache/{}", self.config.url.trim_end_matches('/'), cache_key);

        for attempt in 0..=self.config.max_retries {
            let request = self.client.head(&url);
            let request = self.add_auth(request);

            match request.send() {
                Ok(response) => {
                    if response.status().is_success() {
                        return Ok(true);
                    } else if response.status().as_u16() == 404 {
                        return Ok(false);
                    } else {
                        let status = response.status().as_u16();
                        if attempt < self.config.max_retries {
                            self.wait_before_retry(attempt)?;
                            continue;
                        }
                        return Err(LoomError::Cache(format!(
                            "远程缓存查询失败: HTTP {}",
                            status
                        )));
                    }
                }
                Err(e) => {
                    if attempt < self.config.max_retries {
                        self.wait_before_retry(attempt)?;
                        continue;
                    }
                    return Err(LoomError::Cache(format!(
                        "远程缓存查询失败: {}",
                        e
                    )));
                }
            }
        }

        Ok(false)
    }

    /// 从远程缓存下载产物
    ///
    /// GET /v1/cache/{key} → 200 + JSON body
    ///
    /// 返回恢复的产物文件路径列表。
    pub fn download_artifacts(&self, cache_key: &str, dest_dir: &std::path::Path) -> Result<Vec<PathBuf>, LoomError> {
        let url = format!("{}/v1/cache/{}", self.config.url.trim_end_matches('/'), cache_key);

        for attempt in 0..=self.config.max_retries {
            let request = self.client.get(&url);
            let request = self.add_auth(request);

            match request.send() {
                Ok(response) => {
                    if response.status().is_success() {
                        let body_text = response.text()
                            .map_err(|e| LoomError::Cache(format!("无法读取远程缓存响应: {}", e)))?;
                        let body: RemoteCacheResponse = serde_json::from_str(&body_text)
                            .map_err(|e| LoomError::Cache(format!("无法解析远程缓存响应: {}", e)))?;

                        std::fs::create_dir_all(dest_dir)?;
                        let mut restored = Vec::new();

                        for artifact in &body.artifacts {
                            if self.config.max_artifact_size > 0 && artifact.size > self.config.max_artifact_size {
                                return Err(LoomError::Cache(format!(
                                    "远程产物过大 ({} > {}): {}",
                                    artifact.size,
                                    self.config.max_artifact_size,
                                    artifact.name
                                )));
                            }

                            let data = decode_base64(&artifact.data)?;

                            // 校验哈希
                            let hash = crate::cache::fingerprint::hash_bytes(&data);
                            let hash_hex = hex::encode(hash);
                            if hash_hex != artifact.hash {
                                return Err(LoomError::Cache(format!(
                                    "远程产物哈希不匹配: {} (期望 {}, 实际 {})",
                                    artifact.name,
                                    artifact.hash,
                                    hash_hex
                                )));
                            }

                            let dest = dest_dir.join(&artifact.name);
                            if let Some(parent) = dest.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            std::fs::write(&dest, &data)?;
                            restored.push(dest);
                        }

                        return Ok(restored);
                    } else if response.status().as_u16() == 404 {
                        return Err(LoomError::Cache(format!(
                            "远程缓存未命中: {}",
                            cache_key
                        )));
                    } else {
                        let status = response.status().as_u16();
                        if attempt < self.config.max_retries {
                            self.wait_before_retry(attempt)?;
                            continue;
                        }
                        return Err(LoomError::Cache(format!(
                            "远程缓存下载失败: HTTP {}",
                            status
                        )));
                    }
                }
                Err(e) => {
                    if attempt < self.config.max_retries {
                        self.wait_before_retry(attempt)?;
                        continue;
                    }
                    return Err(LoomError::Cache(format!(
                        "远程缓存下载失败: {}",
                        e
                    )));
                }
            }
        }

        Err(LoomError::Cache("远程缓存下载失败: 重试耗尽".to_string()))
    }

    /// 上传产物到远程缓存
    ///
    /// PUT /v1/cache/{key} → 201 (created) / 200 (updated)
    pub fn upload_artifacts(&self, cache_key: &str, task_name: &str, artifacts: &[PathBuf], config: &ResolvedBuildConfig) -> Result<(), LoomError> {
        let url = format!("{}/v1/cache/{}", self.config.url.trim_end_matches('/'), cache_key);

        // 构建上传 payload
        let remote_artifacts: Vec<RemoteArtifact> = artifacts
            .iter()
            .filter(|p| p.exists())
            .filter_map(|p| {
                let name = p.file_name()?.to_str()?.to_string();
                let data = std::fs::read(p).ok()?;
                let size = data.len() as u64;

                if self.config.max_artifact_size > 0 && size > self.config.max_artifact_size {
                    tracing::warn!(
                        "跳过过大产物 ({} > {}): {}",
                        size,
                        self.config.max_artifact_size,
                        name
                    );
                    return None;
                }

                let hash = crate::cache::fingerprint::hash_bytes(&data);
                let hash_hex = hex::encode(hash);
                let encoded = encode_base64(&data);

                Some(RemoteArtifact {
                    name,
                    size,
                    hash: hash_hex,
                    data: encoded,
                })
            })
            .collect();

        if remote_artifacts.is_empty() {
            return Ok(()); // 没有可上传的产物
        }

        let response = RemoteCacheResponse {
            key: cache_key.to_string(),
            task_name: task_name.to_string(),
            artifacts: remote_artifacts,
            created_at: now_timestamp(),
            compiler_version: crate::cache::fingerprint::COMPILER_VERSION.to_string(),
            target: config.target.clone(),
        };

        let body = serde_json::to_vec(&response)
            .map_err(|e| LoomError::Cache(format!("无法序列化缓存数据: {}", e)))?;

        for attempt in 0..=self.config.max_retries {
            let request = self.client.put(&url);
            let request = self.add_auth(request);
            let request = request.header("Content-Type", "application/json");

            match request.body(body.clone()).send() {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if status == 200 || status == 201 {
                        return Ok(());
                    } else if status == 401 {
                        return Err(LoomError::Cache(
                            "远程缓存认证失败: 请设置 AURA_CACHE_TOKEN 环境变量".to_string(),
                        ));
                    } else if status == 403 {
                        return Err(LoomError::Cache(
                            "远程缓存拒绝写入: 共享模式未启用或权限不足".to_string(),
                        ));
                    } else if status == 413 {
                        return Err(LoomError::Cache(
                            "远程缓存产物过大: 服务器拒绝".to_string(),
                        ));
                    } else if attempt < self.config.max_retries {
                        self.wait_before_retry(attempt)?;
                        continue;
                    } else {
                        return Err(LoomError::Cache(format!(
                            "远程缓存上传失败: HTTP {}",
                            status
                        )));
                    }
                }
                Err(e) => {
                    if attempt < self.config.max_retries {
                        self.wait_before_retry(attempt)?;
                        continue;
                    }
                    return Err(LoomError::Cache(format!(
                        "远程缓存上传失败: {}",
                        e
                    )));
                }
            }
        }

        Err(LoomError::Cache("远程缓存上传失败: 重试耗尽".to_string()))
    }

    /// 使远程缓存失效
    ///
    /// DELETE /v1/cache/{key} → 204
    pub fn invalidate(&self, cache_key: &str) -> Result<(), LoomError> {
        let url = format!("{}/v1/cache/{}", self.config.url.trim_end_matches('/'), cache_key);

        for attempt in 0..=self.config.max_retries {
            let request = self.client.delete(&url);
            let request = self.add_auth(request);

            match request.send() {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if status == 204 || status == 200 {
                        return Ok(());
                    } else if status == 404 {
                        return Ok(()); // 已不存在，视为成功
                    } else if status == 401 {
                        return Err(LoomError::Cache(
                            "远程缓存认证失败: 请设置 AURA_CACHE_TOKEN 环境变量".to_string(),
                        ));
                    } else if attempt < self.config.max_retries {
                        self.wait_before_retry(attempt)?;
                        continue;
                    } else {
                        return Err(LoomError::Cache(format!(
                            "远程缓存失效失败: HTTP {}",
                            status
                        )));
                    }
                }
                Err(e) => {
                    if attempt < self.config.max_retries {
                        self.wait_before_retry(attempt)?;
                        continue;
                    }
                    return Err(LoomError::Cache(format!(
                        "远程缓存失效失败: {}",
                        e
                    )));
                }
            }
        }

        Err(LoomError::Cache("远程缓存失效失败: 重试耗尽".to_string()))
    }

    /// 检查远程缓存服务器是否可达
    pub fn check_health(&self) -> Result<RemoteCacheStatus, LoomError> {
        let url = format!("{}/v1/health", self.config.url.trim_end_matches('/'));

        let request = self.client.get(&url);
        let request = self.add_auth(request);

        match request.send() {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(RemoteCacheStatus::Healthy)
                } else {
                    Ok(RemoteCacheStatus::Unhealthy(response.status().as_u16()))
                }
            }
            Err(e) => Ok(RemoteCacheStatus::Unreachable(e.to_string())),
        }
    }

    /// 获取配置
    pub fn config(&self) -> &RemoteCacheConfig {
        &self.config
    }

    /// 添加认证头
    fn add_auth(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        if let Some(ref token) = self.config.token {
            request.header("Authorization", format!("Bearer {}", token))
        } else {
            request
        }
    }

    /// 指数退避等待
    fn wait_before_retry(&self, attempt: u32) -> Result<(), LoomError> {
        let delay = std::time::Duration::from_millis(100 * (1 << attempt.min(5)) as u64);
        std::thread::sleep(delay);
        Ok(())
    }
}

/// 远程缓存状态
#[derive(Debug, Clone)]
pub enum RemoteCacheStatus {
    Healthy,
    Unhealthy(u16),
    Unreachable(String),
}

impl std::fmt::Display for RemoteCacheStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteCacheStatus::Healthy => write!(f, "✓ 远程缓存正常"),
            RemoteCacheStatus::Unhealthy(code) => write!(f, "✗ 远程缓存异常 (HTTP {})", code),
            RemoteCacheStatus::Unreachable(e) => write!(f, "✗ 远程缓存不可达: {}", e),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// B3.4: CacheService — 整合本地 + 远程缓存
// ═══════════════════════════════════════════════════════════════════════════════

/// 缓存服务（本地 + 远程两级缓存）
///
/// 缓存查找顺序：
///   1. 本地缓存（target/cache/）
///   2. 远程缓存（HTTP Build Cache）
///   3. 未命中 → 执行任务 → 写入两级缓存
pub struct CacheService {
    /// 本地缓存
    local: std::sync::Arc<std::sync::Mutex<crate::cache::local::LocalCache>>,
    /// 远程缓存（可选）
    remote: Option<RemoteCache>,
    /// 构建配置
    build_config: std::sync::Arc<ResolvedBuildConfig>,
    /// 统计信息
    stats: CacheServiceStats,
}

#[derive(Debug, Default, Clone)]
pub struct CacheServiceStats {
    /// 本地缓存命中次数
    pub local_hits: usize,
    /// 远程缓存命中次数
    pub remote_hits: usize,
    /// 缓存未命中次数
    pub misses: usize,
    /// 上传次数
    pub uploads: usize,
    /// 下载次数
    pub downloads: usize,
}

impl CacheService {
    /// 创建缓存服务
    pub fn new(
        local: std::sync::Arc<std::sync::Mutex<crate::cache::local::LocalCache>>,
        remote_config: Option<RemoteCacheConfig>,
        build_config: std::sync::Arc<ResolvedBuildConfig>,
    ) -> Result<Self, LoomError> {
        let remote = match remote_config {
            Some(config) => Some(RemoteCache::new(config)?),
            None => None,
        };

        Ok(Self {
            local,
            remote,
            build_config,
            stats: CacheServiceStats::default(),
        })
    }

    /// 检查任务是否有缓存（本地 → 远程）
    ///
    /// 返回 (是否命中, 缓存来源)
    pub fn lookup(
        &mut self,
        task: &TaskDefinition,
    ) -> Result<(bool, CacheHitSource), LoomError> {
        // 1. 检查本地缓存
        {
            let cache = self.local.lock().map_err(|e| {
                LoomError::Cache(format!("本地缓存锁获取失败: {}", e))
            })?;

            if let Ok(fp) = Fingerprint::compute(task) {
                if cache.is_up_to_date(&task.name, &fp) && cache.has_artifacts(&task.name) {
                    self.stats.local_hits += 1;
                    return Ok((true, CacheHitSource::Local));
                }
            }
        }

        // 2. 检查远程缓存
        if let Some(ref remote) = self.remote {
            let cache_key = RemoteCache::compute_cache_key(task, &self.build_config)?;

            if remote.lookup(&cache_key)? {
                self.stats.remote_hits += 1;
                return Ok((true, CacheHitSource::Remote));
            }
        }

        self.stats.misses += 1;
        Ok((false, CacheHitSource::Miss))
    }

    /// 从缓存恢复产物（本地 → 远程）
    ///
    /// 如果缓存命中，将产物恢复到目标目录。
    pub fn restore(
        &mut self,
        task: &TaskDefinition,
        dest_dir: &std::path::Path,
    ) -> Result<Option<Vec<PathBuf>>, LoomError> {
        // 1. 尝试从本地恢复
        {
            let cache = self.local.lock().map_err(|e| {
                LoomError::Cache(format!("本地缓存锁获取失败: {}", e))
            })?;

            if let Ok(fp) = Fingerprint::compute(task) {
                if cache.is_up_to_date(&task.name, &fp) && cache.has_artifacts(&task.name) {
                    let restored = cache.restore_artifacts(&task.name, dest_dir)?;
                    self.stats.downloads += 1;
                    return Ok(Some(restored));
                }
            }
        }

        // 2. 尝试从远程下载
        if let Some(ref remote) = self.remote {
            let cache_key = RemoteCache::compute_cache_key(task, &self.build_config)?;

            match remote.download_artifacts(&cache_key, dest_dir) {
                Ok(restored) => {
                    self.stats.downloads += 1;

                    // 回写到本地缓存
                    {
                        let mut cache = self.local.lock().map_err(|e| {
                            LoomError::Cache(format!("本地缓存锁获取失败: {}", e))
                        })?;
                        if let Ok(fp) = Fingerprint::compute(task) {
                            let _ = cache.store_fingerprint(&task.name, &fp);
                            let _ = cache.store_artifacts(&task.name, &restored);
                        }
                    }

                    return Ok(Some(restored));
                }
                Err(e) => {
                    tracing::warn!("远程缓存下载失败: {}", e);
                }
            }
        }

        Ok(None)
    }

    /// 存储任务结果到缓存（本地 + 远程）
    pub fn store(
        &mut self,
        task: &TaskDefinition,
        artifacts: &[PathBuf],
    ) -> Result<(), LoomError> {
        // 1. 存储到本地缓存
        {
            let mut cache = self.local.lock().map_err(|e| {
                LoomError::Cache(format!("本地缓存锁获取失败: {}", e))
            })?;

            if let Ok(fp) = Fingerprint::compute(task) {
                let _ = cache.store_fingerprint(&task.name, &fp);
                let _ = cache.store_artifacts(&task.name, artifacts);
            }
        }

        // 2. 上传到远程缓存
        if let Some(ref remote) = self.remote {
            let cache_key = RemoteCache::compute_cache_key(task, &self.build_config)?;
            match remote.upload_artifacts(&cache_key, &task.name, artifacts, &self.build_config) {
                Ok(()) => {
                    self.stats.uploads += 1;
                }
                Err(e) => {
                    tracing::warn!("远程缓存上传失败: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 清除缓存（本地 + 远程）
    pub fn invalidate(&mut self, task: &TaskDefinition) -> Result<(), LoomError> {
        // 1. 清除本地
        {
            let mut cache = self.local.lock().map_err(|e| {
                LoomError::Cache(format!("本地缓存锁获取失败: {}", e))
            })?;
            let _ = cache.remove_task_artifacts(&task.name);
        }

        // 2. 清除远程
        if let Some(ref remote) = self.remote {
            let cache_key = RemoteCache::compute_cache_key(task, &self.build_config)?;
            let _ = remote.invalidate(&cache_key);
        }

        Ok(())
    }

    /// 使所有缓存失效
    pub fn invalidate_all(&mut self, tasks: &[TaskDefinition]) -> Result<(), LoomError> {
        for task in tasks {
            self.invalidate(task)?;
        }
        Ok(())
    }

    /// 获取统计信息
    pub fn stats(&self) -> &CacheServiceStats {
        &self.stats
    }

    /// 获取本地缓存统计
    pub fn local_stats(&self) -> Option<CacheStats> {
        self.local.lock().map(|c| c.stats()).ok()
    }

    /// 检查远程缓存状态
    pub fn remote_health(&self) -> Option<RemoteCacheStatus> {
        self.remote.as_ref().and_then(|r| r.check_health().ok())
    }
}

/// 缓存命中来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHitSource {
    /// 本地缓存命中
    Local,
    /// 远程缓存命中
    Remote,
    /// 缓存未命中
    Miss,
}

impl std::fmt::Display for CacheHitSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheHitSource::Local => write!(f, "本地缓存"),
            CacheHitSource::Remote => write!(f, "远程缓存"),
            CacheHitSource::Miss => write!(f, "未命中"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Base64 编解码（使用 base64 crate 或手动实现）
// ═══════════════════════════════════════════════════════════════════════════════

fn encode_base64(data: &[u8]) -> String {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    engine.encode(data)
}

fn decode_base64(encoded: &str) -> Result<Vec<u8>, LoomError> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    engine.decode(encoded).map_err(|e| {
        LoomError::Cache(format!("Base64 解码失败: {}", e))
    })
}

fn now_timestamp() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::local::LocalCache;
    use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn make_task(name: &str, kind: TaskKind) -> TaskDefinition {
        TaskDefinition {
            name: name.to_string(),
            description: format!("test task {}", name),
            kind,
            depends_on: Vec::new(),
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        }
    }

    fn setup_cache_service() -> (CacheService, TempDir, Arc<Mutex<LocalCache>>) {
        let tmp = TempDir::new().unwrap();
        let cache = Arc::new(Mutex::new(LocalCache::new(tmp.path()).unwrap()));
        let build_config = Arc::new(ResolvedBuildConfig::default());
        let service = CacheService::new(cache.clone(), None, build_config).unwrap();
        (service, tmp, cache)
    }

    #[test]
    fn test_remote_cache_config_default() {
        let config = RemoteCacheConfig::default();
        assert!(config.url.is_empty());
        assert!(config.token.is_none());
        assert!(!config.shared);
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_remote_cache_config_from_build_config() {
        let build_config = ResolvedBuildConfig {
            cache_remote: Some("https://cache.aura-lang.dev".to_string()),
            cache_remote_shared: true,
            ..Default::default()
        };

        let config = RemoteCacheConfig::from_build_config(&build_config).unwrap();
        assert_eq!(config.url, "https://cache.aura-lang.dev");
        assert!(config.shared);
    }

    #[test]
    fn test_remote_cache_config_none() {
        let build_config = ResolvedBuildConfig {
            cache_remote: None,
            ..Default::default()
        };

        assert!(RemoteCacheConfig::from_build_config(&build_config).is_none());
    }

    #[test]
    fn test_remote_cache_new_empty_url() {
        let config = RemoteCacheConfig {
            url: String::new(),
            ..Default::default()
        };
        assert!(RemoteCache::new(config).is_err());
    }

    #[test]
    fn test_remote_cache_status_display() {
        let healthy = RemoteCacheStatus::Healthy;
        assert!(healthy.to_string().contains("正常"));

        let unhealthy = RemoteCacheStatus::Unhealthy(500);
        assert!(unhealthy.to_string().contains("500"));

        let unreachable = RemoteCacheStatus::Unreachable("timeout".to_string());
        assert!(unreachable.to_string().contains("timeout"));
    }

    #[test]
    fn test_cache_hit_source_display() {
        assert_eq!(CacheHitSource::Local.to_string(), "本地缓存");
        assert_eq!(CacheHitSource::Remote.to_string(), "远程缓存");
        assert_eq!(CacheHitSource::Miss.to_string(), "未命中");
    }

    #[test]
    fn test_base64_roundtrip() {
        let data = b"hello world, this is a test of base64 encoding";
        let encoded = encode_base64(data);
        let decoded = decode_base64(&encoded).unwrap();
        assert_eq!(data, &decoded[..]);
    }

    #[test]
    fn test_base64_empty() {
        let data = b"";
        let encoded = encode_base64(data);
        let decoded = decode_base64(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_base64_binary() {
        let data = &[0u8, 1, 2, 255, 128, 64];
        let encoded = encode_base64(data);
        let decoded = decode_base64(&encoded).unwrap();
        assert_eq!(data, &decoded[..]);
    }

    #[test]
    fn test_cache_service_lookup_miss() {
        let (mut service, _tmp, _cache) = setup_cache_service();
        let task = make_task("compile-main", TaskKind::Compile("main".to_string()));

        let (hit, source) = service.lookup(&task).unwrap();
        assert!(!hit);
        assert_eq!(source, CacheHitSource::Miss);
    }

    #[test]
    fn test_cache_service_store_and_lookup() {
        let (mut service, tmp, _cache) = setup_cache_service();
        let task = make_task("compile-main", TaskKind::Compile("main".to_string()));

        // 创建测试文件
        let file = tmp.path().join("test.auc");
        std::fs::write(&file, "compiled bytecode").unwrap();

        // 存储
        service.store(&task, &[file]).unwrap();

        // 查找
        let (hit, source) = service.lookup(&task).unwrap();
        assert!(hit);
        assert_eq!(source, CacheHitSource::Local);
    }

    #[test]
    fn test_cache_service_restore() {
        let (mut service, tmp, _cache) = setup_cache_service();
        let task = make_task("package", TaskKind::Package);
        let dest = tmp.path().join("restored");

        // 创建并存储
        let file = tmp.path().join("app.auz");
        std::fs::write(&file, "package data").unwrap();
        service.store(&task, &[file]).unwrap();

        // 恢复
        let restored = service.restore(&task, &dest).unwrap();
        assert!(restored.is_some());
        let files = restored.unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].exists());

        let content = std::fs::read_to_string(&files[0]).unwrap();
        assert_eq!(content, "package data");
    }

    #[test]
    fn test_cache_service_restore_miss() {
        let (mut service, tmp, _cache) = setup_cache_service();
        let task = make_task("compile-main", TaskKind::Compile("main".to_string()));
        let dest = tmp.path().join("empty");

        let restored = service.restore(&task, &dest).unwrap();
        assert!(restored.is_none());
    }

    #[test]
    fn test_cache_service_invalidate() {
        let (mut service, tmp, _cache) = setup_cache_service();
        let task = make_task("compile-main", TaskKind::Compile("main".to_string()));

        let file = tmp.path().join("test.auc");
        std::fs::write(&file, "data").unwrap();
        service.store(&task, &[file]).unwrap();

        // 验证缓存存在
        let (hit, _) = service.lookup(&task).unwrap();
        assert!(hit);

        // 失效
        service.invalidate(&task).unwrap();

        // 验证缓存已清除
        let (hit, _) = service.lookup(&task).unwrap();
        assert!(!hit);
    }

    #[test]
    fn test_cache_service_invalidate_all() {
        let (mut service, tmp, _cache) = setup_cache_service();
        let task1 = make_task("compile-main", TaskKind::Compile("main".to_string()));
        let task2 = make_task("package", TaskKind::Package);

        let file1 = tmp.path().join("test1.auc");
        std::fs::write(&file1, "data1").unwrap();
        service.store(&task1, &[file1]).unwrap();

        let file2 = tmp.path().join("test2.auz");
        std::fs::write(&file2, "data2").unwrap();
        service.store(&task2, &[file2]).unwrap();

        service.invalidate_all(&[task1.clone(), task2.clone()]).unwrap();

        let (hit1, _) = service.lookup(&task1).unwrap();
        let (hit2, _) = service.lookup(&task2).unwrap();
        assert!(!hit1);
        assert!(!hit2);
    }

    #[test]
    fn test_cache_service_stats() {
        let (mut service, tmp, _cache) = setup_cache_service();
        let task = make_task("compile-main", TaskKind::Compile("main".to_string()));

        // 初始统计
        let stats = service.stats();
        assert_eq!(stats.local_hits, 0);
        assert_eq!(stats.misses, 0);

        // 未命中
        service.lookup(&task).unwrap();
        assert_eq!(service.stats().misses, 1);

        // 存储
        let file = tmp.path().join("test.auc");
        std::fs::write(&file, "data").unwrap();
        service.store(&task, &[file]).unwrap();
        assert_eq!(service.stats().local_hits, 0);

        // 命中
        service.lookup(&task).unwrap();
        assert_eq!(service.stats().local_hits, 1);
        assert_eq!(service.stats().misses, 1);
    }

    #[test]
    fn test_cache_service_local_stats() {
        let (mut service, tmp, _cache) = setup_cache_service();
        let task = make_task("compile-main", TaskKind::Compile("main".to_string()));

        let file = tmp.path().join("test.auc");
        std::fs::write(&file, "data").unwrap();
        service.store(&task, &[file]).unwrap();

        let stats = service.local_stats().unwrap();
        assert!(stats.fingerprint_count >= 1);
    }

    #[test]
    fn test_cache_service_remote_health_no_remote() {
        let (service, _tmp, _cache) = setup_cache_service();
        assert!(service.remote_health().is_none());
    }

    #[test]
    fn test_compute_cache_key_deterministic() {
        let task = make_task("compile-main", TaskKind::Compile("main".to_string()));
        let config = ResolvedBuildConfig::default();

        // 创建一个有 URL 的配置（不实际连接）
        let cache_config = RemoteCacheConfig {
            url: "http://localhost:99999".to_string(),
            ..Default::default()
        };
        let cache = RemoteCache::new(cache_config).unwrap();

        let key1 = RemoteCache::compute_cache_key(&task, &config).unwrap();
        let key2 = RemoteCache::compute_cache_key(&task, &config).unwrap();
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn test_compute_cache_key_changes_with_task() {
        let task1 = make_task("compile-main", TaskKind::Compile("main".to_string()));
        let task2 = make_task("compile-test", TaskKind::Compile("test".to_string()));
        let config = ResolvedBuildConfig::default();

        let cache_config = RemoteCacheConfig {
            url: "http://localhost:99999".to_string(),
            ..Default::default()
        };
        let cache = RemoteCache::new(cache_config).unwrap();

        let key1 = RemoteCache::compute_cache_key(&task1, &config).unwrap();
        let key2 = RemoteCache::compute_cache_key(&task2, &config).unwrap();
        assert_ne!(key1, key2);
    }
}
