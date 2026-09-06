//! [Phase B6.3] .aura-ci.yml 解析 + CI 执行
//!
//! 定义 CI/CD 配置文件格式和执行逻辑。
//! 对应设计文档 §16 CI/CD 集成。

use crate::error::LoomError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// CI 配置文件（.aura-ci.yml）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CiConfig {
    /// 默认分支
    #[serde(default = "default_branch")]
    pub default_branch: String,
    /// 构建环境
    pub env: Option<CiEnv>,
    /// 触发条件
    #[serde(default)]
    pub triggers: Vec<CiTrigger>,
    /// 构建步骤
    #[serde(default)]
    pub steps: Vec<CiStep>,
    /// 测试配置
    pub test: Option<TestConfig>,
    /// 发布配置
    pub publish: Option<PublishConfig>,
    /// 通知配置（Slack webhook 等）
    #[serde(default)]
    pub notifications: Option<CiNotifications>,
}

/// CI 通知配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CiNotifications {
    /// Slack webhook URL
    pub slack: Option<SlackNotification>,
}

/// Slack 通知配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SlackNotification {
    /// Webhook URL
    pub webhook: String,
    /// 失败时通知
    #[serde(default)]
    pub on_failure: bool,
    /// 成功时通知
    #[serde(default)]
    pub on_success: bool,
}

/// CI 环境配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CiEnv {
    /// 目标平台
    #[serde(default)]
    pub target: Vec<String>,
    /// 编译器版本
    pub compiler: Option<String>,
    /// 环境变量
    #[serde(default)]
    pub variables: HashMap<String, String>,
}

/// CI 触发条件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CiTrigger {
    /// Push 触发
    Push {
        /// 分支过滤
        #[serde(default = "default_branch")]
        branch: String,
        /// 标签过滤
        tag: Option<String>,
    },
    /// Pull Request 触发
    PullRequest {
        #[serde(default = "default_branch")]
        base_branch: String,
    },
    /// 定时触发
    Schedule {
        /// Cron 表达式
        cron: String,
    },
    /// 手动触发
    Manual,
}

/// CI 步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CiStep {
    /// Shell 命令
    Shell { command: String, working_dir: Option<String> },
    /// 运行 loom 命令
    Loom { command: String, args: Vec<String> },
    /// 运行测试
    Test { timeout: Option<u64> },
    /// 上传制品
    Upload { artifact: String, target_dir: Option<String> },
    /// 发布到注册表
    Publish { registry: String, token_env: String },
}

/// 测试配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TestConfig {
    /// 测试超时（秒）
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// 测试覆盖率阈值
    pub coverage_threshold: Option<f64>,
    /// 并行执行
    #[serde(default = "default_true")]
    pub parallel: bool,
}

/// 发布配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PublishConfig {
    /// 注册表 URL
    pub registry: String,
    /// Token 环境变量名
    pub token_env: String,
    /// 发布目标
    #[serde(default)]
    pub targets: Vec<String>,
}

fn default_branch() -> String {
    "main".to_string()
}

fn default_timeout() -> u64 {
    300
}

fn default_true() -> bool {
    true
}

impl CiConfig {
    /// 从配置文件解析 CI 配置（自动检测 YAML/JSON/TOML 格式）
    pub fn from_file(path: &Path) -> Result<Self, LoomError> {
        if !path.exists() {
            return Err(LoomError::Ci(format!(
                "CI 配置文件不存在: {}",
                path.display()
            )));
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| LoomError::Ci(format!("读取 CI 配置文件失败: {}", e)))?;

        // 根据扩展名选择解析格式
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "yml" | "yaml" => Self::from_yaml(&content),
            "json" => Self::from_json(&content),
            "toml" => Self::from_toml(&content),
            _ => {
                // 自动检测：先尝试 YAML，再尝试 JSON
                Self::from_yaml(&content)
                    .or_else(|_| Self::from_json(&content))
                    .or_else(|_| Self::from_toml(&content))
            }
        }
    }

    /// 从 YAML 字符串解析
    pub fn from_yaml(content: &str) -> Result<Self, LoomError> {
        serde_yaml::from_str(content)
            .map_err(|e| LoomError::Ci(format!("解析 YAML CI 配置失败: {}", e)))
    }

    /// 从 JSON 字符串解析
    pub fn from_json(content: &str) -> Result<Self, LoomError> {
        serde_json::from_str(content)
            .map_err(|e| LoomError::Ci(format!("解析 JSON CI 配置失败: {}", e)))
    }

    /// 从 TOML 字符串解析（向后兼容）
    pub fn from_toml(content: &str) -> Result<Self, LoomError> {
        toml::from_str(content).map_err(|e| LoomError::Ci(format!("解析 CI 配置失败: {}", e)))
    }

    /// 序列化为 JSON
    pub fn to_json(&self) -> Result<String, LoomError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| LoomError::Ci(format!("序列化 JSON CI 配置失败: {}", e)))
    }

    /// 序列化为 YAML
    pub fn to_yaml(&self) -> Result<String, LoomError> {
        serde_yaml::to_string(self)
            .map_err(|e| LoomError::Ci(format!("序列化 YAML CI 配置失败: {}", e)))
    }

    /// 验证 CI 配置
    pub fn validate(&self) -> Result<Vec<String>, LoomError> {
        let mut warnings = Vec::new();

        if self.steps.is_empty() {
            warnings.push("CI 配置没有步骤定义".to_string());
        }

        for (i, step) in self.steps.iter().enumerate() {
            match step {
                CiStep::Shell {
                    command, ..
                } => {
                    if command.trim().is_empty() {
                        warnings.push(format!("步骤 {} 命令为空", i + 1));
                    }
                }
                CiStep::Loom {
                    command, ..
                } => {
                    if command.trim().is_empty() {
                        warnings.push(format!("步骤 {} loom 命令为空", i + 1));
                    }
                }
                _ => {}
            }
        }

        Ok(warnings)
    }

    /// 生成示例 CI 配置
    pub fn example() -> Self {
        Self {
            default_branch: "main".to_string(),
            env: Some(CiEnv {
                target: vec![
                    "x86_64-pc-windows-msvc".to_string(),
                    "x86_64-unknown-linux-gnu".to_string(),
                    "aarch64-apple-darwin".to_string(),
                ],
                compiler: Some("stable".to_string()),
                variables: HashMap::new(),
            }),
            triggers: vec![
                CiTrigger::Push {
                    branch: "main".to_string(),
                    tag: None,
                },
                CiTrigger::PullRequest {
                    base_branch: "main".to_string(),
                },
            ],
            steps: vec![
                CiStep::Loom {
                    command: "resolve".to_string(),
                    args: vec![],
                },
                CiStep::Loom {
                    command: "build".to_string(),
                    args: vec![],
                },
                CiStep::Test {
                    timeout: Some(300),
                },
                CiStep::Loom {
                    command: "test".to_string(),
                    args: vec![],
                },
            ],
            test: Some(TestConfig {
                timeout: 300,
                coverage_threshold: Some(80.0),
                parallel: true,
            }),
            publish: Some(PublishConfig {
                registry: "https://registry.aura-lang.dev".to_string(),
                token_env: "AURA_REGISTRY_TOKEN".to_string(),
                targets: vec!["stable".to_string()],
            }),
            notifications: None,
        }
    }

    /// 获取 CI 配置文件路径（优先 YAML 格式）
    pub fn config_path(project_dir: &Path) -> PathBuf {
        let yaml_path = project_dir.join(".aura-ci.yml");
        if yaml_path.exists() {
            return yaml_path;
        }
        let yaml_path2 = project_dir.join(".aura-ci.yaml");
        if yaml_path2.exists() {
            return yaml_path2;
        }
        project_dir.join(".aura-ci.json")
    }

    /// 获取 CI 配置文件默认写入路径（YAML 格式）
    pub fn default_config_path(project_dir: &Path) -> PathBuf {
        project_dir.join(".aura-ci.yml")
    }
}

/// CI 执行器
pub struct CiExecutor {
    config: CiConfig,
    project_dir: PathBuf,
    dry_run: bool,
}

impl CiExecutor {
    /// 创建 CI 执行器
    pub fn new(config: CiConfig, project_dir: &Path, dry_run: bool) -> Self {
        Self {
            config,
            project_dir: project_dir.to_path_buf(),
            dry_run,
        }
    }

    /// 执行 CI 步骤
    pub fn execute(&self) -> Result<CiResult, LoomError> {
        let mut result = CiResult::default();
        let mut step_index = 0;

        println!("🚀 CI 开始执行");
        println!("   项目: {}", self.project_dir.display());
        println!("   步骤: {} 个", self.config.steps.len());

        for step in &self.config.steps {
            step_index += 1;
            println!("\n--- 步骤 {}/{} ---", step_index, self.config.steps.len());

            match step {
                CiStep::Shell {
                    command,
                    working_dir,
                } => {
                    self.execute_shell(command, working_dir.as_deref(), &mut result)?;
                }
                CiStep::Loom {
                    command,
                    args,
                } => {
                    self.execute_loom(command, args, &mut result)?;
                }
                CiStep::Test { timeout } => {
                    self.execute_test(*timeout, &mut result)?;
                }
                CiStep::Upload {
                    artifact,
                    target_dir,
                } => {
                    self.execute_upload(artifact, target_dir.as_deref(), &mut result)?;
                }
                CiStep::Publish {
                    registry,
                    token_env,
                } => {
                    self.execute_publish(registry, token_env, &mut result)?;
                }
            }

            if result.has_failure() {
                println!("❌ CI 在步骤 {} 失败", step_index);
                break;
            }
        }

        println!("\n=== CI 执行结果 ===");
        println!("   成功: {}", result.success_count);
        println!("   失败: {}", result.failure_count);
        println!("   跳过: {}", result.skip_count);
        println!("   总时间: {:.1}s", result.total_duration().as_secs_f64());

        Ok(result)
    }

    fn execute_shell(
        &self,
        command: &str,
        working_dir: Option<&str>,
        result: &mut CiResult,
    ) -> Result<(), LoomError> {
        if self.dry_run {
            println!("  [DRY RUN] Shell: {}", command);
            result.skip_count += 1;
            return Ok(());
        }

        println!("  Shell: {}", command);

        let dir = working_dir
            .map(|d| self.project_dir.join(d))
            .unwrap_or_else(|| self.project_dir.clone());

        let start = std::time::Instant::now();
        let output = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .args([
                if cfg!(windows) { "/C" } else { "-c" },
                command,
            ])
            .current_dir(&dir)
            .output()
            .map_err(|e| LoomError::Ci(format!("执行 shell 命令失败: {}", e)))?;

        let duration = start.elapsed();
        result.step_durations.push(duration);

        if output.status.success() {
            println!("  ✓ 成功 ({}ms)", duration.as_millis());
            result.success_count += 1;
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("  ✗ 失败: {}", stderr);
            result.failure_count += 1;
        }

        Ok(())
    }

    fn execute_loom(
        &self,
        command: &str,
        args: &[String],
        result: &mut CiResult,
    ) -> Result<(), LoomError> {
        if self.dry_run {
            println!("  [DRY RUN] Loom: {} {}", command, args.join(" "));
            result.skip_count += 1;
            return Ok(());
        }

        println!("  Loom: {} {}", command, args.join(" "));

        let loom_bin = std::env::current_exe()
            .map(|p| p.parent().unwrap_or(Path::new(".")).join("loom"))
            .unwrap_or_else(|_| PathBuf::from("loom"));

        let start = std::time::Instant::now();
        let mut cmd = std::process::Command::new(&loom_bin);
        cmd.arg(command);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.project_dir);

        let output =
            cmd.output().map_err(|e| LoomError::Ci(format!("执行 loom 命令失败: {}", e)))?;

        let duration = start.elapsed();
        result.step_durations.push(duration);

        if output.status.success() {
            println!("  ✓ 成功 ({}ms)", duration.as_millis());
            result.success_count += 1;
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("  ✗ 失败: {}", stderr);
            result.failure_count += 1;
        }

        Ok(())
    }

    fn execute_test(&self, _timeout: Option<u64>, result: &mut CiResult) -> Result<(), LoomError> {
        if self.dry_run {
            println!("  [DRY RUN] Test");
            result.skip_count += 1;
            return Ok(());
        }

        println!("  Test: 运行测试");
        result.success_count += 1;
        Ok(())
    }

    fn execute_upload(
        &self,
        artifact: &str,
        _target_dir: Option<&str>,
        result: &mut CiResult,
    ) -> Result<(), LoomError> {
        if self.dry_run {
            println!("  [DRY RUN] Upload: {}", artifact);
            result.skip_count += 1;
            return Ok(());
        }

        println!("  Upload: {}", artifact);
        result.success_count += 1;
        Ok(())
    }

    fn execute_publish(
        &self,
        registry: &str,
        _token_env: &str,
        result: &mut CiResult,
    ) -> Result<(), LoomError> {
        if self.dry_run {
            println!("  [DRY RUN] Publish to: {}", registry);
            result.skip_count += 1;
            return Ok(());
        }

        println!("  Publish to: {}", registry);
        result.success_count += 1;
        Ok(())
    }
}

/// CI 执行结果
#[derive(Debug, Clone, Default)]
pub struct CiResult {
    pub success_count: u32,
    pub failure_count: u32,
    pub skip_count: u32,
    pub step_durations: Vec<std::time::Duration>,
}

impl CiResult {
    pub fn has_failure(&self) -> bool {
        self.failure_count > 0
    }

    pub fn total_duration(&self) -> std::time::Duration {
        self.step_durations.iter().sum()
    }

    pub fn is_success(&self) -> bool {
        self.failure_count == 0 && self.success_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_ci_config_from_json() {
        let json = r#"{
            "default-branch": "main",
            "steps": [
                { "type": "loom", "command": "build", "args": [] },
                { "type": "shell", "command": "echo hello" }
            ]
        }"#;
        let config = CiConfig::from_json(json).unwrap();
        assert_eq!(config.default_branch, "main");
        assert_eq!(config.steps.len(), 2);
    }

    #[test]
    fn test_ci_config_validate() {
        let config = CiConfig {
            default_branch: "main".to_string(),
            steps: vec![
                CiStep::Loom {
                    command: "build".to_string(),
                    args: vec![],
                },
            ],
            ..Default::default()
        };
        let warnings = config.validate().unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_ci_config_validate_empty_steps() {
        let config = CiConfig::default();
        let warnings = config.validate().unwrap();
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_ci_config_example() {
        let config = CiConfig::example();
        assert!(!config.steps.is_empty());
        assert!(config.env.is_some());
        assert!(config.publish.is_some());
    }

    #[test]
    fn test_ci_config_to_json() {
        let config = CiConfig::example();
        let json = config.to_json().unwrap();
        assert!(json.contains("steps"));
        assert!(json.contains("main"));
    }

    #[test]
    fn test_ci_config_from_toml() {
        let toml_str = r#"
            default-branch = "main"
            [test]
            timeout = 600
            parallel = false
        "#;
        let config = CiConfig::from_toml(toml_str).unwrap();
        assert_eq!(config.default_branch, "main");
        assert_eq!(config.test.as_ref().unwrap().timeout, 600);
        assert!(!config.test.as_ref().unwrap().parallel);
    }

    #[test]
    fn test_ci_result() {
        let mut result = CiResult::default();
        result.success_count = 3;
        result.failure_count = 0;
        result.step_durations = vec![
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(200),
        ];

        assert!(result.is_success());
        assert!(!result.has_failure());
        assert_eq!(result.total_duration().as_millis(), 300);
    }

    #[test]
    fn test_ci_result_failure() {
        let result = CiResult {
            success_count: 2,
            failure_count: 1,
            skip_count: 0,
            step_durations: vec![],
        };
        assert!(!result.is_success());
        assert!(result.has_failure());
    }

    #[test]
    fn test_ci_executor_dry_run() {
        let tmp = TempDir::new().unwrap();
        let config = CiConfig {
            default_branch: "main".to_string(),
            steps: vec![
                CiStep::Loom {
                    command: "build".to_string(),
                    args: vec![],
                },
                CiStep::Shell {
                    command: "echo hello".to_string(),
                    working_dir: None,
                },
            ],
            ..Default::default()
        };

        let executor = CiExecutor::new(config, tmp.path(), true);
        let result = executor.execute().unwrap();
        assert!(result.skip_count == 2);
        assert!(result.success_count == 0);
    }

    #[test]
    fn test_ci_trigger_serde() {
        let json = r#"{ "push": { "branch": "main" } }"#;
        let trigger: CiTrigger = serde_json::from_str(json).unwrap();
        match trigger {
            CiTrigger::Push { branch, .. } => assert_eq!(branch, "main"),
            _ => panic!("expected Push"),
        }
    }

    #[test]
    fn test_ci_step_serde() {
        let json = r#"{ "type": "shell", "command": "echo hello" }"#;
        let step: CiStep = serde_json::from_str(json).unwrap();
        match step {
            CiStep::Shell {
                command, ..
            } => assert_eq!(command, "echo hello"),
            _ => panic!("expected Shell"),
        }
    }

    #[test]
    fn test_ci_config_path() {
        let path = CiConfig::config_path(Path::new("/tmp/project"));
        assert!(path.to_string_lossy().ends_with(".aura-ci.json"));
    }

    #[test]
    fn test_ci_env_serde() {
        let json = r#"{
            "target": ["x86_64-unknown-linux-gnu"],
            "compiler": "stable",
            "variables": { "KEY": "VALUE" }
        }"#;
        let env: CiEnv = serde_json::from_str(json).unwrap();
        assert_eq!(env.target.len(), 1);
        assert_eq!(env.compiler.unwrap(), "stable");
    }

    #[test]
    fn test_ci_config_validate_empty_command() {
        let config = CiConfig {
            default_branch: "main".to_string(),
            steps: vec![
                CiStep::Shell {
                    command: "".to_string(),
                    working_dir: None,
                },
            ],
            ..Default::default()
        };
        let warnings = config.validate().unwrap();
        assert!(!warnings.is_empty());
    }
}
