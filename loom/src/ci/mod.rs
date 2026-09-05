//! [Phase L6] CI/CD 集成

pub mod config;

/// CI 配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CiConfig {
    /// 配置版本
    pub version: String,
    /// 触发条件
    pub triggers: Option<serde_json::Value>,
    /// 环境变量
    pub env: std::collections::HashMap<String, String>,
    /// 流水线步骤
    pub steps: Vec<CiStep>,
}

/// CI 步骤
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CiStep {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub cache: bool,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default)]
    pub condition: Option<String>,
}

