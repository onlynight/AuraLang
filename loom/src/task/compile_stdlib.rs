//! Phase 2: Standard library compilation task.
//!
//! Compiles .aura source files in the core directory into .auc bytecode files,
//! optionally compiles the C FFI library, and packages the result as a
//! stdlib artifact.
//!
//! Corresponds to Phase 2 §5.2 in the full Aura-ification plan.

use std::path::{Path, PathBuf};

use crate::error::LoomError;
use crate::stdlib::{
    ExecutionMode, FfiIndex, FfiMode, StdlibIndex, compile_cffi_library, generate_ffi_index,
    generate_stdlib_index, scan_aura_files,
};
use crate::task::TaskDefinition;

// ═══════════════════════════════════════════════════════════════════════════════
// Core compilation function
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of a stdlib compilation run.
#[derive(Debug)]
pub struct StdlibCompileResult {
    /// Standard library index (module list + metadata)
    pub stdlib_index: StdlibIndex,
    /// FFI index (extern declarations + pre-loaded addresses)
    pub ffi_index: FfiIndex,
    /// .auc bytecode files produced
    pub auc_files: Vec<PathBuf>,
    /// C FFI static library path (if compiled)
    pub cffi_library: Option<PathBuf>,
    /// Index file path (stdlib_index.json)
    pub index_path: Option<PathBuf>,
    /// FFI index file path (ffi_index.json)
    pub ffi_index_path: Option<PathBuf>,
}

/// Compile all .aura files in `core_dir` into .auc bytecode files.
///
/// This is the main entry point for Phase 2 stdlib compilation:
/// 1. Scans `core_dir` for .aura files
/// 2. Compiles each .aura → .auc via `compiler::codegen::compile_source`
/// 3. Generates stdlib index (module metadata)
/// 4. Generates FFI index (extern declarations)
/// 5. Optionally compiles the C FFI library (if `cffi_dir` exists)
/// 6. Writes index files as JSON
///
/// # Arguments
/// * `core_dir` - Directory containing .aura source files (e.g. "core")
/// * `output_dir` - Output directory for .auc files and indexes
/// * `cffi_dir` - Directory containing .c FFI source files (optional)
/// * `execution_mode` - Target execution mode (Vm / Jit / Aot)
/// * `ffi_mode` - FFI mode (Aot / Cffi / RustFfi)
///
/// # Returns
/// [`StdlibCompileResult`] containing all produced artifacts and indexes.
pub fn compile_stdlib(
    core_dir: &Path,
    output_dir: &Path,
    cffi_dir: Option<&Path>,
    execution_mode: ExecutionMode,
    ffi_mode: FfiMode,
) -> Result<StdlibCompileResult, LoomError> {
    use compiler::codegen::{compile_source, write_auc};

    tracing::info!(
        "aura-stdlib: compiling stdlib (mode: {}, ffi: {})",
        execution_mode,
        ffi_mode
    );

    // 1. Scan .aura files
    let aura_files =
        scan_aura_files(core_dir).map_err(|e| LoomError::Config(format!("scan failed: {}", e)))?;

    tracing::info!("aura-stdlib: found {} .aura files", aura_files.len());

    if aura_files.is_empty() {
        return Err(LoomError::Config(format!(
            "no .aura files found in {}",
            core_dir.display()
        )));
    }

    // 2. Create output directory
    std::fs::create_dir_all(output_dir)
        .map_err(|e| LoomError::Config(format!("cannot create {}: {}", output_dir.display(), e)))?;

    // 3. Compile each .aura → .auc
    let mut auc_files = Vec::new();
    let mut compile_errors = Vec::new();

    for aura_file in &aura_files {
        let module_name = aura_file.file_stem().and_then(|s| s.to_str()).unwrap_or("module");
        let auc_path = output_dir.join(format!("{}.auc", module_name));

        let source = std::fs::read_to_string(aura_file).map_err(|e| {
            LoomError::Config(format!("cannot read {}: {}", aura_file.display(), e))
        })?;

        match compile_source(&source) {
            Ok(module) => {
                if let Err(e) = write_auc(&auc_path.to_string_lossy(), &module) {
                    compile_errors.push(format!("write failed for {}: {}", module_name, e));
                    continue;
                }
                tracing::debug!("  compiled {} → {}", module_name, auc_path.display());
                auc_files.push(auc_path);
            }
            Err(e) => {
                compile_errors.push(format!("compile failed for {}: {}", module_name, e));
            }
        }
    }

    if compile_errors.is_empty() && auc_files.is_empty() {
        return Err(LoomError::Config(
            "all .aura files failed to compile".to_string(),
        ));
    }

    // 4. Generate stdlib index
    let stdlib_index = generate_stdlib_index(
        &aura_files,
        output_dir.to_path_buf(),
        execution_mode,
        ffi_mode,
    )
    .map_err(|e| LoomError::Config(format!("stdlib index generation failed: {}", e)))?;

    // 5. Generate FFI index
    let ffi_index = generate_ffi_index(&aura_files, ffi_mode)
        .map_err(|e| LoomError::Config(format!("FFI index generation failed: {}", e)))?;

    // 6. Compile C FFI library (optional)
    let mut cffi_library = None;
    if let Some(dir) = cffi_dir {
        let result = compile_cffi_library(dir, output_dir);
        if result.success {
            tracing::info!("aura-stdlib: C FFI library compiled");
        } else {
            tracing::warn!(
                "aura-stdlib: C FFI library compilation failed: {}",
                result.log
            );
        }
        cffi_library = result.library_path;
    }

    // 7. Write index files
    let index_path = write_json_file(output_dir, "stdlib_index.json", &stdlib_index)?;
    let ffi_index_path = write_json_file(output_dir, "ffi_index.json", &ffi_index)?;

    // 8. Log summary
    let success_count = auc_files.len();
    let fail_count = compile_errors.len();
    tracing::info!(
        "aura-stdlib: compiled {}/{} modules (mode: {}, ffi: {})",
        success_count,
        aura_files.len(),
        execution_mode,
        ffi_mode
    );
    if !compile_errors.is_empty() {
        tracing::warn!(
            "aura-stdlib: {} module(s) failed to compile",
            compile_errors.len()
        );
        for err in &compile_errors {
            tracing::warn!("  {}", err);
        }
    }

    Ok(StdlibCompileResult {
        stdlib_index,
        ffi_index,
        auc_files,
        cffi_library,
        index_path,
        ffi_index_path,
    })
}

/// Write a JSON-serializable object to a file.
fn write_json_file<T: serde::Serialize>(
    dir: &Path,
    filename: &str,
    value: &T,
) -> Result<Option<PathBuf>, LoomError> {
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(value).map_err(|e| {
        LoomError::Config(format!("JSON serialization failed for {}: {}", filename, e))
    })?;
    std::fs::write(&path, json)
        .map_err(|e| LoomError::Config(format!("cannot write {}: {}", path.display(), e)))?;
    Ok(Some(path))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Task-level wrapper
// ═══════════════════════════════════════════════════════════════════════════════

/// Execute the `compile-stdlib` task.
///
/// This is the task-level entry point called by the task executor.
/// It reads the task definition to determine input/output directories
/// and delegates to [`compile_stdlib`].
pub fn execute_compile_stdlib(
    task: &TaskDefinition,
    core_dir: &Path,
    output_dir: &Path,
    cffi_dir: Option<&Path>,
    execution_mode: ExecutionMode,
    ffi_mode: FfiMode,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let result = compile_stdlib(core_dir, output_dir, cffi_dir, execution_mode, ffi_mode)?;

    let mut artifacts = result.auc_files.clone();
    if let Some(ref lib) = result.cffi_library {
        artifacts.push(lib.clone());
    }
    if let Some(ref idx) = result.index_path {
        artifacts.push(idx.clone());
    }
    if let Some(ref ffi) = result.ffi_index_path {
        artifacts.push(ffi.clone());
    }

    let module_count = result.stdlib_index.len();
    let ffi_count = result.ffi_index.len();
    let message = format!(
        "stdlib compiled: {} modules, {} FFI declarations (mode: {}, ffi: {})",
        module_count, ffi_count, execution_mode, ffi_mode
    );

    Ok((message, artifacts))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid Aura source for testing.
    fn make_valid_aura_source() -> String {
        r#"
fun abs(x: Int): Int {
    if (x < 0) -x else x
}

fun add(a: Int, b: Int): Int {
    a + b
}
"#
        .to_string()
    }

    #[test]
    fn test_compile_stdlib_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let core_dir = tmp.path().join("core");
        std::fs::create_dir_all(&core_dir).unwrap();

        let result = compile_stdlib(
            &core_dir,
            &tmp.path().join("output"),
            None,
            ExecutionMode::Vm,
            FfiMode::Aot,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no .aura files"));
    }

    #[test]
    fn test_compile_stdlib_single_module() {
        let tmp = tempfile::tempdir().unwrap();
        let core_dir = tmp.path().join("core");
        std::fs::create_dir_all(&core_dir).unwrap();

        // Write a simple .aura file
        std::fs::write(core_dir.join("Math.aura"), make_valid_aura_source()).unwrap();

        let output_dir = tmp.path().join("output");
        let result = compile_stdlib(
            &core_dir,
            &output_dir,
            None,
            ExecutionMode::Vm,
            FfiMode::Aot,
        )
        .unwrap();

        // Should have produced at least one .auc file
        assert!(result.auc_files.len() >= 1);

        // Stdlib index should have at least one module
        assert!(result.stdlib_index.len() >= 1);

        // Math module should have abs and add functions
        let math_module = result.stdlib_index.find_module("Math").unwrap();
        assert!(math_module.function_names.iter().any(|n| n == "abs"));
        assert!(math_module.function_names.iter().any(|n| n == "add"));

        // Index files should be written
        assert!(result.index_path.is_some());
        assert!(result.ffi_index_path.is_some());

        // Verify index file content
        let index_file = result.index_path.unwrap();
        assert!(index_file.exists());
        let json = std::fs::read_to_string(&index_file).unwrap();
        let loaded: StdlibIndex = serde_json::from_str(&json).unwrap();
        assert!(loaded.len() >= 1);
    }

    #[test]
    fn test_compile_stdlib_multiple_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let core_dir = tmp.path().join("core");
        std::fs::create_dir_all(&core_dir).unwrap();

        std::fs::write(core_dir.join("Math.aura"), make_valid_aura_source()).unwrap();
        std::fs::write(
            core_dir.join("String.aura"),
            "fun length(s: String): Int { s.len() }\n",
        )
        .unwrap();

        let output_dir = tmp.path().join("output");
        let result = compile_stdlib(
            &core_dir,
            &output_dir,
            None,
            ExecutionMode::Vm,
            FfiMode::Aot,
        )
        .unwrap();

        assert!(result.auc_files.len() >= 2);
        assert!(result.stdlib_index.len() >= 2);
    }

    #[test]
    fn test_compile_stdlib_with_ffi() {
        let tmp = tempfile::tempdir().unwrap();
        let core_dir = tmp.path().join("core");
        std::fs::create_dir_all(&core_dir).unwrap();

        // Write a .aura file with extern declarations
        std::fs::write(
            core_dir.join("FileSystem.aura"),
            r#"
extern "c" "libc" {
    fun fopen(path: String, mode: String): Pointer
    fun fclose(stream: Pointer): Int
}
"#,
        )
        .unwrap();

        let output_dir = tmp.path().join("output");
        let result = compile_stdlib(
            &core_dir,
            &output_dir,
            None,
            ExecutionMode::Vm,
            FfiMode::Aot,
        )
        .unwrap();

        // FFI index should have at least 2 declarations (fopen, fclose)
        assert!(result.ffi_index.len() >= 2);

        // FileSystem module should have extern flag
        let fs_module = result.stdlib_index.find_module("FileSystem").unwrap();
        assert!(fs_module.has_extern);
    }

    #[test]
    fn test_compile_stdlib_aot_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let core_dir = tmp.path().join("core");
        std::fs::create_dir_all(&core_dir).unwrap();

        std::fs::write(core_dir.join("Math.aura"), make_valid_aura_source()).unwrap();

        let output_dir = tmp.path().join("output");
        let result = compile_stdlib(
            &core_dir,
            &output_dir,
            None,
            ExecutionMode::Aot,
            FfiMode::Aot,
        )
        .unwrap();

        // AOT mode should still produce .auc files (bytecode is always generated)
        assert!(result.auc_files.len() >= 1);
        assert_eq!(result.stdlib_index.execution_mode, ExecutionMode::Aot);
    }

    #[test]
    fn test_compile_stdlib_jit_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let core_dir = tmp.path().join("core");
        std::fs::create_dir_all(&core_dir).unwrap();

        std::fs::write(core_dir.join("Math.aura"), make_valid_aura_source()).unwrap();

        let output_dir = tmp.path().join("output");
        let result = compile_stdlib(
            &core_dir,
            &output_dir,
            None,
            ExecutionMode::Jit,
            FfiMode::Cffi,
        )
        .unwrap();

        assert!(result.auc_files.len() >= 1);
        assert_eq!(result.stdlib_index.execution_mode, ExecutionMode::Jit);
        assert_eq!(result.stdlib_index.ffi_mode, FfiMode::Cffi);
    }

    #[test]
    fn test_compile_stdlib_recursive_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let core_dir = tmp.path().join("core");
        let std_dir = core_dir.join("aura").join("lang").join("std");
        std::fs::create_dir_all(&std_dir).unwrap();

        std::fs::write(std_dir.join("Math.aura"), make_valid_aura_source()).unwrap();

        let output_dir = tmp.path().join("output");
        let result = compile_stdlib(
            &core_dir,
            &output_dir,
            None,
            ExecutionMode::Vm,
            FfiMode::Aot,
        )
        .unwrap();

        assert!(result.stdlib_index.len() >= 1);
        let math_module = result.stdlib_index.find_module("Math").unwrap();
        assert_eq!(math_module.full_name, "aura.lang.std.Math");
    }
}
