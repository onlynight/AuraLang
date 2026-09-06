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

use crate::codegen::opcode::{AucSegment, AuraFuncDesc, SEG_DESC_TABLE, SEG_MACHINE};
use crate::vm::abi::{AotCallContext, AotEntry, JitValue};
use crate::vm::mmap_util::{MappedRegion, MemoryProtection};

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
}

/// AOT runtime: manages multiple AOT modules and a global dispatch table
pub struct AotRuntime {
    modules: HashMap<u32, AotModule>,
    next_module_id: u32,
}

impl AotRuntime {
    pub fn new() -> Self {
        AotRuntime {
            modules: HashMap::new(),
            next_module_id: 1,
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

    /// Unload a module (unmmap the machine code)
    pub fn unload_module(&mut self, module_id: u32) -> bool {
        if let Some(m) = self.modules.get_mut(&module_id) {
            m.unload();
        }
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
        let ctx = AotCallContext {
            module_id,
            func_idx: func_idx as u32,
            call_depth: 1,
            ..AotCallContext::new()
        };
        let ctx_ptr: *const () = &ctx as *const AotCallContext as *const ();
        let args_ptr = if args.is_empty() { std::ptr::null() } else { args.as_ptr() };
        entry(args_ptr, &mut ret, args.len(), ctx_ptr);
        Ok(ret)
    }

    pub fn has_module(&self, module_id: u32) -> bool {
        self.modules.contains_key(&module_id)
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
                return self.call_func(id, func_idx, args).ok();
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
