// ================================================================
// 【冻结基线】Rust VM 双实现常驻（VM-PA-00 v3.2 / D6 / P4.3）
// [FROZEN BASELINE] Rust VM dual-impl co-resident (D6/P4.3): bug-fix only, no new capabilities, never reference the Aura VM.
// 本文件属于 Rust VM（只读冻结的兼容基线、第二实现）：
//   - 禁止新增能力，仅允许修 bug 与安全修补；
//   - 不得引用 Aura VM 实现（aura/compiler/aura/lang/compiler/vm/）；
//   - 与 Aura VM 的唯一交集是 .auc 二进制格式（Rust 编译器产出，两侧各自执行）。
// 依据：docs/vm_pure_aura/00-VM纯Aura化技术方案与达成路径.md 三-阶段P4 / 决策D6。
// ================================================================
//! 动态 FFI 加载器（5.9）
//!
//! 对应 技术方案 §3.9 的 `extern "c"` 动态加载需求。在 `dynamic-ffi` feature 下
//! 使用 `libloading` crate 运行时 `dlopen` 动态库，提取函数指针并注册到原生调度表。

use std::collections::HashMap;

use crate::codegen::opcode::FfiAbi;
use crate::vm::value::Value;

/// 动态加载的库实例（保持 Library 存活以防止 dlopen 引用计数归零）
#[cfg(feature = "dynamic-ffi")]
struct LoadedLib {
    path: String,
    lib: libloading::Library,
    /// P8-Rust: 库的 ABI 类型
    abi: FfiAbi,
}

/// 动态 FFI 加载器
pub struct DynamicLoader {
    #[cfg(feature = "dynamic-ffi")]
    libs: Vec<LoadedLib>,
    /// 函数名 → 原生函数
    fns: HashMap<String, fn(&[Value]) -> Value>,
}

impl Default for DynamicLoader {
    fn default() -> Self {
        DynamicLoader::new()
    }
}

impl DynamicLoader {
    pub fn new() -> Self {
        DynamicLoader {
            #[cfg(feature = "dynamic-ffi")]
            libs: Vec::new(),
            fns: HashMap::new(),
        }
    }

    /// 注册原生函数（静态或动态加载后）
    pub fn register_func(&mut self, name: &str, f: fn(&[Value]) -> Value) {
        self.fns.insert(name.to_string(), f);
    }

    /// 查找注册的函数
    pub fn get(&self, name: &str) -> Option<fn(&[Value]) -> Value> {
        self.fns.get(name).copied()
    }

    /// 是否存在该名称的函数
    pub fn contains(&self, name: &str) -> bool {
        self.fns.contains_key(name)
    }

    /// 已注册函数数量
    pub fn len(&self) -> usize {
        self.fns.len()
    }

    /// 运行时加载动态库（dlopen）
    ///
    /// 无 `dynamic-ffi` feature 时为空操作（返回 Ok）。
    ///
    /// `abi` 标记库的 ABI 类型（C / Rust），用于元数据追踪。
    pub fn load_lib(&mut self, path: &str, abi: FfiAbi) -> Result<(), String> {
        #[cfg(feature = "dynamic-ffi")]
        {
            let lib = unsafe {
                libloading::Library::new(path)
                    .map_err(|e| format!("failed to load {}: {}", path, e))?
            };
            self.libs.push(LoadedLib {
                path: path.to_string(),
                lib,
                abi,
            });
            Ok(())
        }
        #[cfg(not(feature = "dynamic-ffi"))]
        {
            let _ = path;
            let _ = abi;
            Ok(())
        }
    }

    /// 从已加载的动态库中解析并注册函数
    ///
    /// `name` 是 C 函数名，`native_fn` 是 Rust 包装函数（`fn(&[Value]) -> Value`），
    /// 由调用方负责从动态库符号转换参数后调用原始 C 函数。
    pub fn resolve_and_register(&mut self, name: &str, native_fn: fn(&[Value]) -> Value) {
        self.register_func(name, native_fn);
    }
}

#[cfg(feature = "dynamic-ffi")]
impl DynamicLoader {
    /// 从最后一个加载的库中获取原始符号
    pub fn get_symbol<'a, T>(&'a self, name: &str) -> Result<libloading::Symbol<'a, T>, String> {
        let last = self.libs.last().ok_or_else(|| "no library loaded".to_string())?;
        let sym = unsafe { last.lib.get(name.as_bytes()) };
        sym.map_err(|e| format!("symbol '{}' not found: {}", name, e))
    }

    /// 获取已加载库的路径
    pub fn loaded_libs(&self) -> Vec<String> {
        self.libs.iter().map(|l| l.path.clone()).collect()
    }
}
