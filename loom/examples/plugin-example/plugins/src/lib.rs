//! 自定义 Aura 构建插件（C ABI 实现）
//!
//! 编译为动态库（.so / .dll / .dylib），通过 libloading 加载。
//!
//! 必须导出的 C ABI 符号：
//!   - aura_plugin_info: 返回插件元信息
//!   - aura_plugin_configure: 配置阶段回调
//!   - aura_plugin_execute: 任务执行回调
//!
//! 编译命令：
//!   cd plugins && cargo build --release
//!   # Windows: target/release/custom_plugin.dll
//!   # Linux:   target/release/libcustom_plugin.so
//!   # macOS:   target/release/libcustom_plugin.dylib

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI 结构体（与 loom 中的 AuraPluginInfo 匹配）
// ═══════════════════════════════════════════════════════════════════════════════

/// 插件元信息（C ABI 结构体）
#[repr(C)]
#[derive(Debug, Default)]
pub struct AuraPluginInfo {
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 静态字符串（插件元信息）
// ═══════════════════════════════════════════════════════════════════════════════

static PLUGIN_NAME: &[u8] = b"custom-plugin\0";
static PLUGIN_VERSION: &[u8] = b"0.1.0\0";
static PLUGIN_DESCRIPTION: &[u8] = b"Custom plugin: registers greet task\0";

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI 导出函数
// ═══════════════════════════════════════════════════════════════════════════════

/// 返回插件元信息
#[no_mangle]
pub extern "C" fn aura_plugin_info() -> AuraPluginInfo {
    AuraPluginInfo {
        name: PLUGIN_NAME.as_ptr() as *const c_char,
        version: PLUGIN_VERSION.as_ptr() as *const c_char,
        description: PLUGIN_DESCRIPTION.as_ptr() as *const c_char,
    }
}

/// 配置阶段回调
///
/// 在构建开始前调用。插件可在此阶段：
///   - 注册自定义任务
///   - 修改源码集配置
///   - 设置默认构建选项
///
/// 返回 0 表示成功，非零表示失败。
#[no_mangle]
pub extern "C" fn aura_plugin_configure() -> i32 {
    println!("[custom-plugin] configure: plugin configured");
    println!("[custom-plugin]   registered custom task: greet");
    println!("[custom-plugin]   usage: loom build --task greet");
    0
}

/// 任务执行回调
///
/// 当任务调度器执行到本插件注册的任务时调用。
///
/// 参数：
///   - task_name: 任务名称（C 字符串）
///   - output_buf: 输出缓冲区（可写）
///   - buf_size: 缓冲区大小
///
/// 返回 0 表示成功，非零表示失败。
#[no_mangle]
pub extern "C" fn aura_plugin_execute(
    task_name: *const c_char,
    output_buf: *mut c_char,
    buf_size: usize,
) -> i32 {
    if task_name.is_null() {
        return 1;
    }

    let task = unsafe { CStr::from_ptr(task_name) }.to_string_lossy().to_string();

    // 根据任务名执行不同逻辑
    let message = match task.as_str() {
        "greet" => "Hello from custom-plugin! Greet task executed successfully.".to_string(),
        "greet-world" => "Hello World! Greeting from custom plugin.".to_string(),
        _ => format!("Unknown task: {}", task),
    };

    // 写入输出缓冲区
    if !output_buf.is_null() && buf_size > 0 {
        let bytes = message.as_bytes();
        let len = std::cmp::min(bytes.len() + 1, buf_size);
        // c_char is i8 on most platforms; cast to u8 for byte-wise copy
        let u8_slice = unsafe { std::slice::from_raw_parts_mut(output_buf as *mut u8, len) };
        u8_slice[..bytes.len()].copy_from_slice(bytes);
        u8_slice[bytes.len()] = 0; // null terminator
    }

    println!("[custom-plugin] execute('{}'): {}", task, message);
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_info() {
        let info = aura_plugin_info();
        let name = unsafe { CStr::from_ptr(info.name) };
        let version = unsafe { CStr::from_ptr(info.version) };
        let desc = unsafe { CStr::from_ptr(info.description) };

        assert_eq!(name.to_str().unwrap(), "custom-plugin");
        assert_eq!(version.to_str().unwrap(), "0.1.0");
        assert_eq!(desc.to_str().unwrap(), "Custom plugin: registers greet task");
    }

    #[test]
    fn test_configure() {
        let rc = aura_plugin_configure();
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_execute_greet() {
        let task = CString::new("greet").unwrap();
        let mut buf = [0u8; 256];

        let rc = aura_plugin_execute(
            task.as_ptr(),
            buf.as_mut_ptr() as *mut c_char,
            buf.len(),
        );

        assert_eq!(rc, 0);
        let output = String::from_utf8_lossy(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(buf.len())]);
        assert!(output.contains("Hello from custom-plugin"));
    }

    #[test]
    fn test_execute_unknown() {
        let task = CString::new("unknown-task").unwrap();
        let mut buf = [0u8; 256];

        let rc = aura_plugin_execute(
            task.as_ptr(),
            buf.as_mut_ptr() as *mut c_char,
            buf.len(),
        );

        assert_eq!(rc, 0);
        let output = String::from_utf8_lossy(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(buf.len())]);
        assert!(output.contains("Unknown task"));
    }
}
