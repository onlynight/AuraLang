//! 源码映射基础设施（P0.6 — SourceMap）
//!
//! 职责：
//! - 注册源文件并缓存每行的起始字节偏移
//! - 字节偏移 ↔ (行, 列) 双向换算（列按“字符数”计，与 `Span` 语义一致）
//! - 为诊断信息渲染带源码行与波浪线指示的片段
//!
//! `Span` 本身已内嵌行列信息（词法/语法阶段直接写入），`SourceMap` 负责
//! 在多文件场景下由字节偏移重新解析位置，并输出人类可读的源码片段。

use crate::Span;

/// 源文件句柄（在 `SourceMap` 中的下标包装）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(usize);

/// 单个已注册的源文件
#[derive(Debug, Clone)]
pub struct SourceFile {
    name: String,
    source: String,
    /// 每行起始字节偏移；`line_starts[0] == 0`
    line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        let source = source.into();
        let mut line_starts = vec![0usize];
        for (idx, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(idx + 1);
            }
        }
        Self {
            name: name.into(),
            source,
            line_starts,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// 第 `line` 行（1-based）的起始字节偏移
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line.checked_sub(1)?).copied()
    }

    /// 第 `line` 行（1-based）的文本，不含行尾换行符
    pub fn line(&self, line: usize) -> Option<&str> {
        let start = self.line_start(line)?;
        let end = self.source[start..]
            .find('\n')
            .map(|offset| start + offset)
            .unwrap_or(self.source.len());
        let text = &self.source[start..end];
        Some(text.strip_suffix('\r').unwrap_or(text))
    }

    /// 字节偏移 → 行号（1-based）
    pub fn line_index(&self, byte_offset: usize) -> usize {
        match self.line_starts.binary_search(&byte_offset) {
            Ok(idx) => idx + 1,
            Err(idx) => idx.max(1),
        }
    }

    /// 字节偏移 → (行, 列)（均 1-based，列按字符数计）
    pub fn location(&self, byte_offset: usize) -> (usize, usize) {
        let line = self.line_index(byte_offset);
        let start = self.line_start(line).unwrap_or(0);
        let offset = byte_offset.min(self.source.len()).max(start);
        let col = self.source[start..offset].chars().count() + 1;
        (line, col)
    }
}

/// 多文件源码映射表
#[derive(Debug, Default, Clone)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// 注册一个源文件，返回句柄
    pub fn add_file(&mut self, name: impl Into<String>, source: impl Into<String>) -> FileId {
        let id = FileId(self.files.len());
        self.files.push(SourceFile::new(name, source));
        id
    }

    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0]
    }

    pub fn source(&self, id: FileId) -> &str {
        &self.files[id.0].source
    }

    /// 字节偏移 → (行, 列)
    pub fn location(&self, id: FileId, byte_offset: usize) -> (usize, usize) {
        self.files[id.0].location(byte_offset)
    }

    /// `Span` → ((起始行, 起始列), (结束行, 结束列))，全部由字节偏移重新推算
    pub fn span_location(&self, id: FileId, span: &Span) -> ((usize, usize), (usize, usize)) {
        (self.location(id, span.start), self.location(id, span.end))
    }

    /// 渲染带源码行与波浪线的片段（替代仅有行列号的单薄输出）。
    ///
    /// 支持多行 `Span`：起始行从 `start_col` 下划到行尾，中间行整行下划（`~`），
    /// 结束行从行首下划到 `end_col`。
    pub fn snippet(&self, id: FileId, span: &Span) -> String {
        let file = self.file(id);
        let start_line = span.start_line.max(1);
        let end_line = span.end_line.max(start_line);
        let last_line = end_line.min(file.line_count());
        let gutter = last_line.to_string().len().max(1);

        let mut out = String::new();
        for line in start_line..=last_line {
            let text = file.line(line).unwrap_or("");
            out.push_str(&format!("{:>width$} | {}\n", line, text, width = gutter));

            // 计算该行下划线的起始列（0-based）与宽度（按字符数计）
            let (pad, width, marker) = if start_line == end_line {
                // 单行：start_col → end_col
                let pad = span.start_col.saturating_sub(1).min(text.chars().count());
                let width = span.end_col.saturating_sub(span.start_col).max(1);
                (pad, width, '^')
            } else if line == start_line {
                // 起始行：start_col → 行尾
                let pad = span.start_col.saturating_sub(1).min(text.chars().count());
                let width = text.chars().count().saturating_sub(pad).max(1);
                (pad, width, '^')
            } else if line == end_line {
                // 结束行：行首 → end_col
                let width = span
                    .end_col
                    .saturating_sub(1)
                    .max(1)
                    .min(text.chars().count());
                (0, width, '^')
            } else {
                // 中间行：整行
                (0, text.chars().count().max(1), '~')
            };

            out.push_str(&format!(
                "{:>width$} | {}{}\n",
                "",
                " ".repeat(pad),
                marker.to_string().repeat(width),
                width = gutter
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (SourceMap, FileId) {
        let mut sm = SourceMap::new();
        let id = sm.add_file("main.aura", "val x = 1\nval y = \"hi\"\n");
        (sm, id)
    }

    #[test]
    fn test_line_starts_and_count() {
        let (sm, id) = sample();
        assert_eq!(sm.file(id).line_count(), 3);
        assert_eq!(sm.file(id).line(1), Some("val x = 1"));
        assert_eq!(sm.file(id).line(2), Some("val y = \"hi\""));
    }

    #[test]
    fn test_location_mapping() {
        let (sm, id) = sample();
        assert_eq!(sm.location(id, 0), (1, 1));
        assert_eq!(sm.location(id, 4), (1, 5));
        // 第二行第 9 个字符（"hi" 的起始引号）
        assert_eq!(sm.location(id, 10 + 8), (2, 9));
    }

    #[test]
    fn test_span_location() {
        let (sm, id) = sample();
        let span = Span {
            start: 4,
            end: 9,
            start_line: 1,
            start_col: 5,
            end_line: 1,
            end_col: 10,
        };
        assert_eq!(sm.span_location(id, &span), ((1, 5), (1, 10)));
    }

    #[test]
    fn test_snippet_rendering() {
        let (sm, id) = sample();
        let span = Span {
            start: 4,
            end: 5,
            start_line: 1,
            start_col: 5,
            end_line: 1,
            end_col: 6,
        };
        let out = sm.snippet(id, &span);
        assert!(out.contains("val x = 1"));
        assert!(out.contains('^'));
    }

    #[test]
    fn test_multi_line_snippet() {
        let mut sm = SourceMap::new();
        let id = sm.add_file("m.aura", "fun f() {\n    return 1\n}\n");
        let span = Span {
            start: 0,
            end: 22,
            start_line: 1,
            start_col: 1,
            end_line: 3,
            end_col: 2,
        };
        let out = sm.snippet(id, &span);
        assert!(out.contains("fun f() {"));
        assert!(out.contains("    return 1"));
        assert!(out.contains('}'));
    }

    #[test]
    fn test_crlf_line_text() {
        let sm = {
            let mut sm = SourceMap::new();
            sm.add_file("crlf.aura", "a\r\nb\r\n");
            sm
        };
        let id = FileId(0);
        assert_eq!(sm.file(id).line(1), Some("a"));
    }

    #[test]
    fn test_multi_line_snippet_underline() {
        // 跨行 span：第 1 行 start_col→行尾（^），第 2 行整行（~），第 3 行 行首→end_col（^）
        let mut sm = SourceMap::new();
        let id = sm.add_file("m.aura", "val a = 1\nval b = 2\nval c = 3\n");
        // span 覆盖第 1 行第 5 列 到 第 3 行第 5 列
        let span = Span {
            start: 4,
            end: 24,
            start_line: 1,
            start_col: 5,
            end_line: 3,
            end_col: 5,
        };
        let out = sm.snippet(id, &span);
        let lines: Vec<&str> = out.lines().collect();
        // 结构：源码行 / 下划线行 交替
        // 第 1 行源码
        assert!(lines[0].contains("val a = 1"));
        // 第 1 行下划线：^ 从第 5 列起（pad=4 空格）
        assert!(lines[1].ends_with("    ^^^^^"));
        // 第 2 行源码
        assert!(lines[2].contains("val b = 2"));
        // 第 2 行整行 ~
        assert!(lines[3].ends_with("~~~~~~"));
        // 第 3 行源码
        assert!(lines[4].contains("val c = 3"));
        // 第 3 行下划线：^ 行首到 第 5 列
        assert!(lines[5].ends_with("^^^^"));
    }
}
