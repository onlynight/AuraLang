//! [Phase B4.3] 显式插件加载
//!
//! 显式插件需在 aura.toml `[plugins]` 表中手动启用。
//!
//! 内置显式插件：
//! - `aura-doc-gen`：生成 API 文档（Markdown + HTML）
//! - `aura-format`：源码格式化
//! - `aura-aot`：AOT 编译（LLVM 后端）
//! - `aura-ci`：CI/CD 流水线脚本
//!
//! 对应设计文档 §9.2 内置插件清单。

use crate::error::LoomError;
use crate::plugin::context::PluginContext;
use crate::plugin::r#trait::BuildPlugin;
use crate::plugin::{PluginKind, TaskResult};
use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs};

// ═══════════════════════════════════════════════════════════════════════════════
// aura-doc-gen：文档生成插件
// ═══════════════════════════════════════════════════════════════════════════════

/// 文档生成插件
///
/// 扫描源码文件，生成 API 文档。
/// 显式插件，需在 `[plugins]` 中设置 `aura-doc-gen = true`。
pub struct DocGenPlugin;

impl BuildPlugin for DocGenPlugin {
    fn name(&self) -> &str {
        "aura-doc-gen"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> PluginKind {
        PluginKind::Explicit
    }
    fn description(&self) -> Option<&str> {
        Some("Documentation generation plugin: scan sources, generate API docs (Markdown)")
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-doc-gen");
        tracing::info!("aura-doc-gen: Documentation generation plugin activated");

        // 注册 "doc" 任务
        let has_doc = ctx.tasks.iter().any(|t| t.name == "doc");
        if !has_doc {
            let task = TaskDefinition {
                name: "doc".to_string(),
                description: "Generate API documentation".to_string(),
                kind: TaskKind::Plugin("doc-gen".to_string()),
                depends_on: vec!["compile-main".to_string()],
                inputs: TaskInputs::default(),
                outputs: TaskOutputs {
                    dir: ctx.out_dir().join("docs"),
                    files: Vec::new(),
                },
            };
            ctx.add_task(task);
            tracing::info!("  Registered task: doc (depends on compile-main)");
        }

        Ok(())
    }

    fn execute(&self, task_name: &str, ctx: &PluginContext) -> Result<TaskResult, LoomError> {
        match task_name {
            "doc" => {
                // 扫描源码文件，生成 Markdown 文档
                let project_dir = &ctx.project_dir;
                let out_dir = project_dir.join(&ctx.build_config.out_dir).join("docs");
                std::fs::create_dir_all(&out_dir)?;

                // 发现源码文件
                let src_dir = project_dir.join("src");
                if !src_dir.exists() {
                    return Ok(TaskResult::ok(
                        "aura-doc-gen: no src directory, skipping doc generation",
                    ));
                }

                let mut files = Vec::new();
                discover_aura_files(&src_dir, &mut files);

                if files.is_empty() {
                    return Ok(TaskResult::ok(
                        "aura-doc-gen: no source files, skipping doc generation",
                    ));
                }

                // 生成文档
                let mut doc_content = String::from("# API Documentation\n\n");
                doc_content.push_str(&format!(
                    "> Project: {}\n> Version: {}\n> Generated: {}\n\n",
                    ctx.manifest.name,
                    ctx.manifest.version,
                    chrono_or_local()
                ));

                doc_content.push_str("## Module List\n\n");
                doc_content.push_str("| Module | File |\n");
                doc_content.push_str("|------|------|\n");

                let mut artifacts = Vec::new();
                for file in &files {
                    let rel = file.strip_prefix(project_dir).unwrap_or(file);
                    let module_name =
                        file.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                    doc_content.push_str(&format!("| `{}` | `{}` |\n", module_name, rel.display()));

                    // 为每个模块生成单独的文档页面
                    let module_doc_path = out_dir.join(format!("{}.md", module_name));
                    let module_doc = generate_module_doc(file, &ctx.manifest.name);
                    std::fs::write(&module_doc_path, &module_doc)?;
                    artifacts.push(module_doc_path);
                }

                // 写入主文档
                let main_doc_path = out_dir.join("index.md");
                std::fs::write(&main_doc_path, &doc_content)?;
                artifacts.push(main_doc_path);

                Ok(TaskResult::ok_with_artifacts(
                    format!(
                        "aura-doc-gen: generated {} module docs → {}",
                        files.len(),
                        out_dir.display()
                    ),
                    artifacts,
                ))
            }
            _ => Ok(TaskResult::err(format!(
                "aura-doc-gen: unknown task '{}' (supported tasks: doc)",
                task_name
            ))),
        }
    }
}

/// 递归发现 .aura 文件
fn discover_aura_files(dir: &std::path::Path, result: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                discover_aura_files(&path, result);
            } else if path.extension().map(|e| e == "aura").unwrap_or(false) {
                result.push(path);
            }
        }
    }
}

/// 为模块生成文档页面
fn generate_module_doc(file: &std::path::Path, project_name: &str) -> String {
    let module_name = file.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
    let rel = file.display().to_string();

    let mut doc = String::new();
    doc.push_str(&format!("# `{}`\n\n", module_name));
    doc.push_str(&format!("> Source: {}\n\n", rel));
    doc.push_str(&format!("> Project: {}\n\n", project_name));
    doc.push_str("---\n\n");
    doc.push_str("## Overview\n\n");
    doc.push_str("Auto-generated, no detailed documentation yet.\n\n");
    doc.push_str("## Public Interface\n\n");

    // 尝试读取源文件内容，提取函数声明
    if let Ok(content) = std::fs::read_to_string(file) {
        doc.push_str("```aura\n");
        doc.push_str(&content);
        doc.push_str("\n```\n");
    }

    doc
}

fn chrono_or_local() -> String {
    // 简单的本地时间字符串（不依赖 chrono crate）
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}s since epoch", now.as_secs())
}

// ═══════════════════════════════════════════════════════════════════════════════
// aura-format：格式化插件
// ═══════════════════════════════════════════════════════════════════════════════

/// 源码格式化插件
///
/// 格式化 Aura 源码文件。
/// 显式插件，需在 `[plugins]` 中设置 `aura-format = true`。
pub struct FormatPlugin;

impl BuildPlugin for FormatPlugin {
    fn name(&self) -> &str {
        "aura-format"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> PluginKind {
        PluginKind::Explicit
    }
    fn description(&self) -> Option<&str> {
        Some("Formatting plugin: unify source code format")
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-format");
        tracing::info!("aura-format: Formatting plugin activated");

        // 注册 "fmt" 任务
        let has_fmt = ctx.tasks.iter().any(|t| t.name == "fmt");
        if !has_fmt {
            let task = TaskDefinition {
                name: "fmt".to_string(),
                description: "Format source code".to_string(),
                kind: TaskKind::Plugin("fmt".to_string()),
                depends_on: Vec::new(),
                inputs: TaskInputs::default(),
                outputs: TaskOutputs::default(),
            };
            ctx.add_task(task);
            tracing::info!("  Registered task: fmt");
        }

        // 注册 "fmt-check" 任务
        let has_fmt_check = ctx.tasks.iter().any(|t| t.name == "fmt-check");
        if !has_fmt_check {
            let task = TaskDefinition {
                name: "fmt-check".to_string(),
                description: "Check source code format (no file modification)".to_string(),
                kind: TaskKind::Plugin("fmt-check".to_string()),
                depends_on: Vec::new(),
                inputs: TaskInputs::default(),
                outputs: TaskOutputs::default(),
            };
            ctx.add_task(task);
            tracing::info!("  Registered task: fmt-check");
        }

        Ok(())
    }

    fn execute(&self, task_name: &str, ctx: &PluginContext) -> Result<TaskResult, LoomError> {
        let project_dir = &ctx.project_dir;
        let src_dir = project_dir.join("src");

        if !src_dir.exists() {
            return Ok(TaskResult::ok("aura-format: no src directory, skipping"));
        }

        let mut files = Vec::new();
        discover_aura_files(&src_dir, &mut files);

        if files.is_empty() {
            return Ok(TaskResult::ok("aura-format: no source files, skipping"));
        }

        match task_name {
            "fmt" => {
                // 格式化：当前是占位符（实际格式化由 aura-fmt 工具处理）
                let count = files.len();
                Ok(TaskResult::ok(format!(
                    "aura-format: formatted {} files (placeholder, actual formatting pending)",
                    count
                )))
            }
            "fmt-check" => {
                // 检查模式：报告格式不一致的文件
                let count = files.len();
                Ok(TaskResult::ok(format!(
                    "aura-format: checked {} files format (placeholder, actual checking pending)",
                    count
                )))
            }
            _ => Ok(TaskResult::err(format!(
                "aura-format: unknown task '{}' (supported tasks: fmt, fmt-check)",
                task_name
            ))),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// aura-aot：AOT 编译插件
// ═══════════════════════════════════════════════════════════════════════════════

/// AOT 编译插件
///
/// 通过 LLVM 后端将 Aura 字节码编译为原生可执行文件。
/// 显式插件，需在 `[plugins]` 中设置 `aura-aot = true`。
/// 需要 `llvm` feature 编译。
pub struct AotPlugin;

impl BuildPlugin for AotPlugin {
    fn name(&self) -> &str {
        "aura-aot"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> PluginKind {
        PluginKind::Explicit
    }
    fn description(&self) -> Option<&str> {
        Some("AOT compilation plugin: compile native executables via LLVM backend")
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-aot");
        tracing::info!("aura-aot: AOT compilation plugin activated");

        // 注册 "aot" 任务
        let has_aot = ctx.tasks.iter().any(|t| t.name == "aot");
        if !has_aot {
            let task = TaskDefinition {
                name: "aot".to_string(),
                description: "AOT compile to native executable".to_string(),
                kind: TaskKind::Plugin("aot".to_string()),
                depends_on: vec!["compile-main".to_string()],
                inputs: TaskInputs::default(),
                outputs: TaskOutputs {
                    dir: ctx.out_dir().join("native"),
                    files: Vec::new(),
                },
            };
            ctx.add_task(task);
            tracing::info!("  Registered task: aot (depends on compile-main)");
        }

        Ok(())
    }

    fn execute(&self, _task_name: &str, _ctx: &PluginContext) -> Result<TaskResult, LoomError> {
        Ok(TaskResult::ok(
            "aura-aot: AOT compilation (placeholder, requires llvm feature and LLVM backend implementation)",
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// aura-ci：CI 插件
// ═══════════════════════════════════════════════════════════════════════════════

/// CI 插件
///
/// 生成 CI/CD 流水线配置（GitHub Actions, GitLab CI 等）。
/// 显式插件，需在 `[plugins]` 中设置 `aura-ci = true`。
pub struct CiPlugin;

impl BuildPlugin for CiPlugin {
    fn name(&self) -> &str {
        "aura-ci"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> PluginKind {
        PluginKind::Explicit
    }
    fn description(&self) -> Option<&str> {
        Some("CI plugin: generate CI/CD pipeline configuration")
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-ci");
        tracing::info!("aura-ci: CI plugin activated");

        // 注册 "ci" 任务
        let has_ci = ctx.tasks.iter().any(|t| t.name == "ci");
        if !has_ci {
            let task = TaskDefinition {
                name: "ci".to_string(),
                description: "Run full CI pipeline".to_string(),
                kind: TaskKind::Plugin("ci".to_string()),
                depends_on: vec![
                    "package".to_string(),
                    "verify".to_string(),
                ],
                inputs: TaskInputs::default(),
                outputs: TaskOutputs::default(),
            };
            ctx.add_task(task);
            tracing::info!("  Registered task: ci (depends on package, verify)");
        }

        Ok(())
    }

    fn execute(&self, _task_name: &str, _ctx: &PluginContext) -> Result<TaskResult, LoomError> {
        Ok(TaskResult::ok(
            "aura-ci: CI pipeline (placeholder, Phase B6 will implement full functionality)",
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 显式插件列表
// ═══════════════════════════════════════════════════════════════════════════════

/// 获取启用的显式插件
///
/// 根据 manifest 的 `[plugins]` 配置，返回需要加载的显式插件列表。
pub fn explicit_plugins(manifest: &crate::manifest::LoomManifest) -> Vec<Box<dyn BuildPlugin>> {
    let mut plugins: Vec<Box<dyn BuildPlugin>> = Vec::new();

    if manifest.plugins.aura_doc_gen {
        plugins.push(Box::new(DocGenPlugin));
        tracing::debug!("Activated explicit plugin: aura-doc-gen");
    }

    if manifest.plugins.aura_format {
        plugins.push(Box::new(FormatPlugin));
        tracing::debug!("Activated explicit plugin: aura-format");
    }

    if manifest.plugins.aura_aot {
        plugins.push(Box::new(AotPlugin));
        tracing::debug!("Activated explicit plugin: aura-aot");
    }

    if manifest.plugins.aura_ci {
        plugins.push(Box::new(CiPlugin));
        tracing::debug!("Activated explicit plugin: aura-ci");
    }

    plugins
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::LoomManifest;
    use crate::manifest::parse::default_manifest;
    use tempfile::TempDir;

    #[test]
    fn test_doc_gen_plugin_name() {
        let plugin = DocGenPlugin;
        assert_eq!(plugin.name(), "aura-doc-gen");
        assert_eq!(plugin.kind(), PluginKind::Explicit);
    }

    #[test]
    fn test_doc_gen_plugin_description() {
        let plugin = DocGenPlugin;
        assert!(plugin.description().is_some());
        assert!(plugin.description().unwrap().contains("Documentation"));
    }

    #[test]
    fn test_doc_gen_plugin_configure() {
        let plugin = DocGenPlugin;
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());
        assert!(ctx.is_plugin_active("aura-doc-gen"));

        // 应该注册了 doc 任务
        let has_doc = ctx.tasks.iter().any(|t| t.name == "doc");
        assert!(has_doc);

        let doc_task = ctx.tasks.iter().find(|t| t.name == "doc").unwrap();
        assert_eq!(doc_task.depends_on, vec!["compile-main".to_string()]);
    }

    #[test]
    fn test_doc_gen_plugin_configure_no_duplicate() {
        let plugin = DocGenPlugin;
        let mut ctx = PluginContext::new_default();
        ctx.add_task(TaskDefinition {
            name: "doc".to_string(),
            description: "Existing".to_string(),
            kind: TaskKind::Plugin("doc-gen".to_string()),
            depends_on: vec!["compile-main".to_string()],
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        });

        assert!(plugin.configure(&mut ctx).is_ok());
        let count = ctx.tasks.iter().filter(|t| t.name == "doc").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_doc_gen_plugin_execute_no_src() {
        let plugin = DocGenPlugin;
        let tmp = TempDir::new().unwrap();
        let mut ctx = PluginContext::new_default();
        ctx.project_dir = tmp.path().to_path_buf();
        ctx.build_config.out_dir = tmp.path().join("target/build").display().to_string();

        let result = plugin.execute("doc", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("no src directory"));
    }

    #[test]
    fn test_doc_gen_plugin_execute_with_sources() {
        let plugin = DocGenPlugin;
        let tmp = TempDir::new().unwrap();

        // 创建 src 目录和源码文件
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("main.aura"),
            "fun main() {\n    println(\"hello\")\n}\n",
        )
        .unwrap();
        std::fs::write(
            src.join("utils.aura"),
            "fun add(a: int, b: int) -> int {\n    a + b\n}\n",
        )
        .unwrap();

        let mut ctx = PluginContext::new_default();
        ctx.project_dir = tmp.path().to_path_buf();
        ctx.build_config.out_dir = "target/build".to_string();

        let result = plugin.execute("doc", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("2 module docs"));

        // 检查生成的文档
        let docs_dir = tmp.path().join("target/build/docs");
        assert!(docs_dir.exists());
        assert!(docs_dir.join("index.md").exists());
        assert!(docs_dir.join("main.md").exists());
        assert!(docs_dir.join("utils.md").exists());

        // 检查文档内容
        let index_content = std::fs::read_to_string(docs_dir.join("index.md")).unwrap();
        assert!(index_content.contains("API Documentation"));
        assert!(index_content.contains("test-project"));
        assert!(index_content.contains("`main`"));
        assert!(index_content.contains("`utils`"));
    }

    #[test]
    fn test_doc_gen_plugin_execute_unknown_task() {
        let plugin = DocGenPlugin;
        let ctx = PluginContext::new_default();
        let result = plugin.execute("unknown-task", &ctx).unwrap();
        assert!(!result.success);
        assert!(result.output.contains("unknown task"));
    }

    #[test]
    fn test_format_plugin_name() {
        let plugin = FormatPlugin;
        assert_eq!(plugin.name(), "aura-format");
        assert_eq!(plugin.kind(), PluginKind::Explicit);
    }

    #[test]
    fn test_format_plugin_configure_registers_tasks() {
        let plugin = FormatPlugin;
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());

        let has_fmt = ctx.tasks.iter().any(|t| t.name == "fmt");
        let has_fmt_check = ctx.tasks.iter().any(|t| t.name == "fmt-check");
        assert!(has_fmt);
        assert!(has_fmt_check);
    }

    #[test]
    fn test_format_plugin_execute_fmt() {
        let plugin = FormatPlugin;
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.aura"), "fun main(){}").unwrap();

        let mut ctx = PluginContext::new_default();
        ctx.project_dir = tmp.path().to_path_buf();

        let result = plugin.execute("fmt", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("formatted"));
    }

    #[test]
    fn test_format_plugin_execute_fmt_check() {
        let plugin = FormatPlugin;
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.aura"), "fun main(){}").unwrap();

        let mut ctx = PluginContext::new_default();
        ctx.project_dir = tmp.path().to_path_buf();

        let result = plugin.execute("fmt-check", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("checked"));
    }

    #[test]
    fn test_format_plugin_execute_unknown() {
        let plugin = FormatPlugin;
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.aura"), "fun main(){}").unwrap();

        let mut ctx = PluginContext::new_default();
        ctx.project_dir = tmp.path().to_path_buf();

        let result = plugin.execute("unknown", &ctx).unwrap();
        assert!(!result.success);
        assert!(result.output.contains("unknown task"));
    }

    #[test]
    fn test_aot_plugin_name() {
        let plugin = AotPlugin;
        assert_eq!(plugin.name(), "aura-aot");
        assert_eq!(plugin.kind(), PluginKind::Explicit);
    }

    #[test]
    fn test_aot_plugin_configure() {
        let plugin = AotPlugin;
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());

        let has_aot = ctx.tasks.iter().any(|t| t.name == "aot");
        assert!(has_aot);
    }

    #[test]
    fn test_aot_plugin_execute() {
        let plugin = AotPlugin;
        let ctx = PluginContext::new_default();
        let result = plugin.execute("aot", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("AOT"));
    }

    #[test]
    fn test_ci_plugin_name() {
        let plugin = CiPlugin;
        assert_eq!(plugin.name(), "aura-ci");
        assert_eq!(plugin.kind(), PluginKind::Explicit);
    }

    #[test]
    fn test_ci_plugin_configure() {
        let plugin = CiPlugin;
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());

        let has_ci = ctx.tasks.iter().any(|t| t.name == "ci");
        assert!(has_ci);

        let ci_task = ctx.tasks.iter().find(|t| t.name == "ci").unwrap();
        assert!(ci_task.depends_on.contains(&"package".to_string()));
        assert!(ci_task.depends_on.contains(&"verify".to_string()));
    }

    #[test]
    fn test_ci_plugin_execute() {
        let plugin = CiPlugin;
        let ctx = PluginContext::new_default();
        let result = plugin.execute("ci", &ctx).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_explicit_plugins_default_manifest() {
        let manifest = default_manifest("test");
        let plugins = explicit_plugins(&manifest);
        // 默认情况下显式插件都不启用
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_explicit_plugins_enabled() {
        let toml_str = r#"
name = "test"
version = "1.0.0"

[plugins]
aura-doc-gen = true
aura-format = true
aura-aot = false
aura-ci = false
"#;
        let manifest: LoomManifest = toml::from_str(toml_str).unwrap();
        let plugins = explicit_plugins(&manifest);
        assert_eq!(plugins.len(), 2);
        assert!(plugins.iter().any(|p| p.name() == "aura-doc-gen"));
        assert!(plugins.iter().any(|p| p.name() == "aura-format"));
    }

    #[test]
    fn test_explicit_plugins_all_enabled() {
        let toml_str = r#"
name = "test"
version = "1.0.0"

[plugins]
aura-doc-gen = true
aura-format = true
aura-aot = true
aura-ci = true
"#;
        let manifest: LoomManifest = toml::from_str(toml_str).unwrap();
        let plugins = explicit_plugins(&manifest);
        assert_eq!(plugins.len(), 4);
    }

    #[test]
    fn test_all_explicit_plugins_configure() {
        let toml_str = r#"
name = "test"
version = "1.0.0"

[plugins]
aura-doc-gen = true
aura-format = true
aura-aot = true
aura-ci = true
"#;
        let manifest: LoomManifest = toml::from_str(toml_str).unwrap();
        let plugins = explicit_plugins(&manifest);
        let mut ctx = PluginContext::new_default();

        for plugin in &plugins {
            assert!(
                plugin.configure(&mut ctx).is_ok(),
                "Plugin {} should configure successfully",
                plugin.name()
            );
        }

        // 检查所有任务都注册了
        let task_names: Vec<&str> = ctx.tasks.iter().map(|t| t.name.as_str()).collect();
        assert!(task_names.contains(&"doc"));
        assert!(task_names.contains(&"fmt"));
        assert!(task_names.contains(&"fmt-check"));
        assert!(task_names.contains(&"aot"));
        assert!(task_names.contains(&"ci"));
    }
}
