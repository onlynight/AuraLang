//! [Phase B4.4] 外部插件加载（.so/.dll）
//!
//! 外部插件编译为动态库（.so / .dll / .dylib），通过 libloading 加载。
//!
//! ## ABI 规范
//!
//! 外部插件库必须导出以下 C 函数：
//!
//! ```c
//! // 返回插件元信息（C ABI）
//! extern "C" fn aura_plugin_info() -> AuraPluginInfo;
//!
//! // 配置插件（注册任务等）
//! extern "C" fn aura_plugin_configure() -> i32;
//!
//! // 执行任务
//! // 返回 0 表示成功，非 0 表示失败
//! extern "C" fn aura_plugin_execute(
//!     task_name: *const c_char,
//!     output_buf: *mut c_char,
//!     output_buf_len: usize
//! ) -> i32;
//! ```
//!
//! 其中 `AuraPluginInfo` 结构体：
//! ```c
//! struct AuraPluginInfo {
//!     name: *const c_char,       // 插件名称（静态字符串）
//!     version: *const c_char,    // 插件版本（静态字符串）
//!     description: *const c_char,// 插件描述（可为 null）
//! }
//! ```
//!
//! 对应设计文档 §9.1 `PluginKind::External`。

use crate::error::LoomError;
use crate::plugin::context::PluginContext;
use crate::plugin::r#trait::BuildPlugin;
use crate::plugin::{PluginKind, TaskResult};
use libloading::{Library, Symbol};
use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI 定义
// ═══════════════════════════════════════════════════════════════════════════════

/// 插件元信息（C ABI 结构体）
#[repr(C)]
#[derive(Debug, Default)]
pub struct AuraPluginInfo {
    /// 插件名称（静态 C 字符串指针）
    pub name: *const c_char,
    /// 插件版本（静态 C 字符串指针）
    pub version: *const c_char,
    /// 插件描述（静态 C 字符串指针，可为 null）
    pub description: *const c_char,
}

impl AuraPluginInfo {
    /// 安全读取插件名称
    pub fn name_str(&self) -> Result<&str, LoomError> {
        if self.name.is_null() {
            return Err(LoomError::Plugin("Plugin name is null".to_string()));
        }
        unsafe {
            CStr::from_ptr(self.name)
                .to_str()
                .map_err(|e| LoomError::Plugin(format!("Plugin name encoding error: {}", e)))
        }
    }

    /// 安全读取插件版本
    pub fn version_str(&self) -> Result<&str, LoomError> {
        if self.version.is_null() {
            return Err(LoomError::Plugin("Plugin version is null".to_string()));
        }
        unsafe {
            CStr::from_ptr(self.version)
                .to_str()
                .map_err(|e| LoomError::Plugin(format!("Plugin version encoding error: {}", e)))
        }
    }

    /// 安全读取插件描述（可为 null）
    pub fn description_str(&self) -> Option<&str> {
        if self.description.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(self.description).to_str().ok() }
    }
}

/// 外部插件加载错误
#[derive(Debug)]
pub enum ExternalPluginError {
    /// 库文件不存在
    FileNotFound(PathBuf),
    /// 无法加载动态库
    LoadError(String),
    /// 找不到导出函数
    MissingSymbol(String),
    /// 函数调用失败
    CallError(String),
    /// 无效的插件信息
    InvalidInfo(String),
}

impl std::fmt::Display for ExternalPluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExternalPluginError::FileNotFound(path) => {
                write!(f, "Plugin file not found: {}", path.display())
            }
            ExternalPluginError::LoadError(e) => write!(f, "Dynamic library load failed: {}", e),
            ExternalPluginError::MissingSymbol(name) => {
                write!(f, "Exported function not found: {}", name)
            }
            ExternalPluginError::CallError(msg) => write!(f, "Plugin call failed: {}", msg),
            ExternalPluginError::InvalidInfo(msg) => write!(f, "Invalid plugin info: {}", msg),
        }
    }
}

impl std::error::Error for ExternalPluginError {}

// ═══════════════════════════════════════════════════════════════════════════════
// 外部插件实现
// ═══════════════════════════════════════════════════════════════════════════════

/// 外部插件（通过 libloading 加载的 .so/.dll）
///
/// 持有加载的动态库引用和函数指针。
/// 原始 C 字符串在加载时解析为 Rust 字符串，确保 Send+Sync 安全。
pub struct ExternalPlugin {
    /// 加载的动态库（保持存活，防止符号被卸载）
    #[allow(dead_code)]
    library: Library,
    /// 插件名称（从 FFI 解析后存储为 Rust String）
    name: String,
    /// 插件版本
    version: String,
    /// 插件描述
    description: Option<String>,
    /// 配置函数指针
    configure_fn: extern "C" fn() -> i32,
    /// 执行函数指针
    execute_fn: extern "C" fn(*const c_char, *mut c_char, usize) -> i32,
}

// Safety: Library and extern "C" fn pointers are thread-safe.
// The strings are owned and safe. The raw pointers in AuraPluginInfo
// are not stored, only parsed strings are kept.
unsafe impl Send for ExternalPlugin {}
unsafe impl Sync for ExternalPlugin {}

impl ExternalPlugin {
    /// 从路径加载外部插件
    ///
    /// 加载动态库并解析导出函数。
    /// 库必须导出 `aura_plugin_info`, `aura_plugin_configure`, `aura_plugin_execute`。
    pub fn load(path: &Path) -> Result<Self, ExternalPluginError> {
        if !path.exists() {
            return Err(ExternalPluginError::FileNotFound(path.to_path_buf()));
        }

        let library = unsafe { Library::new(path) }
            .map_err(|e| ExternalPluginError::LoadError(e.to_string()))?;

        // 加载导出函数并提取函数指针（可 Copy，不借用 library）
        let info_fn: extern "C" fn() -> AuraPluginInfo = unsafe {
            let symbol: Symbol<'_, extern "C" fn() -> AuraPluginInfo> = library
                .get(b"aura_plugin_info")
                .map_err(|e| ExternalPluginError::LoadError(e.to_string()))?;
            *symbol
        };

        let info = info_fn();

        if info.name.is_null() || info.version.is_null() {
            return Err(ExternalPluginError::InvalidInfo(
                "Plugin info name and version cannot be null".to_string(),
            ));
        }

        // 解析 C 字符串为 Rust String（确保 Send+Sync）
        let name = unsafe { CStr::from_ptr(info.name).to_string_lossy().to_string() };
        let version = unsafe { CStr::from_ptr(info.version).to_string_lossy().to_string() };
        let description = if info.description.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(info.description).to_string_lossy().to_string() })
        };

        let configure_fn: extern "C" fn() -> i32 = unsafe {
            let symbol: Symbol<'_, extern "C" fn() -> i32> = library
                .get(b"aura_plugin_configure")
                .map_err(|e| ExternalPluginError::LoadError(e.to_string()))?;
            *symbol
        };

        let execute_fn: extern "C" fn(*const c_char, *mut c_char, usize) -> i32 = unsafe {
            let symbol: Symbol<'_, extern "C" fn(*const c_char, *mut c_char, usize) -> i32> =
                library
                    .get(b"aura_plugin_execute")
                    .map_err(|e| ExternalPluginError::LoadError(e.to_string()))?;
            *symbol
        };

        Ok(Self {
            library,
            name,
            version,
            description,
            configure_fn,
            execute_fn,
        })
    }

    /// 获取插件名称
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 获取插件版本
    pub fn version(&self) -> &str {
        &self.version
    }

    /// 获取插件描述
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// 调用插件的 configure 函数
    pub fn call_configure(&self) -> Result<(), ExternalPluginError> {
        let rc = (self.configure_fn)();
        if rc == 0 {
            Ok(())
        } else {
            Err(ExternalPluginError::CallError(format!(
                "Plugin '{}' configure returned error code {}",
                self.name, rc
            )))
        }
    }

    /// 调用插件的 execute 函数
    pub fn call_execute(
        &self,
        task_name: &str,
        output_buf: &mut [u8],
    ) -> Result<(), ExternalPluginError> {
        let task_cstr = CString::new(task_name).map_err(|e| {
            ExternalPluginError::CallError(format!("Task name encoding error: {}", e))
        })?;

        let rc = (self.execute_fn)(
            task_cstr.as_ptr(),
            output_buf.as_mut_ptr() as *mut c_char,
            output_buf.len(),
        );

        if rc == 0 {
            Ok(())
        } else {
            Err(ExternalPluginError::CallError(format!(
                "Plugin '{}' execute('{}') returned error code {}",
                self.name, task_name, rc
            )))
        }
    }
}

impl BuildPlugin for ExternalPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn kind(&self) -> PluginKind {
        PluginKind::External
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn configure(&self, _ctx: &mut PluginContext) -> Result<(), LoomError> {
        self.call_configure()
            .map_err(|e| LoomError::Plugin(format!("External plugin configure failed: {}", e)))
    }

    fn execute(&self, task_name: &str, _ctx: &PluginContext) -> Result<TaskResult, LoomError> {
        // 准备输出缓冲区（2KB）
        let mut buf = vec![0u8; 2048];

        self.call_execute(task_name, &mut buf)
            .map_err(|e| LoomError::Plugin(format!("External plugin execute failed: {}", e)))?;

        // 从缓冲区提取输出字符串
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        let output = String::from_utf8_lossy(&buf[..end]).to_string();

        if output.is_empty() {
            Ok(TaskResult::ok(format!("Plugin '{}' executed", self.name)))
        } else {
            Ok(TaskResult::ok(output))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 外部插件路径检测
// ═══════════════════════════════════════════════════════════════════════════════

/// 检测路径是否为有效的插件动态库
pub fn is_plugin_library(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| matches!(ext, "so" | "dll" | "dylib"))
        .unwrap_or(false)
}

/// 加载外部插件（从路径，带扩展名检测）
pub fn load_external_plugin(path: &Path) -> Result<Box<dyn BuildPlugin>, LoomError> {
    if !path.exists() {
        return Err(LoomError::Plugin(format!(
            "External plugin file not found: {}",
            path.display()
        )));
    }

    if !is_plugin_library(path) {
        return Err(LoomError::Plugin(format!(
            "External plugin file format not supported (requires .so/.dll/.dylib): {}",
            path.display()
        )));
    }

    let plugin = ExternalPlugin::load(path).map_err(|e| LoomError::Plugin(e.to_string()))?;
    Ok(Box::new(plugin))
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginKind;

    #[test]
    fn test_aura_plugin_info_null_name() {
        let info = AuraPluginInfo {
            name: std::ptr::null(),
            version: std::ptr::null(),
            description: std::ptr::null(),
        };
        assert!(info.name_str().is_err());
        assert!(info.version_str().is_err());
        assert!(info.description_str().is_none());
    }

    #[test]
    fn test_aura_plugin_info_valid() {
        let name = b"test-plugin\0".as_ptr() as *const c_char;
        let version = b"1.0.0\0".as_ptr() as *const c_char;
        let desc = b"A test plugin\0".as_ptr() as *const c_char;

        let info = AuraPluginInfo {
            name,
            version,
            description: desc,
        };

        assert_eq!(info.name_str().unwrap(), "test-plugin");
        assert_eq!(info.version_str().unwrap(), "1.0.0");
        assert_eq!(info.description_str().unwrap(), "A test plugin");
    }

    #[test]
    fn test_aura_plugin_info_no_description() {
        let name = b"my-plugin\0".as_ptr() as *const c_char;
        let version = b"0.5.0\0".as_ptr() as *const c_char;

        let info = AuraPluginInfo {
            name,
            version,
            description: std::ptr::null(),
        };

        assert_eq!(info.name_str().unwrap(), "my-plugin");
        assert_eq!(info.version_str().unwrap(), "0.5.0");
        assert!(info.description_str().is_none());
    }

    #[test]
    fn test_aura_plugin_info_default() {
        let info = AuraPluginInfo::default();
        assert!(info.name.is_null());
        assert!(info.version.is_null());
        assert!(info.description.is_null());
    }

    #[test]
    fn test_is_plugin_library_so() {
        assert!(is_plugin_library(Path::new("foo.so")));
    }

    #[test]
    fn test_is_plugin_library_dll() {
        assert!(is_plugin_library(Path::new("foo.dll")));
    }

    #[test]
    fn test_is_plugin_library_dylib() {
        assert!(is_plugin_library(Path::new("foo.dylib")));
    }

    #[test]
    fn test_is_plugin_library_invalid() {
        assert!(!is_plugin_library(Path::new("foo.txt")));
        assert!(!is_plugin_library(Path::new("foo.rs")));
        assert!(!is_plugin_library(Path::new("no_extension")));
    }

    #[test]
    fn test_is_plugin_library_no_extension() {
        assert!(!is_plugin_library(Path::new("Makefile")));
    }

    #[test]
    fn test_load_external_plugin_file_not_found() {
        let result = load_external_plugin(Path::new("/nonexistent/plugin.so"));
        assert!(result.is_err());
        let err = if let Err(e) = result { e.to_string() } else { unreachable!() };
        assert!(err.contains("not found"));
    }

    #[test]
    fn test_load_external_plugin_invalid_extension() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("plugin.txt");
        std::fs::write(&file, "not a library").unwrap();

        let result = load_external_plugin(&file);
        assert!(result.is_err());
        let err = if let Err(e) = result { e.to_string() } else { unreachable!() };
        assert!(err.contains("not supported"));
    }

    #[test]
    fn test_external_plugin_error_display_file_not_found() {
        let err = ExternalPluginError::FileNotFound(PathBuf::from("/test/foo.so"));
        assert!(err.to_string().contains("not found"));
        assert!(err.to_string().contains("foo.so"));
    }

    #[test]
    fn test_external_plugin_error_display_missing_symbol() {
        let err = ExternalPluginError::MissingSymbol("aura_plugin_info".to_string());
        assert!(err.to_string().contains("not found"));
        assert!(err.to_string().contains("aura_plugin_info"));
    }

    #[test]
    fn test_external_plugin_error_display_call_error() {
        let err = ExternalPluginError::CallError("returned error code 1".to_string());
        assert!(err.to_string().contains("call failed"));
        assert!(err.to_string().contains("1"));
    }

    #[test]
    fn test_external_plugin_error_display_invalid_info() {
        let err = ExternalPluginError::InvalidInfo("name 为 null".to_string());
        assert!(err.to_string().contains("Invalid plugin info"));
    }

    #[test]
    fn test_external_plugin_load_file_not_found() {
        let result = ExternalPlugin::load(Path::new("/nonexistent/foo.so"));
        assert!(result.is_err());
        if let Err(e) = result {
            match e {
                ExternalPluginError::FileNotFound(_) => {}
                _ => panic!("Expected FileNotFound error"),
            }
        }
    }

    #[test]
    fn test_external_plugin_load_invalid_format() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("invalid.so");
        std::fs::write(&file, "this is not a valid shared library").unwrap();

        let result = ExternalPlugin::load(&file);
        assert!(result.is_err());
    }

    #[test]
    fn test_external_plugin_error_is_error() {
        let err = ExternalPluginError::CallError("test".to_string());
        let dyn_err: &dyn std::error::Error = &err;
        assert!(!dyn_err.to_string().is_empty());
    }
}
