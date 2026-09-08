//! JIT 原生函数调度器（C 兼容，供 JIT 直接调用）
//!
//! 提供 C 兼容的调度函数，JIT 生成对此函数的调用，替代回退到解释器。

use super::NativeRegistry;
use crate::vm::BytecodeNative;
use crate::vm::abi::JitValue;
use crate::vm::value::Value;

/// 全局 NativeRegistry 指针（VM 初始化时设置）
static mut NATIVE_REGISTRY_PTR: *mut NativeRegistry = std::ptr::null_mut();

/// 全局 BytecodeNative 数组指针（VM 初始化时设置，供 JIT 调度器使用）
static mut NATIVES_PTR: *const BytecodeNative = std::ptr::null();

/// 设置全局 NativeRegistry 指针（VM 初始化时调用）
pub fn set_native_registry(reg: *mut NativeRegistry) {
    unsafe {
        NATIVE_REGISTRY_PTR = reg;
    }
}

/// 获取全局 NativeRegistry 指针
pub fn get_native_registry() -> *mut NativeRegistry {
    unsafe { NATIVE_REGISTRY_PTR }
}

/// 设置全局 BytecodeNative 数组指针（VM 初始化时调用）
pub fn set_natives_ptr(natives: *const BytecodeNative) {
    unsafe {
        NATIVES_PTR = natives;
    }
}

/// 获取全局 BytecodeNative 数组指针
pub fn get_natives_ptr() -> *const BytecodeNative {
    unsafe { NATIVES_PTR }
}

/// JIT 原生函数调度器（C 兼容）
///
/// JIT 生成对此函数的调用，替代回退到解释器。
/// 参数：
/// - native_idx: 原生函数索引
/// - argc: 参数个数
/// - args_ptr: 参数数组指针（JitValue 格式）
/// - out_ptr: 返回值输出指针（JitValue 格式）
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aura_jit_call_native_by_index(
    native_idx: i64,
    argc: i64,
    args_ptr: *const JitValue,
    out_ptr: *mut JitValue,
) {
    let reg = match get_native_registry() {
        p if !p.is_null() => &*p,
        _ => {
            *out_ptr = JitValue::null();
            return;
        }
    };

    let natives_ptr = match get_natives_ptr() {
        p if !p.is_null() => p,
        _ => {
            *out_ptr = JitValue::null();
            return;
        }
    };

    let native_idx = native_idx as usize;
    let argc = argc as usize;

    // 从 natives 表获取函数名
    let native = &*natives_ptr.add(native_idx);
    let name = &native.name;

    // 查找原生函数
    if let Some(native_fn) = reg.get(name) {
        // 将 JitValue 参数转换为 Value 数组
        let values: Vec<Value> = (0..argc).map(|i| (*args_ptr.add(i)).to_value()).collect();

        // 调用原生函数
        let result = native_fn(&values);

        // 将返回值转换为 JitValue
        let jit_result = JitValue::from_value(&result);
        *out_ptr = jit_result;
    } else {
        // 函数未注册，返回 Null
        *out_ptr = JitValue::null();
    }
}
