//! [Phase B4.2] Convention plugin loading.
//!
//! Convention plugins are auto-activated at build start, providing default
//! behaviour. They do not need to be declared in aura.toml.
//!
//! Built-in convention plugins:
//! - `aura-stdlib`: compiles core/ stdlib source files into .auc bytecode + indexes
//! - `aura-test-harness`: registers `run-tests` task, injects test framework
//! - `aura-watch`: file watching + incremental rebuild
//!
//! Corresponds to design document §9.2 built-in plugin list.

use std::path::{Path, PathBuf};

use crate::error::LoomError;
use crate::plugin::context::PluginContext;
use crate::plugin::r#trait::BuildPlugin;
use crate::plugin::{PluginKind, TaskResult};
use crate::stdlib::{ExecutionMode, FfiMode};
use crate::task::compile_stdlib::compile_stdlib;
use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs};

// ═══════════════════════════════════════════════════════════════════════════════
// aura-stdlib: standard library plugin (Phase 2)
// ═══════════════════════════════════════════════════════════════════════════════

/// Standard library plugin.
///
/// Phase 2: actually compiles core/ .aura files into .auc bytecode,
/// generates stdlib index and FFI index, and compiles the C FFI library.
/// Convention plugin, auto-activated.
///
/// Configuration:
/// - `core_dir`: source directory (default: `<project_dir>/core`, fallback: `phantom-source`)
/// - `cffi_dir`: C FFI source directory (default: `<project_dir>/core/cffi`, optional)
/// - `execution_mode`: target mode (from build config: vm | jit | aot)
/// - `ffi_mode`: FFI mode (from build config: aot | cffi)
pub struct StdlibPlugin {
    /// Override for the core source directory (None = auto-detect).
    pub core_dir: Option<PathBuf>,
    /// Override for the C FFI source directory (None = auto-detect).
    pub cffi_dir: Option<PathBuf>,
}

impl Default for StdlibPlugin {
    fn default() -> Self {
        Self {
            core_dir: None,
            cffi_dir: None,
        }
    }
}

impl BuildPlugin for StdlibPlugin {
    fn name(&self) -> &str {
        "aura-stdlib"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn kind(&self) -> PluginKind {
        PluginKind::Convention
    }
    fn description(&self) -> Option<&str> {
        Some(
            "Standard library plugin: compiles core/ .aura files into .auc bytecode, generates stdlib index + FFI index (Phase 2)",
        )
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-stdlib");

        // Resolve core source directory (graceful fallback if not found)
        let core_dir = match resolve_core_dir(&ctx.project_dir, &self.core_dir) {
            Ok(dir) => dir,
            Err(e) => {
                tracing::warn!(
                    "aura-stdlib: core directory not available, skipping stdlib compilation: {}",
                    e
                );
                return Ok(());
            }
        };

        // Resolve C FFI source directory
        let cffi_dir = if let Some(ref dir) = self.cffi_dir {
            Some(dir.clone())
        } else {
            let default_cffi = core_dir.join("cffi");
            if default_cffi.is_dir() { Some(default_cffi) } else { None }
        };

        // Determine output directory
        let output_dir = ctx.out_dir().join("stdlib");

        // Determine execution mode from build config
        let execution_mode = map_compile_mode(ctx.build_config.mode);

        // Determine FFI mode from build config
        let ffi_mode = map_ffi_mode(ctx.build_config.ffi_mode.clone());

        tracing::info!(
            "aura-stdlib: compiling (core: {}, mode: {}, ffi: {})",
            core_dir.display(),
            execution_mode,
            ffi_mode
        );

        // Run the compilation
        match compile_stdlib(
            &core_dir,
            &output_dir,
            cffi_dir.as_deref(),
            execution_mode,
            ffi_mode,
        ) {
            Ok(result) => {
                let module_count = result.stdlib_index.len();
                let ffi_count = result.ffi_index.len();
                let auc_count = result.auc_files.len();

                tracing::info!(
                    "aura-stdlib: compiled {}/{} modules, {} FFI declarations",
                    auc_count,
                    module_count,
                    ffi_count
                );

                // Register a compile-stdlib task to record the result
                let task = TaskDefinition {
                    name: "compile-stdlib".to_string(),
                    description: "Compile standard library (Phase 2)".to_string(),
                    kind: TaskKind::Plugin("aura-stdlib".to_string()),
                    depends_on: Vec::new(),
                    inputs: TaskInputs {
                        files: result.auc_files.clone(),
                        ..Default::default()
                    },
                    outputs: TaskOutputs {
                        files: result.auc_files.clone(),
                        dir: output_dir.clone(),
                    },
                };
                ctx.add_task(task);
            }
            Err(e) => {
                tracing::warn!(
                    "aura-stdlib: compilation failed (continuing without stdlib): {}",
                    e
                );
            }
        }

        Ok(())
    }

    fn execute(&self, task_name: &str, _ctx: &PluginContext) -> Result<TaskResult, LoomError> {
        if task_name == "compile-stdlib" {
            Ok(TaskResult::ok(
                "aura-stdlib: stdlib compilation completed (results stored in output dir)",
            ))
        } else {
            Ok(TaskResult::ok(
                "aura-stdlib: no task to execute (compilation runs in configure phase)",
            ))
        }
    }
}

/// Resolve the core source directory.
///
/// Lookup order:
/// 1. Explicit override (`StdlibPlugin.core_dir`)
/// 2. `AURA_CORE_DIR` environment variable
/// 3. `<project_dir>/core`
/// 4. `<project_dir>/phantom-source` (backward compatibility)
fn resolve_core_dir(
    project_dir: &Path,
    override_dir: &Option<PathBuf>,
) -> Result<PathBuf, LoomError> {
    // 1. Explicit override
    if let Some(dir) = override_dir {
        if dir.is_dir() {
            return Ok(dir.clone());
        }
        return Err(LoomError::Config(format!(
            "configured core directory does not exist: {}",
            dir.display()
        )));
    }

    // 2. Environment variable
    if let Ok(env_dir) = std::env::var("AURA_CORE_DIR") {
        let path = PathBuf::from(&env_dir);
        if path.is_dir() {
            return Ok(path);
        }
    }

    // 3. <project_dir>/core
    let core = project_dir.join("core");
    if core.is_dir() {
        return Ok(core);
    }

    // 4. <project_dir>/phantom-source (backward compat)
    let phantom = project_dir.join("phantom-source");
    if phantom.is_dir() {
        tracing::warn!("aura-stdlib: 'core' not found, falling back to 'phantom-source'");
        return Ok(phantom);
    }

    Err(LoomError::Config(format!(
        "core source directory not found (looked for 'core' and 'phantom-source' in {})",
        project_dir.display()
    )))
}

/// Map loom's CompileMode to stdlib's ExecutionMode.
fn map_compile_mode(mode: crate::manifest::CompileMode) -> ExecutionMode {
    match mode {
        crate::manifest::CompileMode::Vm => ExecutionMode::Vm,
        crate::manifest::CompileMode::Jit => ExecutionMode::Jit,
        crate::manifest::CompileMode::Aot => ExecutionMode::Aot,
    }
}

/// Map loom's FfiMode to stdlib's FfiMode.
fn map_ffi_mode(mode: crate::manifest::FfiMode) -> FfiMode {
    match mode {
        crate::manifest::FfiMode::Cabi => FfiMode::Cffi,
        crate::manifest::FfiMode::Aot => FfiMode::Aot,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// aura-test-harness: test framework plugin
// ═══════════════════════════════════════════════════════════════════════════════

/// Test framework plugin.
///
/// Registers `run-tests` task and injects test framework support.
/// Convention plugin, auto-activated.
pub struct TestHarnessPlugin;

impl BuildPlugin for TestHarnessPlugin {
    fn name(&self) -> &str {
        "aura-test-harness"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> PluginKind {
        PluginKind::Convention
    }
    fn description(&self) -> Option<&str> {
        Some("Test framework plugin: registers run-tests task, injects test framework")
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-test-harness");
        tracing::info!("aura-test-harness: test framework activated");

        let has_run_tests = ctx.tasks.iter().any(|t| t.name == "run-tests");

        if !has_run_tests {
            let task = TaskDefinition {
                name: "run-tests".to_string(),
                description: "Execute tests".to_string(),
                kind: TaskKind::Test,
                depends_on: vec!["compile-test".to_string()],
                inputs: TaskInputs::default(),
                outputs: TaskOutputs::default(),
            };
            ctx.add_task(task);
            tracing::info!("  registered task: run-tests");
        }

        Ok(())
    }

    fn execute(&self, _task_name: &str, _ctx: &PluginContext) -> Result<TaskResult, LoomError> {
        Ok(TaskResult::ok("aura-test-harness: test framework ready"))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// aura-watch: Watch mode plugin
// ═══════════════════════════════════════════════════════════════════════════════

/// Watch mode plugin.
///
/// Enables file watching + incremental rebuild.
/// Convention plugin, auto-activated.
pub struct WatchPlugin;

impl BuildPlugin for WatchPlugin {
    fn name(&self) -> &str {
        "aura-watch"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> PluginKind {
        PluginKind::Convention
    }
    fn description(&self) -> Option<&str> {
        Some("Watch mode plugin: file watching + incremental rebuild")
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-watch");
        tracing::info!("aura-watch: Watch mode plugin activated");

        let has_watch = ctx.tasks.iter().any(|t| t.name == "watch");

        if !has_watch {
            let task = TaskDefinition {
                name: "watch".to_string(),
                description: "Watch source changes, incremental rebuild".to_string(),
                kind: TaskKind::Watch,
                depends_on: vec!["resolve".to_string()],
                inputs: TaskInputs::default(),
                outputs: TaskOutputs::default(),
            };
            ctx.add_task(task);
            tracing::info!("  registered task: watch");
        }

        Ok(())
    }

    fn execute(&self, _task_name: &str, _ctx: &PluginContext) -> Result<TaskResult, LoomError> {
        Ok(TaskResult::ok(
            "aura-watch: Watch mode ready (Phase B7 implements full functionality)",
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Convention plugin list
// ═══════════════════════════════════════════════════════════════════════════════

/// Get all built-in convention plugins.
///
/// These plugins are auto-activated at build start, no need to declare in aura.toml.
/// Users can disable via `[plugins]` table with `aura-xxx = false`.
pub fn convention_plugins(manifest: &crate::manifest::LoomManifest) -> Vec<Box<dyn BuildPlugin>> {
    let mut plugins: Vec<Box<dyn BuildPlugin>> = Vec::new();

    if manifest.plugins.aura_stdlib {
        plugins.push(Box::new(StdlibPlugin::default()));
        tracing::debug!("activated convention plugin: aura-stdlib");
    }

    if manifest.plugins.aura_test_harness {
        plugins.push(Box::new(TestHarnessPlugin));
        tracing::debug!("activated convention plugin: aura-test-harness");
    }

    if manifest.plugins.aura_watch {
        plugins.push(Box::new(WatchPlugin));
        tracing::debug!("activated convention plugin: aura-watch");
    }

    plugins
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse::default_manifest;

    #[test]
    fn test_stdlib_plugin_name() {
        let plugin = StdlibPlugin::default();
        assert_eq!(plugin.name(), "aura-stdlib");
        assert_eq!(plugin.version(), "2.0.0");
        assert_eq!(plugin.kind(), PluginKind::Convention);
    }

    #[test]
    fn test_stdlib_plugin_description() {
        let plugin = StdlibPlugin::default();
        assert!(plugin.description().is_some());
        assert!(plugin.description().unwrap().contains("Standard library"));
    }

    #[test]
    fn test_stdlib_plugin_configure() {
        let plugin = StdlibPlugin::default();
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());
        assert!(ctx.is_plugin_active("aura-stdlib"));
    }

    #[test]
    fn test_stdlib_plugin_execute() {
        let plugin = StdlibPlugin::default();
        let ctx = PluginContext::new_default();
        let result = plugin.execute("any", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("aura-stdlib"));
    }

    #[test]
    fn test_test_harness_plugin_name() {
        let plugin = TestHarnessPlugin;
        assert_eq!(plugin.name(), "aura-test-harness");
        assert_eq!(plugin.kind(), PluginKind::Convention);
    }

    #[test]
    fn test_test_harness_plugin_configure_registers_task() {
        let plugin = TestHarnessPlugin;
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());

        let has_run_tests = ctx.tasks.iter().any(|t| t.name == "run-tests");
        assert!(has_run_tests);

        let run_tests = ctx.tasks.iter().find(|t| t.name == "run-tests").unwrap();
        assert_eq!(run_tests.depends_on, vec!["compile-test".to_string()]);
    }

    #[test]
    fn test_test_harness_plugin_no_duplicate_task() {
        let plugin = TestHarnessPlugin;
        let mut ctx = PluginContext::new_default();

        ctx.add_task(TaskDefinition {
            name: "run-tests".to_string(),
            description: "already exists".to_string(),
            kind: TaskKind::Test,
            depends_on: vec!["compile-test".to_string()],
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        });

        assert!(plugin.configure(&mut ctx).is_ok());
        let count = ctx.tasks.iter().filter(|t| t.name == "run-tests").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_test_harness_plugin_execute() {
        let plugin = TestHarnessPlugin;
        let ctx = PluginContext::new_default();
        let result = plugin.execute("run-tests", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("aura-test-harness"));
    }

    #[test]
    fn test_watch_plugin_name() {
        let plugin = WatchPlugin;
        assert_eq!(plugin.name(), "aura-watch");
        assert_eq!(plugin.kind(), PluginKind::Convention);
    }

    #[test]
    fn test_watch_plugin_configure_registers_task() {
        let plugin = WatchPlugin;
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());

        let has_watch = ctx.tasks.iter().any(|t| t.name == "watch");
        assert!(has_watch);
    }

    #[test]
    fn test_watch_plugin_no_duplicate_task() {
        let plugin = WatchPlugin;
        let mut ctx = PluginContext::new_default();
        ctx.add_task(TaskDefinition {
            name: "watch".to_string(),
            description: "already exists".to_string(),
            kind: TaskKind::Watch,
            depends_on: vec!["resolve".to_string()],
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        });

        assert!(plugin.configure(&mut ctx).is_ok());
        let count = ctx.tasks.iter().filter(|t| t.name == "watch").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_convention_plugins_default_manifest() {
        let manifest = default_manifest("test");
        let plugins = convention_plugins(&manifest);
        assert!(plugins.len() >= 3);
        assert!(plugins.iter().any(|p| p.name() == "aura-stdlib"));
        assert!(plugins.iter().any(|p| p.name() == "aura-test-harness"));
        assert!(plugins.iter().any(|p| p.name() == "aura-watch"));
    }

    #[test]
    fn test_convention_plugins_disabled() {
        let toml_str = r#"
name = "test"
version = "1.0.0"

[plugins]
aura-stdlib = false
aura-test-harness = false
aura-watch = false
"#;
        let manifest: crate::manifest::LoomManifest = toml::from_str(toml_str).unwrap();
        let plugins = convention_plugins(&manifest);
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_convention_plugins_partial_disable() {
        let toml_str = r#"
name = "test"
version = "1.0.0"

[plugins]
aura-stdlib = false
aura-test-harness = true
aura-watch = false
"#;
        let manifest: crate::manifest::LoomManifest = toml::from_str(toml_str).unwrap();
        let plugins = convention_plugins(&manifest);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name(), "aura-test-harness");
    }

    #[test]
    fn test_all_convention_plugins_configure() {
        let manifest = default_manifest("test");
        let plugins = convention_plugins(&manifest);
        let mut ctx = PluginContext::new_default();

        for plugin in &plugins {
            assert!(
                plugin.configure(&mut ctx).is_ok(),
                "Plugin {} should configure successfully",
                plugin.name()
            );
        }

        assert!(ctx.is_plugin_active("aura-stdlib"));
        assert!(ctx.is_plugin_active("aura-test-harness"));
        assert!(ctx.is_plugin_active("aura-watch"));
    }

    #[test]
    fn test_resolve_core_dir_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        // Create phantom-source directory (fallback path)
        let phantom = tmp.path().join("phantom-source");
        std::fs::create_dir_all(&phantom).unwrap();

        let result = resolve_core_dir(tmp.path(), &None).unwrap();
        assert!(result.ends_with("phantom-source"));
    }

    #[test]
    fn test_resolve_core_dir_core_prefers_over_phantom() {
        let tmp = tempfile::tempdir().unwrap();
        // Create both core and phantom-source directories
        let core = tmp.path().join("core");
        std::fs::create_dir_all(&core).unwrap();
        let phantom = tmp.path().join("phantom-source");
        std::fs::create_dir_all(&phantom).unwrap();

        let result = resolve_core_dir(tmp.path(), &None).unwrap();
        assert!(result.ends_with("core"));
    }

    #[test]
    fn test_resolve_core_dir_explicit_override() {
        let tmp = tempfile::tempdir().unwrap();
        let custom = tmp.path().join("my-core");
        std::fs::create_dir_all(&custom).unwrap();

        let result = resolve_core_dir(tmp.path(), &Some(custom.clone())).unwrap();
        assert_eq!(result, custom);
    }

    #[test]
    fn test_resolve_core_dir_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let result = resolve_core_dir(tmp.path(), &None);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_compile_mode() {
        assert_eq!(
            map_compile_mode(crate::manifest::CompileMode::Vm),
            ExecutionMode::Vm
        );
        assert_eq!(
            map_compile_mode(crate::manifest::CompileMode::Jit),
            ExecutionMode::Jit
        );
        assert_eq!(
            map_compile_mode(crate::manifest::CompileMode::Aot),
            ExecutionMode::Aot
        );
    }

    #[test]
    fn test_map_ffi_mode() {
        assert_eq!(map_ffi_mode(crate::manifest::FfiMode::Cabi), FfiMode::Cffi);
        assert_eq!(map_ffi_mode(crate::manifest::FfiMode::Aot), FfiMode::Aot);
    }
}
