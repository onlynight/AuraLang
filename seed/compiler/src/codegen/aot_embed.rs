//! AOT 机器码嵌入（Phase 1）
//!
//! 将 AOT 编译产物（`.text` 机器码 + 函数描述符）嵌入 [`BytecodeModule`]，
//! 由 `.auc` v4 段表承载，供 [`AotRuntime`](crate::vm::aot_runtime::AotRuntime)
//! 在 VM 加载时 mmap 执行。对应 docs/AOT机器码嵌入方案-详细设计.md §3.5 / §6.2。
//!
//! VM 的 `do_call` / `CallAot` 会优先查 AOT 分发表，即使字节码仍是 `OP_CALL`
//! 会由 [`AotRuntime`](crate::vm::aot_runtime::AotRuntime) 自动派发到机器码。
//!
//! Phase 1 只嵌入 Int/Float/Bool/Unit 签名的函数；String / 闭包 / 集合等
//! 留待 Phase 2。
use std::path::Path;

use super::aot::types::sanitizellvm;
use super::aot::{AotCodeGenerator, AotError, AotOptions, OutputFormat};
use super::hir::HirProgram;
use super::opcode::{
    AucSegment, AuraFuncDesc, BytecodeModule, SEG_DESC_TABLE, SEG_MACHINE, SEG_PROT_EXEC,
    SEG_PROT_READ, SEG_STRING_POOL,
};

/// AOT 嵌入结果
pub struct AotEmbedResult {
    /// 嵌入后的字节码模块（已置 aot_segments / aot_blob_data / aot_mode）
    pub module: BytecodeModule,
    /// 机器码段大小（字节）
    pub machine_size: usize,
    /// 函数描述符数量
    pub desc_count: usize,
}
/// 将 AOT 机器码嵌入字节码模块，返回可直接序列化为 `.auc` v4 的模块。
///
/// `module`：字节码编译产物；`hir`：同源 HIR（用于生成 AOT IR）。
/// 内部在 `work_dir` 生成临时 `.ll` / `.o` / `.blob` 后自动清理。
/// 失败时调用者可回退到纯字节码输出。
pub fn embed_aot(
    module: BytecodeModule,
    hir: &HirProgram,
    options: AotOptions,
    work_dir: &Path,
) -> Result<AotEmbedResult, AotError> {
    if let Err(e) = std::fs::create_dir_all(work_dir) {
        return Err(AotError::Io(format!(
            "failed to create temp directory {}: {}",
            work_dir.display(),
            e
        )));
    }
    // ── 1. AOT 编译：HIR → LLVM IR → .o → 机器码 blob + 描述符 ──
    let generator = AotCodeGenerator::new(options);
    let output = generator.compile(hir, work_dir, OutputFormat::Blob)?;
    let blob_path = output.blob_path.ok_or_else(|| {
        AotError::ToolError("AOT compilation did not produce a blob file".to_string())
    })?;

    let machine_code = std::fs::read(&blob_path).map_err(|e| {
        AotError::Io(format!(
            "failed to read machine code blob {}: {}",
            blob_path.display(),
            e
        ))
    })?;
    let descs = output.descriptors;
    let _ = std::fs::remove_dir_all(work_dir);
    if machine_code.is_empty() {
        return Err(AotError::ToolError(
            "machine code blob is empty".to_string(),
        ));
    }

    // ── 2. 按名匹配：为命中包装函数的 BytecodeFunction 置 AOT 标记 ──
    let mut module = module;
    let mut matched = 0usize;
    for (name, desc) in &descs {
        if desc.entry_offset == 0 {
            continue;
        }
        let san = sanitizellvm(name);
        for f in module.functions.iter_mut() {
            if sanitizellvm(&f.name) == san {
                f.aot_mode = 1;
                f.aot_desc_idx = matched as u32 + 1;
                break;
            }
        }
        matched += 1;
    }
    if matched == 0 {
        return Err(AotError::ToolError(
            "no match between AOT descriptors and bytecode functions (Phase 1 only supports Int/Float/Bool/Unit signatures)".to_string(),
        ));
    }
    // ── 3. 组装段表 + 段数据区 ──
    let (segments, blob_data, machine_size) = assemble_segments(&machine_code, &descs);

    Ok(AotEmbedResult {
        module: BytecodeModule {
            aot_segments: segments,
            aot_blob_data: blob_data,
            header_flags: module.compute_header_flags(),
            ..module
        },
        machine_size,
        desc_count: descs.len(),
    })
}

/// 组装段表与段数据区（SEG_MACHINE + SEG_DESC_TABLE + SEG_STRING_POOL）
///
/// 返回 `(段表, 段数据区, 机器码大小)`。
/// 机器码段被填充到 16 字节对齐，描述符表紧随其后，字符串池在最后。
/// 字符串池包含函数名（null 终止），描述符的 name_offset/name_len 指向字符串池。
fn assemble_segments(
    machine_code: &[u8],
    descs: &[(String, AuraFuncDesc)],
) -> (Vec<AucSegment>, Vec<u8>, usize) {
    let machine_size = machine_code.len();
    let aligned_machine_size = (machine_size + 15) / 16 * 16;

    // ── Phase 2.9: 构建字符串池（设计文档 §5.5）──
    // 收集所有唯一函数名
    let mut unique_names: Vec<String> = Vec::new();
    let mut name_to_idx: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (name, _) in descs {
        if !name_to_idx.contains_key(name) {
            name_to_idx.insert(name.clone(), unique_names.len());
            unique_names.push(name.clone());
        }
    }

    // 构建字符串池数据：先偏移表，后数据区
    let mut string_pool = Vec::new();
    let offset_table_size = unique_names.len() * 4;
    string_pool.resize(offset_table_size, 0); // 偏移表占位
    let data_start = offset_table_size;

    let mut name_offsets: Vec<u32> = Vec::with_capacity(unique_names.len());
    for (i, name) in unique_names.iter().enumerate() {
        let offset = data_start + name_offsets.iter().map(|o| *o as usize + 1).sum::<usize>();
        name_offsets.push(offset as u32);
        string_pool.extend_from_slice(name.as_bytes());
        string_pool.push(0); // null 终止
    }

    // 写入偏移表
    for (i, off) in name_offsets.iter().enumerate() {
        let pos = i * 4;
        string_pool[pos..pos + 4].copy_from_slice(&off.to_le_bytes());
    }

    // 更新描述符的 name_offset / name_len
    let mut descs_with_names: Vec<(String, AuraFuncDesc)> = Vec::with_capacity(descs.len());
    for (name, d) in descs {
        let idx = *name_to_idx.get(name).unwrap_or(&0);
        let mut d = *d;
        d.name_offset = name_offsets[idx];
        d.name_len = name.len() as u16;
        descs_with_names.push((name.clone(), d));
    }

    // 序列化描述符表
    let mut desc_bytes = Vec::with_capacity(descs_with_names.len() * AuraFuncDesc::SIZE);
    for (_, d) in &descs_with_names {
        let slice = unsafe {
            std::slice::from_raw_parts(d as *const AuraFuncDesc as *const u8, AuraFuncDesc::SIZE)
        };
        desc_bytes.extend_from_slice(slice);
    }

    // 组装段数据区: machine_code (16 对齐) + desc_table + string_pool
    let mut blob_data =
        Vec::with_capacity(aligned_machine_size + desc_bytes.len() + string_pool.len());
    blob_data.extend_from_slice(machine_code);
    blob_data.resize(aligned_machine_size, 0);
    blob_data.extend_from_slice(&desc_bytes);
    blob_data.extend_from_slice(&string_pool);

    let desc_offset = aligned_machine_size as u32;
    let pool_offset = desc_offset + desc_bytes.len() as u32;

    let segments = vec![
        AucSegment {
            id: SEG_MACHINE,
            offset: 0,
            size: machine_size as u32,
            flags: SEG_PROT_READ | SEG_PROT_EXEC,
        },
        AucSegment {
            id: SEG_DESC_TABLE,
            offset: desc_offset,
            size: desc_bytes.len() as u32,
            flags: SEG_PROT_READ,
        },
        AucSegment {
            id: SEG_STRING_POOL,
            offset: pool_offset,
            size: string_pool.len() as u32,
            flags: SEG_PROT_READ,
        },
    ];
    (segments, blob_data, machine_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_descs() -> Vec<(String, AuraFuncDesc)> {
        vec![
            (
                "add".to_string(),
                AuraFuncDesc {
                    entry_offset: 0x40,
                    num_args: 2,
                    return_tag: 0,
                    flags: 1,
                    ..AuraFuncDesc::default()
                },
            ),
            (
                "neg".to_string(),
                AuraFuncDesc {
                    entry_offset: 0x80,
                    num_args: 1,
                    return_tag: 0,
                    flags: 1,
                    ..AuraFuncDesc::default()
                },
            ),
        ]
    }

    #[test]
    fn test_assemble_segments_layout() {
        let code: Vec<u8> = (0..48u8).collect();
        let descs = sample_descs();
        let (segs, blob, msize) = assemble_segments(&code, &descs);
        assert_eq!(msize, 48);
        // blob = 48 (machine, 16-aligned) + 64 (desc) + string_pool
        let expected_machine = (48 + 15) / 16 * 16;
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].id, SEG_MACHINE);
        assert_eq!(segs[0].size, 48);
        assert!(segs[0].is_exec());
        assert_eq!(segs[1].id, SEG_DESC_TABLE);
        assert_eq!(segs[1].offset, expected_machine as u32);
        assert_eq!(segs[1].size, 64);
        assert!(!segs[1].is_exec());
        assert_eq!(segs[2].id, SEG_STRING_POOL);
        // 描述符表按原始字节可读回
        let d0 = unsafe {
            std::ptr::read_unaligned(blob[expected_machine..].as_ptr() as *const AuraFuncDesc)
        };
        assert_eq!(d0.entry_offset, 0x40);
        assert_eq!(d0.num_args, 2);
        // 描述符的 name_offset 指向字符串池
        assert!(d0.name_offset > 0);
    }

    #[test]
    fn test_assemble_segments_pads_machine_code() {
        let code: Vec<u8> = vec![0xCC; 20]; // 非 16 对齐
        let (segs, blob, _) = assemble_segments(&code, &[]);
        assert_eq!(segs[0].size, 20);
        assert_eq!(segs[0].is_exec(), true);
        assert_eq!(segs[1].size, 0);
        assert_eq!(blob.len(), 32);
    }

    #[test]
    fn test_assemble_segments_consumable_by_aot_runtime() {
        let code: Vec<u8> = vec![0x90; 256];
        let mut descs = sample_descs();
        descs[0].1.entry_offset = 0x100;
        descs[1].1.entry_offset = 0x200;
        let (segs, blob, _) = assemble_segments(&code, &descs);
        let mut m = crate::vm::aot_runtime::AotModule::load(
            &blob,
            &segs,
            &[1u32, 2, 0],
            9,
            "embed-test".to_string(),
        )
        .unwrap();
        assert!(m.find_entry(0).is_some());
        assert!(m.find_entry(1).is_some());
        assert!(m.find_entry(2).is_none());
        m.unload();
    }
}
