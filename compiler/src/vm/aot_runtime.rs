//! AOT 运行时模块
//!
//! Phase 1 AOT 机器码嵌入方案的核心运行时：
//! - [`AotModule`]：单个模块 —— mmap 机器码段（W^X），解析描述符表，构建分发表
//! - [`AotRuntime`]：模块注册表 + 函数派发
//!
//! 对应 docs/AOT机器码嵌入方案-详细设计.md §5。机器码入口与 [`JitEntry`]
//! 签名一致（共享 JitValue ABI），VM 分发与 JIT 分支对称。

use std::collections::HashMap;
use std::os::raw::c_void;
use std::path::Path;

use crate::codegen::opcode::{AucSegment, AuraFuncDesc, FUNC_EXPORT, SEG_DESC_TABLE, SEG_MACHINE};
use crate::vm::abi::{AotCallContext, AotEntry, JitValue};
use crate::vm::mmap_util::{MappedRegion, MemoryProtection};

/// Phase 3.2: AOT 调用深度上限（设计文档 §3.2）
pub const AOT_MAX_CALL_DEPTH: u32 = 1024;

/// Phase 3.6: 模块诊断信息（设计文档 §3.6）
#[derive(Debug, Clone)]
pub struct ModuleDiagnostics {
    pub module_id: u32,
    pub name: String,
    pub is_loaded: bool,
    pub code_base: usize,
    pub func_count: usize,
    pub dispatch_count: usize,
    pub entry_offsets: Vec<u64>,
}

/// A single loaded AOT module
pub struct AotModule {
    pub module_id: u32,
    pub name: String,
    code_region: Option<MappedRegion>,
    pub func_descriptors: Vec<AuraFuncDesc>,
    pub dispatch_table: Vec<Option<AotEntry>>,
}

impl AotModule {
    pub fn load(
        data: &[u8],
        segments: &[AucSegment],
        func_desc_idx: &[u32],
        module_id: u32,
        name: String,
    ) -> Result<Self, String> {
        let machine = segments
            .iter()
            .find(|s| s.id == SEG_MACHINE)
            .ok_or_else(|| "missing SEG_MACHINE".to_string())?;
        let desc_seg = segments
            .iter()
            .find(|s| s.id == SEG_DESC_TABLE)
            .ok_or_else(|| "missing SEG_DESC_TABLE".to_string())?;

        if machine.size == 0 {
            return Err("machine code segment is empty".to_string());
        }
        let m_end = (machine.offset + machine.size) as usize;
        if m_end > data.len() {
            return Err(format!(
                "machine segment out of range: offset={} size={} data={}",
                machine.offset,
                machine.size,
                data.len()
            ));
        }
        let d_end = (desc_seg.offset + desc_seg.size) as usize;
        if d_end > data.len() {
            return Err(format!(
                "descriptor segment out of range: offset={} size={} data={}",
                desc_seg.offset,
                desc_seg.size,
                data.len()
            ));
        }

        let mut region =
            MappedRegion::map_anonymous(machine.size as usize, MemoryProtection::read_write())?;
        let slice = region.as_mut_slice();
        let bytes = &data[machine.offset as usize..][..machine.size as usize];
        slice[..bytes.len()].copy_from_slice(bytes);
        unsafe {
            region.protect(MemoryProtection::read_exec())?;
        }
        let base = region.base;

        let desc_bytes = &data[desc_seg.offset as usize..][..desc_seg.size as usize];
        let descs = Self::parse_segments(desc_bytes)?;
        let dispatch_table = Self::rebuild_dispatch_table(base, &descs, func_desc_idx);

        Ok(AotModule {
            module_id,
            name,
            code_region: Some(region),
            func_descriptors: descs,
            dispatch_table,
        })
    }

    /// 从动态库导出符号构建 AOT 模块（Tier 2 共享库加载）
    ///
    /// 用于 `load_shared_library`：dlopen 后通过 dlsym 获取每个 `aura_aot_*` 符号地址，
    /// 直接构建分发表（无 mmap，OS 管理内存）。
    ///
    /// `symbols`：`(函数名, 入口函数指针, 参数个数, 返回标签, 参数标签编码)`
    pub fn from_symbols(
        symbols: Vec<(String, AotEntry, u8, u8, u8)>,
        module_id: u32,
        name: String,
    ) -> Self {
        let descs: Vec<AuraFuncDesc> = symbols
            .iter()
            .map(|(_, _, nargs, ret_tag, arg_tags)| AuraFuncDesc {
                name_offset: 0,
                name_len: 0,
                _pad1: 0,
                entry_offset: 0, // 共享库模式不使用 entry_offset（绝对地址）
                num_args: *nargs,
                arg_tags: *arg_tags,
                return_tag: *ret_tag,
                flags: FUNC_EXPORT,
                source_line: 0,
                source_file_offset: 0,
                _pad2: 0,
            })
            .collect();

        let dispatch_table: Vec<Option<AotEntry>> =
            symbols.iter().map(|(_, entry, _, _, _)| Some(*entry)).collect();

        AotModule {
            module_id,
            name,
            code_region: None,
            func_descriptors: descs,
            dispatch_table,
        }
    }

    fn parse_segments(data: &[u8]) -> Result<Vec<AuraFuncDesc>, String> {
        if data.len() % AuraFuncDesc::SIZE != 0 {
            return Err(format!(
                "descriptor table size not multiple of {}: {}",
                AuraFuncDesc::SIZE,
                data.len()
            ));
        }
        let count = data.len() / AuraFuncDesc::SIZE;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let off = i * AuraFuncDesc::SIZE;
            let addr = unsafe { data.as_ptr().add(off) as *const AuraFuncDesc };
            let desc = unsafe { std::ptr::read_unaligned(addr) };
            if desc.name_len as usize > data.len() {
                return Err(format!("function {} name_len out of range", i));
            }
            out.push(desc);
        }
        Ok(out)
    }

    fn rebuild_dispatch_table(
        base: usize,
        descs: &[AuraFuncDesc],
        func_desc_idx: &[u32],
    ) -> Vec<Option<AotEntry>> {
        let mut table = vec![None; func_desc_idx.len()];
        for (func_idx, &di) in func_desc_idx.iter().enumerate() {
            if di == 0 || (di as usize) > descs.len() {
                continue;
            }
            let off = descs[(di as usize) - 1].entry_offset as usize;
            if off > 0 && off % 16 == 0 {
                let addr = base + off;
                let ptr: *const c_void = addr as *const c_void;
                table[func_idx] =
                    Some(unsafe { std::mem::transmute::<*const c_void, AotEntry>(ptr) });
            }
        }
        table
    }

    pub fn unload(&mut self) {
        self.dispatch_table.clear();
        self.code_region.take();
    }

    pub fn find_entry(&self, func_idx: usize) -> Option<AotEntry> {
        self.dispatch_table.get(func_idx).copied().flatten()
    }

    pub fn is_loaded(&self) -> bool {
        self.code_region.is_some()
    }

    pub fn code_base(&self) -> usize {
        self.code_region.as_ref().map(|r| r.base).unwrap_or(0)
    }

    /// Phase 4.3: 解析函数名（从描述符的 name_offset 提取）
    /// 需要字符串池支持，当前使用默认名称
    pub fn resolve_func_name(&self, func_idx: usize) -> Option<String> {
        self.func_descriptors.get(func_idx).and_then(|desc| {
            if desc.name_len == 0 {
                None
            } else {
                // 描述符中存储了 name_offset 和 name_len
                // 但字符串池数据在 .auc 文件的 SEG_STRING_POOL 段中
                // 这里返回默认名称作为占位
                Some(format!("func_{}", func_idx))
            }
        })
    }
}

/// Phase 4.3: 模块依赖描述（跨模块调用支持）
#[derive(Debug, Clone)]
pub struct ModuleDependency {
    /// 依赖的模块名称
    pub name: String,
    /// 需要从该模块导入的函数名列表
    pub imports: Vec<String>,
    /// 依赖模块已加载时的模块 ID（None = 未解析）
    pub resolved_module_id: Option<u32>,
}

/// Phase 4.3: 跨模块符号条目
#[derive(Debug, Clone)]
pub struct CrossModuleSymbol {
    /// 函数名
    pub name: String,
    /// 所属模块 ID
    pub module_id: u32,
    /// 函数在分发表中的索引
    pub func_idx: usize,
}

/// AOT runtime: manages multiple AOT modules and a global dispatch table
pub struct AotRuntime {
    modules: HashMap<u32, AotModule>,
    next_module_id: u32,
    /// Phase 3.5: AOT 入口查找缓存 (func_idx -> module_id)
    entry_cache: HashMap<usize, u32>,
    /// Phase 4.3: 跨模块符号表 (func_name -> CrossModuleSymbol)
    cross_module_symbols: HashMap<String, CrossModuleSymbol>,
    /// Phase 4.3: 模块依赖表 (module_id -> dependencies)
    module_dependencies: HashMap<u32, Vec<ModuleDependency>>,
    /// Phase 4.5: 共享库句柄表 (module_id -> 动态库引用)
    /// 使用 `Box<dyn Send + Sync>` 类型擦除，避免 `#[cfg]` 在结构体字段上。
    /// 仅 `load_shared_library`（`dynamic-ffi` feature）填充此表。
    shared_lib_handles: HashMap<u32, Box<dyn std::any::Any + Send + Sync>>,
    /// extern interface: 函数名 → func_idx 映射 (module_id -> {func_name -> func_idx})
    func_name_map: HashMap<u32, HashMap<String, usize>>,
}

impl AotRuntime {
    pub fn new() -> Self {
        AotRuntime {
            modules: HashMap::new(),
            next_module_id: 1,
            entry_cache: HashMap::new(),
            cross_module_symbols: HashMap::new(),
            module_dependencies: HashMap::new(),
            shared_lib_handles: HashMap::new(),
            func_name_map: HashMap::new(),
        }
    }

    /// Load an AOT module from a `.auc` file
    pub fn load_module(&mut self, auc_path: &Path) -> Result<u32, String> {
        let path = match auc_path.to_str() {
            Some(s) => s.to_string(),
            None => return Err("invalid path".to_string()),
        };
        let bytes = std::fs::read(&path).map_err(|e| format!("read {} failed: {}", path, e))?;
        let module = crate::codegen::serialize::from_bytes(&bytes).map_err(|e| e.to_string())?;
        if !module.has_aot() {
            return Err("module has no AOT segments".to_string());
        }
        let desc_idx: Vec<u32> = module.functions.iter().map(|f| f.aot_desc_idx).collect();
        let name = if module.module_identity.name.is_empty() {
            "aot_module".to_string()
        } else {
            module.module_identity.name.clone()
        };
        self.load_module_from(&module.aot_blob_data, &module.aot_segments, &desc_idx, name)
    }

    /// Load an AOT module from a segment data region
    pub fn load_module_from(
        &mut self,
        data: &[u8],
        segments: &[AucSegment],
        func_desc_idx: &[u32],
        name: String,
    ) -> Result<u32, String> {
        let module_id = self.next_module_id;
        let aot = AotModule::load(data, segments, func_desc_idx, module_id, name)?;
        self.modules.insert(module_id, aot);
        self.next_module_id = self.next_module_id.saturating_add(1);
        Ok(module_id)
    }

    /// Phase 4.5: 从动态库加载 AOT 模块（Tier 2 共享库）
    ///
    /// dlopen 动态库 → 用 `object` crate 解析符号表 → 枚举 `aura_aot_*` 导出符号
    /// → dlsym 获取函数地址 → 构建 `AotModule`。
    /// 动态库句柄存储在 `shared_lib_handles` 中以防止被 OS 卸载。
    ///
    /// 需要 `llvm` + `dynamic-ffi` 两个 feature。
    ///
    /// 返回模块 ID，调用者可用 `call_func(module_id, func_idx, args)` 调用。
    /// `func_idx` = 符号在枚举结果中的索引。
    #[cfg(all(feature = "llvm", feature = "dynamic-ffi"))]
    pub fn load_shared_library(&mut self, lib_path: &str) -> Result<u32, String> {
        use object::Object;
        use object::read::File as ObjectFile;

        // 1. dlopen
        let lib = unsafe {
            libloading::Library::new(lib_path)
                .map_err(|e| format!("dlopen 失败 {}: {}", lib_path, e))?
        };

        // 2. 用 object crate 解析共享库的导出符号
        //    PE DLL 使用导出表（export table），不是 COFF 符号表；
        //    ELF/Mach-O 使用符号表（symbols()）。
        let bytes =
            std::fs::read(lib_path).map_err(|e| format!("读取动态库 {} 失败: {}", lib_path, e))?;
        let file = ObjectFile::parse(&bytes[..])
            .map_err(|e| format!("解析动态库 {} 失败: {}", lib_path, e))?;

        // 3. 收集所有 aura_aot_* 符号名
        let mut symbol_names: Vec<String> = Vec::new();
        match &file {
            ObjectFile::Pe32(pe_file) => {
                if let Some(export_table) =
                    pe_file.export_table().map_err(|e| format!("解析导出表失败: {}", e))?
                {
                    for export in
                        export_table.exports().map_err(|e| format!("读取导出表失败: {}", e))?
                    {
                        if let Some(name_bytes) = export.name {
                            let name = String::from_utf8_lossy(name_bytes).to_string();
                            if name.starts_with("aura_aot_") {
                                symbol_names.push(name);
                            }
                        }
                    }
                }
            }
            ObjectFile::Pe64(pe_file) => {
                if let Some(export_table) =
                    pe_file.export_table().map_err(|e| format!("解析导出表失败: {}", e))?
                {
                    for export in
                        export_table.exports().map_err(|e| format!("读取导出表失败: {}", e))?
                    {
                        if let Some(name_bytes) = export.name {
                            let name = String::from_utf8_lossy(name_bytes).to_string();
                            if name.starts_with("aura_aot_") {
                                symbol_names.push(name);
                            }
                        }
                    }
                }
            }
            _ => {
                // ELF/Mach-O/COFF：通过符号表枚举
                use object::read::ObjectSymbol;
                for symbol in file.symbols() {
                    if symbol.is_undefined() {
                        continue;
                    }
                    if let Ok(name) = symbol.name() {
                        if name.starts_with("aura_aot_") {
                            symbol_names.push(name.to_string());
                        }
                    }
                }
            }
        }

        if symbol_names.is_empty() {
            return Err(format!(
                "动态库 {} 中未找到 aura_aot_* 导出符号（请确认编译时使用 --shared 生成）",
                lib_path
            ));
        }

        // 4. 为每个符号获取函数指针
        let mut symbols: Vec<(String, AotEntry, u8, u8, u8)> = Vec::new();
        for name in &symbol_names {
            // 解析元数据
            let meta = parse_aot_symbol_name(name);
            let (_, nargs, ret_tag, arg_tags_vec) = match meta {
                Some(m) => m,
                None => continue,
            };
            let arg_tags_u8 = compute_arg_tags(&arg_tags_vec);

            // dlsym 获取函数地址
            let sym = unsafe {
                lib.get::<fn()>(name.as_bytes())
                    .map_err(|e| format!("dlsym 失败 {}: {}", name, e))?
            };
            // Symbol<T> implements Deref<Target = T>, dereference to get fn() then cast
            let entry_fn: fn() = unsafe { std::mem::transmute(*sym) };
            let addr: usize = entry_fn as usize;
            let entry = unsafe { std::mem::transmute::<usize, AotEntry>(addr) };

            symbols.push((name.clone(), entry, nargs, ret_tag, arg_tags_u8));
        }

        // 5. 从文件名提取模块名
        let file_name = std::path::Path::new(lib_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| lib_path.to_string());
        let stem = std::path::Path::new(&file_name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(file_name);

        // 6. 构建 AotModule
        let module_id = self.next_module_id;
        let aot = AotModule::from_symbols(symbols.clone(), module_id, stem.clone());
        self.modules.insert(module_id, aot);

        // 6.5 填充函数名 → func_idx 映射（extern interface 支持）
        for (func_idx, (name, _, _, _, _)) in symbols.iter().enumerate() {
            if let Some((func_name, _, _, _)) = parse_aot_symbol_name(name) {
                self.func_name_map.entry(module_id).or_default().insert(func_name, func_idx);
            }
        }

        // 7. 存储库句柄（防止被 OS 卸载）
        self.shared_lib_handles.insert(module_id, Box::new(lib));

        self.next_module_id = self.next_module_id.saturating_add(1);
        Ok(module_id)
    }

    /// 获取共享库导出的函数数量（Phase 4.5）
    #[cfg(all(feature = "llvm", feature = "dynamic-ffi"))]
    pub fn shared_lib_func_count(&self, module_id: u32) -> usize {
        self.modules.get(&module_id).map(|m| m.func_descriptors.len()).unwrap_or(0)
    }

    /// Unload a module (unmmap the machine code, drop shared library handle)
    pub fn unload_module(&mut self, module_id: u32) -> bool {
        if let Some(m) = self.modules.get_mut(&module_id) {
            m.unload();
        }
        self.shared_lib_handles.remove(&module_id);
        self.modules.remove(&module_id).is_some()
    }

    /// Invoke a function in an AOT module
    pub unsafe fn call_func(
        &mut self,
        module_id: u32,
        func_idx: usize,
        args: &[JitValue],
    ) -> Result<JitValue, String> {
        let module = self
            .modules
            .get_mut(&module_id)
            .ok_or_else(|| format!("module {} not loaded", module_id))?;
        let entry = module
            .find_entry(func_idx)
            .ok_or_else(|| format!("function {} not found in AOT dispatch table", func_idx))?;
        let mut ret = JitValue::null();
        let mut ctx = AotCallContext {
            module_id,
            func_idx: func_idx as u32,
            call_depth: 1,
            ..AotCallContext::new()
        };
        let ctx_ptr: *const () = &ctx as *const AotCallContext as *const ();
        let args_ptr = if args.is_empty() { std::ptr::null() } else { args.as_ptr() };
        entry(args_ptr, &mut ret, args.len(), ctx_ptr);
        // Phase 2.6: 检查异常码（AOT 函数通过 ctx.exception 返回异常状态）
        if ctx.exception != 0 {
            return Err(format!(
                "AOT function '{}' (module={}, func_idx={}) raised exception code {}",
                module.name, module_id, func_idx, ctx.exception
            ));
        }
        Ok(ret)
    }

    pub fn has_module(&self, module_id: u32) -> bool {
        self.modules.contains_key(&module_id)
    }

    /// 按名称查找函数索引（extern interface 支持）
    pub fn lookup_func_idx(&self, module_id: u32, func_name: &str) -> Option<usize> {
        self.func_name_map.get(&module_id)?.get(func_name).copied()
    }

    /// Whether any module has an AOT entry for `func_idx`
    pub fn has_entry(&self, func_idx: usize) -> bool {
        self.modules.values().any(|m| m.find_entry(func_idx).is_some())
    }

    /// Invoke by function index, searching all loaded modules
    pub unsafe fn call_func_by_idx(
        &mut self,
        func_idx: usize,
        args: &[JitValue],
    ) -> Option<JitValue> {
        let ids: Vec<u32> = self.modules.keys().copied().collect();
        for id in ids {
            let has = match self.modules.get(&id) {
                Some(m) => m.find_entry(func_idx).is_some(),
                None => false,
            };
            if has {
                return unsafe { self.call_func(id, func_idx, args) }.ok();
            }
        }
        None
    }

    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    pub fn get_module(&self, module_id: u32) -> Option<&AotModule> {
        self.modules.get(&module_id)
    }

    // ── Phase 3.3: 热重载 API（设计文档 §3.3）──

    /// 热重载模块：卸载旧版本并加载新版本，不重启进程
    ///
    /// 返回新的模块 ID。卸载失败时返回错误。
    /// 调用者负责提供新版本数据。
    pub fn hot_reload_module(
        &mut self,
        old_module_id: u32,
        new_data: &[u8],
        segments: &[AucSegment],
        func_desc_idx: &[u32],
        name: String,
    ) -> Result<u32, String> {
        // 1. 验证旧模块存在
        if !self.modules.contains_key(&old_module_id) {
            return Err(format!("hot_reload: module {} not loaded", old_module_id));
        }

        // 2. 卸载旧模块
        let old = self.modules.remove(&old_module_id);
        if let Some(mut m) = old {
            m.unload();
        }

        // 3. 加载新版本
        self.load_module_from(new_data, segments, func_desc_idx, name)
    }

    /// 列出所有已加载模块的 ID 和名称
    pub fn list_modules(&self) -> Vec<(u32, &str)> {
        self.modules.iter().map(|(&id, m)| (id, m.name.as_str())).collect()
    }

    // ── Phase 3.2: 模块沙箱（设计文档 §3.2）──

    /// 检查调用深度是否超过限制
    pub fn check_call_depth(&self, depth: u32) -> bool {
        depth <= AOT_MAX_CALL_DEPTH
    }

    /// 当前最大调用深度限制
    pub fn max_call_depth() -> u32 {
        AOT_MAX_CALL_DEPTH
    }

    // ── Phase 3.5: AOT 入口查找缓存（设计文档 §3.5）──

    /// 获取缓存的 AOT 入口（避免每次调用都遍历模块表）
    pub fn cached_find_entry(&mut self, func_idx: usize) -> Option<(u32, AotEntry)> {
        // 先查缓存
        if let Some(&cached_id) = self.entry_cache.get(&func_idx) {
            if let Some(m) = self.modules.get(&cached_id) {
                if let Some(entry) = m.find_entry(func_idx) {
                    return Some((cached_id, entry));
                }
            }
            // 缓存失效，清除
            self.entry_cache.remove(&func_idx);
        }
        // 遍历查找并缓存
        for (id, m) in &self.modules {
            if let Some(entry) = m.find_entry(func_idx) {
                self.entry_cache.insert(func_idx, *id);
                return Some((*id, entry));
            }
        }
        None
    }

    /// 清除 AOT 入口缓存（模块卸载/重载后调用）
    pub fn clear_entry_cache(&mut self) {
        self.entry_cache.clear();
    }

    // ── Phase 3.6: 详细错误报告（设计文档 §3.6）──

    /// 获取模块诊断信息
    pub fn module_diagnostics(&self, module_id: u32) -> Option<ModuleDiagnostics> {
        self.modules.get(&module_id).map(|m| ModuleDiagnostics {
            module_id,
            name: m.name.clone(),
            is_loaded: m.is_loaded(),
            code_base: m.code_base(),
            func_count: m.func_descriptors.len(),
            dispatch_count: m.dispatch_table.iter().filter(|e| e.is_some()).count(),
            entry_offsets: m.func_descriptors.iter().map(|d| d.entry_offset).collect(),
        })
    }

    /// 获取所有模块诊断信息
    pub fn all_diagnostics(&self) -> Vec<ModuleDiagnostics> {
        self.modules.iter().map(|(&id, _)| self.module_diagnostics(id).unwrap()).collect()
    }
}

// ==================== Phase 4.3: 跨模块调用 ====================

impl AotRuntime {
    /// 注册模块依赖关系
    pub fn register_module_dependency(&mut self, module_id: u32, dependency: ModuleDependency) {
        self.module_dependencies.entry(module_id).or_default().push(dependency);
    }

    /// 解析所有模块依赖
    pub fn resolve_dependencies(&mut self) -> (usize, usize) {
        let mut resolved = 0;
        let mut unresolved = 0;
        let mut available_funcs: HashMap<String, (u32, usize)> = HashMap::new();
        for (&id, module) in &self.modules {
            for (idx, desc) in module.func_descriptors.iter().enumerate() {
                let name = module.resolve_func_name(idx);
                if let Some(name) = name {
                    available_funcs.insert(name, (id, idx));
                }
            }
        }
        let module_ids: Vec<u32> = self.module_dependencies.keys().copied().collect();
        for mod_id in module_ids {
            if let Some(deps) = self.module_dependencies.get_mut(&mod_id) {
                for dep in deps.iter_mut() {
                    if dep.resolved_module_id.is_some() {
                        continue;
                    }
                    let dep_module_id = self
                        .modules
                        .iter()
                        .find_map(|(&id, m)| if m.name == dep.name { Some(id) } else { None });
                    if let Some(dep_id) = dep_module_id {
                        dep.resolved_module_id = Some(dep_id);
                        for func_name in &dep.imports {
                            if let Some(&(src_id, src_idx)) = available_funcs.get(func_name) {
                                self.cross_module_symbols.insert(
                                    func_name.clone(),
                                    CrossModuleSymbol {
                                        name: func_name.clone(),
                                        module_id: src_id,
                                        func_idx: src_idx,
                                    },
                                );
                                resolved += 1;
                            } else {
                                unresolved += 1;
                            }
                        }
                    } else {
                        unresolved += dep.imports.len();
                    }
                }
            }
        }
        (resolved, unresolved)
    }

    /// 按函数名查找跨模块符号
    pub fn lookup_cross_module_symbol(&self, func_name: &str) -> Option<&CrossModuleSymbol> {
        self.cross_module_symbols.get(func_name)
    }

    /// 获取模块的依赖列表
    pub fn get_module_dependencies(&self, module_id: u32) -> Option<&[ModuleDependency]> {
        self.module_dependencies.get(&module_id).map(|v| v.as_slice())
    }

    /// 获取已解析的跨模块符号数量
    pub fn cross_module_symbol_count(&self) -> usize {
        self.cross_module_symbols.len()
    }
}

// ==================== Phase 4.5: 共享库符号解析辅助 ====================

/// 从 `aura_aot_<name>!<nargs>!<rettag>!<tag0>!<tag1>!...` 解析元数据
fn parse_aot_symbol_name(name: &str) -> Option<(String, u8, u8, Vec<u8>)> {
    if !name.starts_with("aura_aot_") {
        return None;
    }
    let parts: Vec<&str> = name.split('!').collect();
    if parts.len() < 3 {
        return None;
    }
    let func_part = parts[0]["aura_aot_".len()..].to_string();
    let nargs: u8 = parts[1].parse().ok()?;
    let rettag: u8 = parts[2].parse().ok()?;
    let arg_tags: Vec<u8> = parts[3..].iter().filter_map(|s| s.parse().ok()).collect();
    Some((func_part, nargs, rettag, arg_tags))
}

/// 将参数类型标签列表编码为 AuraFuncDesc.arg_tags (u8)
fn compute_arg_tags(arg_tags: &[u8]) -> u8 {
    if arg_tags.len() <= 2 {
        let t0 = arg_tags.first().copied().unwrap_or(0) & 0x0F;
        let t1 = arg_tags.get(1).copied().unwrap_or(0) & 0x0F;
        (t0 << 4) | t1
    } else {
        (arg_tags.len() as u8) & 0x0F
    }
}

// ==================== Phase 4.5: 插件系统 ====================

/// Phase 4.5: 插件信息
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// 插件名称
    pub name: String,
    /// 插件路径（动态库路径）
    pub path: String,
    /// 插件版本
    pub version: String,
    /// 插件导出函数数量
    pub func_count: usize,
}

/// Phase 4.5: 插件管理器
///
/// 管理第三方 AOT 模块的加载、卸载和发现。
/// 支持通过动态库（`.so`/`.dylib`/`.dll`）加载插件。
pub struct PluginManager {
    /// 已加载插件列表
    plugins: Vec<PluginInfo>,
    /// 插件加载目录（搜索路径）
    search_paths: Vec<String>,
    /// 关联的 AOT 运行时
    runtime: AotRuntime,
}

impl PluginManager {
    /// 创建新的插件管理器
    pub fn new() -> Self {
        PluginManager {
            plugins: Vec::new(),
            search_paths: Vec::new(),
            runtime: AotRuntime::new(),
        }
    }

    /// 添加插件搜索路径
    pub fn add_search_path(&mut self, path: String) {
        self.search_paths.push(path);
    }

    /// 获取搜索路径列表
    pub fn search_paths(&self) -> &[String] {
        &self.search_paths
    }

    /// 加载插件（从动态库文件）
    ///
    /// 支持 `.auc` 文件格式的插件，以及 Tier 2 动态库格式（`.dll`/`.so`/`.dylib`）。
    /// 动态库格式需要 `dynamic-ffi` feature。
    pub fn load_plugin(&mut self, path: &str) -> Result<u32, String> {
        // Phase 4.5: 共享库路径（Tier 2 动态库）
        if path.ends_with(".dll") || path.ends_with(".so") || path.ends_with(".dylib") {
            #[cfg(all(feature = "llvm", feature = "dynamic-ffi"))]
            {
                let module_id = self.runtime.load_shared_library(path)?;
                let func_count = self.runtime.shared_lib_func_count(module_id);
                let stem = std::path::Path::new(path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string());
                let info = PluginInfo {
                    name: stem,
                    path: path.to_string(),
                    version: String::new(),
                    func_count,
                };
                self.plugins.push(info);
                return Ok(module_id);
            }
            #[cfg(not(all(feature = "llvm", feature = "dynamic-ffi")))]
            {
                return Err(format!(
                    "共享库加载需要启用 llvm + dynamic-ffi feature。\n\
                     请使用: cargo run --features llvm,dynamic-ffi\n\
                     然后调用 AotRuntime::load_shared_library(\"{}\")。",
                    path
                ));
            }
        }

        let plugin_info = self.load_plugin_from_path(path)?;
        let module_id = self.runtime.load_module(std::path::Path::new(path))?;
        self.plugins.push(plugin_info);
        Ok(module_id)
    }

    /// 从路径加载插件信息
    fn load_plugin_from_path(&self, path: &str) -> Result<PluginInfo, String> {
        // 尝试读取 .auc 文件获取插件元信息
        if path.ends_with(".auc") {
            let bytes =
                std::fs::read(path).map_err(|e| format!("读取插件 {} 失败: {}", path, e))?;
            let module = crate::codegen::serialize::from_bytes(&bytes)
                .map_err(|e| format!("解析插件 {} 失败: {}", path, e))?;

            Ok(PluginInfo {
                name: module.module_identity.name.clone(),
                path: path.to_string(),
                version: module.module_identity.version.clone(),
                func_count: module.functions.len(),
            })
        } else if path.ends_with(".dll") || path.ends_with(".so") || path.ends_with(".dylib") {
            // Phase 4.1: 共享库路径（Tier 2 动态库）。
            // PluginManager 当前仅支持 .auc 格式的插件加载。
            // 共享库加载请使用 DynamicLoader（启用 dynamic-ffi feature）：
            //   DynamicLoader::load_lib(path, FfiAbi::C)
            // 然后通过 dlsym 获取 aura_aot_* 符号（JitValue ABI 包装函数）。
            // PluginManager 的完整共享库支持（含符号枚举与 AotModule 构建）留待后续阶段。
            Err(format!(
                "共享库加载尚未实现：PluginManager 当前仅支持 .auc 格式。\n\
                 请使用 DynamicLoader（启用 dynamic-ffi feature）加载动态库：\n\
                   DynamicLoader::load_lib(\"{}\", FfiAbi::C)\n\
                 然后通过 dlsym 查找 aura_aot_* 符号（JitValue ABI 包装函数）。",
                path
            ))
        } else {
            // 动态库路径，提取文件名作为插件名
            let file_name = std::path::Path::new(path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            let stem = std::path::Path::new(&file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or(file_name);

            Ok(PluginInfo {
                name: stem,
                path: path.to_string(),
                version: String::new(),
                func_count: 0,
            })
        }
    }

    /// 卸载插件
    pub fn unload_plugin(&mut self, plugin_name: &str) -> bool {
        // 找到插件对应的模块 ID 并卸载
        let plugin_idx = self.plugins.iter().position(|p| p.name == plugin_name);
        if let Some(idx) = plugin_idx {
            // 遍历运行时模块找到匹配的模块 ID
            let module_ids: Vec<u32> =
                self.runtime.all_diagnostics().iter().map(|d| d.module_id).collect();
            for id in module_ids {
                if let Some(diag) = self.runtime.module_diagnostics(id) {
                    if diag.name == plugin_name {
                        self.runtime.unload_module(id);
                        self.plugins.remove(idx);
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 列出所有已加载插件
    pub fn list_plugins(&self) -> &[PluginInfo] {
        &self.plugins
    }

    /// 获取插件数量
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// 获取关联的 AOT 运行时（用于调用插件函数）
    pub fn runtime(&self) -> &AotRuntime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut AotRuntime {
        &mut self.runtime
    }

    /// 发现插件（扫描搜索路径中的 `.auc` 文件）
    pub fn discover_plugins(&self) -> Vec<String> {
        let mut found = Vec::new();
        for search_path in &self.search_paths {
            if let Ok(entries) = std::fs::read_dir(search_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "auc").unwrap_or(false) {
                        found.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
        found
    }

    /// 加载所有发现的插件
    pub fn load_all_discovered(&mut self) -> Vec<Result<u32, String>> {
        let plugins = self.discover_plugins();
        plugins.iter().map(|p| self.load_plugin(p)).collect()
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::opcode::{
        AucSegment, AuraFuncDesc, FUNC_EXPORT, SEG_DESC_TABLE, SEG_MACHINE, SEG_PROT_EXEC,
        SEG_PROT_READ,
    };

    fn expect_err(res: Result<AotModule, String>) -> String {
        match res {
            Err(e) => e,
            Ok(_) => panic!("expected an error, got a loaded module"),
        }
    }

    /// Build a segment data region (machine segment first, then descriptor table)
    fn make_segment_data(
        machine_size: usize,
        descs: &[AuraFuncDesc],
    ) -> (Vec<u8>, Vec<AucSegment>) {
        let mut data = Vec::new();
        data.resize(machine_size, 0x90);
        let machine = AucSegment {
            id: SEG_MACHINE,
            offset: 0,
            size: machine_size as u32,
            flags: SEG_PROT_READ | SEG_PROT_EXEC,
        };
        let mut desc_bytes = Vec::new();
        for d in descs {
            let slice = unsafe {
                std::slice::from_raw_parts(
                    d as *const AuraFuncDesc as *const u8,
                    AuraFuncDesc::SIZE,
                )
            };
            desc_bytes.extend_from_slice(slice);
        }
        let desc_seg = AucSegment {
            id: SEG_DESC_TABLE,
            offset: data.len() as u32,
            size: desc_bytes.len() as u32,
            flags: SEG_PROT_READ,
        };
        data.extend_from_slice(&desc_bytes);
        (
            data,
            vec![
                machine, desc_seg,
            ],
        )
    }

    #[test]
    fn test_func_desc_size_is_32() {
        assert_eq!(AuraFuncDesc::SIZE, 32);
    }

    #[test]
    fn test_parse_segments_rejects_bad_size() {
        let err = AotModule::parse_segments(&[0u8; 31]).unwrap_err();
        assert!(err.contains("multiple"));
    }

    #[test]
    fn test_load_builds_dispatch_table() {
        let descs = vec![
            AuraFuncDesc {
                entry_offset: 0x100,
                num_args: 1,
                flags: FUNC_EXPORT,
                ..AuraFuncDesc::default()
            },
            AuraFuncDesc {
                entry_offset: 0x200,
                num_args: 2,
                flags: FUNC_EXPORT,
                ..AuraFuncDesc::default()
            },
            AuraFuncDesc {
                entry_offset: 4,
                ..AuraFuncDesc::default()
            },
        ];
        let (data, segs) = make_segment_data(512, &descs);
        let func_desc_idx = vec![
            1u32, 2, 0, 3,
        ];
        let mut m =
            AotModule::load(&data, &segs, &func_desc_idx, 7, "unit-test".to_string()).unwrap();
        assert!(m.is_loaded());
        assert_eq!(m.func_descriptors.len(), 3);
        assert!(m.find_entry(0).is_some(), "func 0 -> desc #1");
        assert!(m.find_entry(1).is_some(), "func 1 -> desc #2");
        assert!(
            m.find_entry(2).is_none(),
            "func 2 -> desc idx 0 means no AOT"
        );
        assert!(
            m.find_entry(3).is_none(),
            "entry_offset=4 is not 16-byte aligned"
        );
        assert!(m.find_entry(4).is_none(), "index out of range");
        assert!(m.code_base() != 0);
        m.unload();
        assert!(!m.is_loaded());
        assert!(m.find_entry(0).is_none());
        assert_eq!(m.code_base(), 0);
    }

    #[test]
    fn test_load_rejects_missing_desc_segment() {
        let segs = vec![
            AucSegment {
                id: SEG_MACHINE,
                offset: 0,
                size: 256,
                flags: SEG_PROT_READ | SEG_PROT_EXEC,
            },
        ];
        let data = vec![0u8; 256];
        let err = expect_err(AotModule::load(&data, &segs, &[], 1, "x".to_string()));
        assert!(err.contains("DESC_TABLE"));
    }

    #[test]
    fn test_load_rejects_empty_machine_size() {
        // 机器码段大小为 0 必须报错
        let segs = vec![
            AucSegment {
                id: SEG_MACHINE,
                offset: 0,
                size: 0,
                flags: SEG_PROT_READ | SEG_PROT_EXEC,
            },
            AucSegment {
                id: SEG_DESC_TABLE,
                offset: 0,
                size: 0,
                flags: SEG_PROT_READ,
            },
        ];
        let err = expect_err(AotModule::load(&[], &segs, &[], 1, "x".to_string()));
        assert!(err.contains("empty"));
    }

    #[test]
    fn test_load_rejects_segment_out_of_range() {
        let descs = vec![
            AuraFuncDesc {
                entry_offset: 0x100,
                ..AuraFuncDesc::default()
            },
        ];
        let (data, mut segs) = make_segment_data(256, &descs);
        segs[0].offset = 100;
        let err = expect_err(AotModule::load(&data, &segs, &[1u32], 1, "x".to_string()));
        assert!(err.contains("out of range"));
    }

    #[test]
    fn test_runtime_load_unload() {
        let descs = vec![
            AuraFuncDesc {
                entry_offset: 0x100,
                ..AuraFuncDesc::default()
            },
        ];
        let (data, segs) = make_segment_data(256, &descs);
        let mut rt = AotRuntime::new();
        assert_eq!(rt.module_count(), 0);
        assert!(!rt.has_entry(0));
        let id1 = rt.load_module_from(&data, &segs, &[1u32], "m1".to_string()).unwrap();
        let id2 = rt.load_module_from(&data, &segs, &[1u32], "m2".to_string()).unwrap();
        assert_ne!(id1, id2);
        assert_eq!(rt.module_count(), 2);
        assert!(rt.has_module(id1));
        assert!(rt.has_entry(0));
        assert!(rt.get_module(id1).is_some());
        assert!(rt.unload_module(id1));
        assert!(!rt.unload_module(id1));
        assert!(!rt.has_module(id1));
        assert_eq!(rt.module_count(), 1);
        assert!(rt.unload_module(id2));
        assert!(rt.get_module(id2).is_none());
    }
}
