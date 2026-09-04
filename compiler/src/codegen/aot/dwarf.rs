//! DWARF 调试信息（基础）
//!
//! 对应 技术方案 §9.5 DWARF 调试信息生成。
//!
//! LLVM 通过 `!DIFile` / `!DISubprogram` / `!DILocation` 等元数据描述调试信息。
//! 通过 `-g` 参数可以让 LLVM 自动生成 DWARF；这里我们提供 Aura 侧的基础
//! 元数据（源码文件、函数、行号），插入到生成的 LLVM IR 中。

use std::path::Path;

use crate::codegen::hir::HirFunction;

/// 调试信息元数据（简化版）
#[derive(Debug)]
pub struct DebugInfo {
    /// 源码文件名
    pub source_file: String,
    /// 源码目录
    pub source_dir: String,
    /// 函数符号（用于 DISubprogram）
    pub subprograms: Vec<String>,
    /// 是否启用
    pub enabled: bool,
}

impl DebugInfo {
    pub fn new(source_file: impl AsRef<Path>) -> Self {
        let path = source_file.as_ref();
        let source_file = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "main.aura".to_string());
        let source_dir = path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        Self {
            source_file,
            source_dir,
            subprograms: Vec::new(),
            enabled: true,
        }
    }

    /// 添加函数符号
    pub fn add_subprogram(&mut self, name: &str) {
        if !self.subprograms.contains(&name.to_string()) {
            self.subprograms.push(name.to_string());
        }
    }
}

/// 生成 LLVM IR 中的 debug_info 元数据段
///
/// 简化实现：直接输出模块级元数据，函数级元数据在生成函数时单独插入。
/// 生成合法的 `!DICompileUnit` / `!DIFile` / `!DISubprogram` 实体——
/// 函数体通过 `!dbg !N` 指令级元数据关联（见 [`emit_program`] 的
/// `subprogram_index` 传递）。
pub fn emit_debug_metadata(di: &DebugInfo) -> String {
    if !di.enabled {
        return String::new();
    }

    let mut s = String::new();
    s.push_str("; ---- Debug Info ----\n");
    // DICompileUnit 引用 !0（DIFile）；subprogram 元数据由 emit_subprogram
    // 以 `!N` 形式追加到本段末尾（由调用方拼接）。
    s.push_str(&format!(
        "!0 = !DIFile(filename: \"{}\", directory: \"{}\")\n",
        di.source_file, di.source_dir
    ));
    s.push_str(&format!(
        "!1 = distinct !DICompileUnit(language: DW_LANG_C99, file: !0, isOptimized: false, producer: \"Aura AOT\")\n"
    ));
    // 模块级 flags：启用 DWARF 版本（对应 llvm -g 行为）
    s.push_str("!llvm.module.flags = !{!2, !3}\n");
    s.push_str("!2 = !{i32 2, !\"Dwarf Version\", i32 4}\n");
    s.push_str("!3 = !{i32 2, !\"Debug Info Version\", i32 3}\n");
    // 编译单元注册表（llc 校验：DICompileUnit 必须列入 llvm.dbg.cu）
    s.push_str("!llvm.dbg.cu = !{!1}\n");
    s
}

/// 生成函数级的 DISubprogram 元数据实体（`!N = !DISubprogram(...)`）
///
/// 返回 `(sub_id, loc_id, text)`：`sub_id` 供 DILocation 引用，
/// `loc_id` 供函数体 `!dbg !N` 引用；`text` 是需要拼入模块元数据段的实体定义。
pub fn emit_subprogram(
    di: &DebugInfo,
    func: &HirFunction,
    line: u32,
    index: u32,
) -> (u32, u32, String) {
    let sub_id = 10 + index * 2;
    let loc_id = sub_id + 1;
    let text = if di.enabled {
        format!(
            "!{sub_id} = distinct !DISubprogram(name: \"{name}\", linkageName: \"{name}\", isOptimized: false, line: {line}, scope: !0, file: !0, type: !4, spFlags: DISPFlagDefinition, unit: !1)\n!{loc_id} = !DILocation(line: {line}, column: 0, scope: !{sub_id})\n",
            sub_id = sub_id,
            loc_id = loc_id,
            name = func.name,
            line = line,
        )
    } else {
        String::new()
    };
    (sub_id, loc_id, text)
}

/// 生成一个最小函数类型元数据实体（DISubroutineType，供 DISubprogram.type 引用）
pub fn emit_subroutine_type() -> String {
    // !4 由 emit_subprogram 引用；此处提供最小合法定义（返回 void）
    "!4 = !DISubroutineType(types: !5)\n!5 = !{}\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_info_new() {
        let di = DebugInfo::new("src/main.aura");
        assert!(di.enabled);
        assert_eq!(di.source_file, "main.aura");
    }

    #[test]
    fn test_emit_metadata() {
        let di = DebugInfo::new("src/main.aura");
        let ir = emit_debug_metadata(&di);
        assert!(ir.contains("DICompileUnit"));
        assert!(ir.contains("main.aura"));
    }

    #[test]
    fn test_disabled() {
        let mut di = DebugInfo::new("src/main.aura");
        di.enabled = false;
        assert_eq!(emit_debug_metadata(&di), "");
    }
}
