//! [Phase B6.1] 注册表 REST API 客户端
//!
//! 实现设计文档 §13.3 注册表协议：
//! - GET  /v1/packages/{name}                     → 包信息 + 版本列表
//! - GET  /v1/packages/{name}/versions/{version}  → 版本详情 + 下载 URL
//! - POST /v1/packages/{name}                      → 发布新包（需认证）
//! - GET  /v1/packages/{name}/search?q=keyword    → 搜索包

use crate::error::LoomError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════════
// 响应类型
// ═══════════════════════════════════════════════════════════════════════════════

/// 包信息（GET /v1/packages/{name} 响应）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PackageInfo {
    /// 包名
    pub name: String,
    /// 最新版本
    pub latest: Option<String>,
    /// 版本列表
    #[serde(default)]
    pub versions: Vec<VersionInfo>,
    /// 包元数据
    pub metadata: Option<PackageMetadata>,
}

/// 包元数据
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PackageMetadata {
    /// 描述
    pub description: Option<String>,
    /// 许可证
    pub license: Option<String>,
    /// 作者
    pub authors: Option<Vec<String>>,
    /// 仓库 URL
    pub repository: Option<String>,
}

/// 版本信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct VersionInfo {
    /// 版本号
    pub version: String,
    /// 校验和（sha256:...）
    pub checksum: Option<String>,
    /// 下载 URL
    pub download: Option<String>,
    /// 发布时间
    pub released: Option<String>,
}

/// 搜索结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SearchResult {
    /// 包名
    pub name: String,
    /// 描述
    pub description: Option<String>,
    /// 最新版本
    pub latest: Option<String>,
}

/// 发布请求（POST /v1/packages/{name} 请求体）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PublishRequest {
    /// 版本号
    pub version: String,
    /// 包制品（base64 编码）
    pub artifact: String,
    /// 校验和
    pub checksum: String,
    /// 包元数据
    pub metadata: PackageMetadata,
}

/// 发布响应
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PublishResult {
    /// 是否成功
    pub success: bool,
    /// 消息
    pub message: String,
    /// 包名
    pub name: String,
    /// 版本号
    pub version: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 注册表客户端
// ═══════════════════════════════════════════════════════════════════════════════

/// 注册表配置
#[derive(Debug, Clone, Default)]
pub struct RegistryConfig {
    /// 注册表基础 URL（如 "https://registry.aura-lang.dev"）
    pub base_url: String,
    /// 认证 token（可选）
    pub token: Option<String>,
}

impl RegistryConfig {
    /// 创建默认配置
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: None,
        }
    }

    /// 设置认证 token
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// 解析 URL 中的环境变量占位符
    pub fn resolve_env_vars(&self) -> Self {
        let base_url = self.base_url.replace(
            "${AURA_REGISTRY_URL}",
            std::env::var("AURA_REGISTRY_URL").unwrap_or_default().as_str(),
        );
        let token = self
            .token
            .as_ref()
            .map(|t| {
                t.replace(
                    "${AURA_REGISTRY_TOKEN}",
                    std::env::var("AURA_REGISTRY_TOKEN").unwrap_or_default().as_str(),
                )
            })
            .filter(|s| !s.is_empty());
        Self {
            base_url,
            token,
        }
    }
}

/// 注册表 REST API 客户端
///
/// 对应设计文档 §13.3 注册表协议。
pub struct RegistryClient {
    config: RegistryConfig,
    http: reqwest::blocking::Client,
}

impl RegistryClient {
    /// 创建注册表客户端
    pub fn new(config: RegistryConfig) -> Result<Self, LoomError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| LoomError::Registry(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            config: config.resolve_env_vars(),
            http,
        })
    }

    /// 获取注册表配置
    pub fn config(&self) -> &RegistryConfig {
        &self.config
    }

    /// 获取包信息 + 版本列表
    ///
    /// GET /v1/packages/{name}
    pub fn get_package(&self, name: &str) -> Result<PackageInfo, LoomError> {
        let url = format!(
            "{}/v1/packages/{}",
            self.config.base_url.trim_end_matches('/'),
            name
        );
        let mut req = self.http.get(&url);

        if let Some(ref token) = self.config.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req
            .send()
            .map_err(|e| LoomError::Registry(format!("Request failed {}: {}", url, e)))?;

        if !resp.status().is_success() {
            return Err(LoomError::Registry(format!(
                "Failed to get package info: HTTP {} - {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )));
        }

        let text = resp
            .text()
            .map_err(|e| LoomError::Registry(format!("Failed to read response body: {}", e)))?;
        serde_json::from_str(&text)
            .map_err(|e| LoomError::Registry(format!("Failed to parse package info: {}", e)))
    }

    /// 获取版本详情 + 下载 URL
    ///
    /// GET /v1/packages/{name}/versions/{version}
    pub fn get_version(&self, name: &str, version: &str) -> Result<VersionInfo, LoomError> {
        let url = format!(
            "{}/v1/packages/{}/versions/{}",
            self.config.base_url.trim_end_matches('/'),
            name,
            version
        );
        let mut req = self.http.get(&url);

        if let Some(ref token) = self.config.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req
            .send()
            .map_err(|e| LoomError::Registry(format!("Request failed {}: {}", url, e)))?;

        if !resp.status().is_success() {
            return Err(LoomError::Registry(format!(
                "Failed to get version info: HTTP {} - {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )));
        }

        let text = resp
            .text()
            .map_err(|e| LoomError::Registry(format!("Failed to read response body: {}", e)))?;
        serde_json::from_str(&text)
            .map_err(|e| LoomError::Registry(format!("Failed to parse version info: {}", e)))
    }

    /// 搜索包
    ///
    /// GET /v1/packages/search?q={keyword}
    pub fn search(&self, keyword: &str) -> Result<Vec<SearchResult>, LoomError> {
        let url = format!(
            "{}/v1/packages/search?q={}",
            self.config.base_url.trim_end_matches('/'),
            urlencoding::encode(keyword)
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .map_err(|e| LoomError::Registry(format!("Search request failed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(LoomError::Registry(format!(
                "Search failed: HTTP {} - {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )));
        }

        let text = resp
            .text()
            .map_err(|e| LoomError::Registry(format!("Failed to read response body: {}", e)))?;
        serde_json::from_str(&text)
            .map_err(|e| LoomError::Registry(format!("Failed to parse search results: {}", e)))
    }

    /// 发布包
    ///
    /// POST /v1/packages/{name}
    pub fn publish(
        &self,
        name: &str,
        request: &PublishRequest,
    ) -> Result<PublishResult, LoomError> {
        let url = format!(
            "{}/v1/packages/{}",
            self.config.base_url.trim_end_matches('/'),
            name
        );

        let token = self.config.token.as_ref().ok_or_else(|| {
            LoomError::Registry("Publish requires authentication token".to_string())
        })?;

        let body = serde_json::to_string(request).map_err(|e| {
            LoomError::Registry(format!("Failed to serialize publish request: {}", e))
        })?;

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .map_err(|e| LoomError::Registry(format!("Publish request failed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(LoomError::Registry(format!(
                "Publish failed: HTTP {} - {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )));
        }

        let text = resp
            .text()
            .map_err(|e| LoomError::Registry(format!("Failed to read response body: {}", e)))?;
        serde_json::from_str(&text)
            .map_err(|e| LoomError::Registry(format!("Failed to parse publish response: {}", e)))
    }

    /// 下载包制品
    ///
    /// GET {download_url}
    pub fn download(&self, url: &str, dest: &std::path::Path) -> Result<u64, LoomError> {
        let resp =
            self.http.get(url).send().map_err(|e| {
                LoomError::Registry(format!("Download request failed {}: {}", url, e))
            })?;

        if !resp.status().is_success() {
            return Err(LoomError::Registry(format!(
                "Download failed: HTTP {}",
                resp.status()
            )));
        }

        let bytes = resp
            .bytes()
            .map_err(|e| LoomError::Registry(format!("failed to read response body: {}", e)))?;
        let size = bytes.len() as u64;

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| LoomError::Registry(format!("Failed to create directory: {}", e)))?;
        }

        std::fs::write(dest, &bytes)
            .map_err(|e| LoomError::Registry(format!("Failed to write file: {}", e)))?;

        Ok(size)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// URL 编码工具（简化版，避免额外依赖）
// ═══════════════════════════════════════════════════════════════════════════════

mod urlencoding {
    /// 简单的 URL 编码
    pub fn encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 2);
        for byte in input.as_bytes() {
            match *byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(std::char::from_u32(*byte as u32).unwrap());
                }
                _ => {
                    result.push('%');
                    result.push(hex_char((byte >> 4) & 0x0F));
                    result.push(hex_char(byte & 0x0F));
                }
            }
        }
        result
    }

    fn hex_char(n: u8) -> char {
        match n {
            0..=9 => std::char::from_u32((b'0' + n) as u32).unwrap(),
            10..=15 => std::char::from_u32((b'A' + n - 10) as u32).unwrap(),
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_config_new() {
        let config = RegistryConfig::new("https://registry.example.com");
        assert_eq!(config.base_url, "https://registry.example.com");
        assert!(config.token.is_none());
    }

    #[test]
    fn test_registry_config_with_token() {
        let config = RegistryConfig::new("https://registry.example.com").with_token("secret");
        assert_eq!(config.token, Some("secret".to_string()));
    }

    #[test]
    fn test_registry_config_resolve_env_vars() {
        unsafe {
            std::env::set_var("AURA_REGISTRY_URL", "https://env.example.com");
            std::env::set_var("AURA_REGISTRY_TOKEN", "env-token");
        }

        let config = RegistryConfig {
            base_url: "${AURA_REGISTRY_URL}".to_string(),
            token: Some("${AURA_REGISTRY_TOKEN}".to_string()),
        }
        .resolve_env_vars();

        assert_eq!(config.base_url, "https://env.example.com");
        assert_eq!(config.token, Some("env-token".to_string()));

        unsafe {
            std::env::remove_var("AURA_REGISTRY_URL");
            std::env::remove_var("AURA_REGISTRY_TOKEN");
        }
    }

    #[test]
    fn test_registry_client_new() {
        let config = RegistryConfig::new("https://registry.example.com");
        let client = RegistryClient::new(config).unwrap();
        assert_eq!(client.config().base_url, "https://registry.example.com");
    }

    #[test]
    fn test_package_info_deserialize() {
        let json = r#"{
            "name": "aura-json",
            "latest": "1.2.3",
            "versions": [
                {
                    "version": "1.2.3",
                    "checksum": "sha256:abc",
                    "download": "https://registry.example.com/v1/packages/aura-json/1.2.3/aura-json.auz",
                    "released": "2024-01-15T10:00:00Z"
                }
            ],
            "metadata": {
                "description": "JSON parsing and serialization",
                "license": "MIT"
            }
        }"#;

        let info: PackageInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "aura-json");
        assert_eq!(info.latest, Some("1.2.3".to_string()));
        assert_eq!(info.versions.len(), 1);
        assert_eq!(info.versions[0].version, "1.2.3");
        assert_eq!(
            info.metadata.as_ref().unwrap().license,
            Some("MIT".to_string())
        );
    }

    #[test]
    fn test_version_info_deserialize() {
        let json = r#"{
            "version": "1.2.3",
            "checksum": "sha256:abc",
            "download": "https://registry.example.com/v1/packages/aura-json/1.2.3/aura-json.auz",
            "released": "2024-01-15T10:00:00Z"
        }"#;

        let info: VersionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.version, "1.2.3");
        assert_eq!(info.checksum, Some("sha256:abc".to_string()));
        assert!(info.download.is_some());
    }

    #[test]
    fn test_search_result_deserialize() {
        let json = r#"{
            "name": "aura-json",
            "description": "JSON 解析与序列化",
            "latest": "1.2.3"
        }"#;

        let result: SearchResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.name, "aura-json");
        assert_eq!(result.latest, Some("1.2.3".to_string()));
    }

    #[test]
    fn test_publish_request_serialize() {
        let request = PublishRequest {
            version: "1.0.0".to_string(),
            artifact: "base64encodeddata".to_string(),
            checksum: "sha256:abc".to_string(),
            metadata: PackageMetadata {
                description: Some("Test package".to_string()),
                license: Some("MIT".to_string()),
                authors: None,
                repository: None,
            },
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: PublishRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, "1.0.0");
        assert_eq!(parsed.metadata.license, Some("MIT".to_string()));
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding::encode("hello"), "hello");
        assert_eq!(urlencoding::encode("hello world"), "hello%20world");
        assert_eq!(urlencoding::encode("hello+world"), "hello%2Bworld");
        assert_eq!(urlencoding::encode("hello#world"), "hello%23world");
    }
}
