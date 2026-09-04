//! 动态 FFI 加载器（5.9）
//!
//! 对应 技术方案 §3.9 的 `extern "c"` 动态加载需求。在 `dynamic-ffi` feature 下
//! 使用 `libloading` crate 运行时 `dlopen` 动态库，提取函数指针并注册到原生调度表。

use std::collections::HashMap;

use crate::vm::value::Value;

/// 动态加载的库实例（保持 Library 存活以防止 dlopen 引用计数归零）
#[cfg(feature = "dynamic-ffi")]
struct LoadedLib {
    path: String,
    lib: libloading::Library,
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
    pub fn load_lib(&mut self, path: &str) -> Result<(), String> {
        #[cfg(feature = "dynamic-ffi")]
        {
            let lib = libloading::Library::new(path)
                .map_err(|e| format!("failed to load {}: {}", path, e))?;
            self.libs.push(LoadedLib {
                path: path.to_string(),
                lib,
            });
            Ok(())
        }
        #[cfg(not(feature = "dynamic-ffi"))]
        {
            let _ = path;
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
        let last = self
            .libs
            .last()
            .ok_or_else(|| "no library loaded".to_string())?;
        last.lib
            .get(name.as_bytes())
            .map_err(|e| format!("symbol '{}' not found: {}", name, e))
    }

    /// 获取已加载库的路径
    pub fn loaded_libs(&self) -> &[String] {
        &self.libs.iter().map(|l| l.path.clone()).collect::<Vec<_>>()
    }
}
