//! [Phase B6] CI/CD 集成
//!
// 支持 .aura-ci.yml 配置 + CI 执行。
//! 对应设计文档 §16 CI/CD 集成。

pub mod config;

pub use config::{
    CiConfig, CiEnv, CiExecutor, CiResult, CiStep, CiTrigger, PublishConfig, TestConfig,
};
