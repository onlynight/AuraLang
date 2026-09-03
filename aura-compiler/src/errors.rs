use crate::Span;
use crate::source_map::{FileId, SourceMap};

/// Compilation error with source location.
#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub span: Span,
    pub severity: ErrorSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
}

impl CompileError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            severity: ErrorSeverity::Error,
        }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            severity: ErrorSeverity::Warning,
        }
    }

    pub fn spanned(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            severity: ErrorSeverity::Error,
        }
    }

    pub fn emit(&self) {
        eprintln!("Aura {} [{}]: {}", self.severity, self.span, self.message);
    }

    /// 带源码片段的诊断输出（配合 `SourceMap` 使用）
    ///
    /// ```text
    /// error: type mismatch: cannot initialize 'Int' with 'String'
    ///  --> main.aura:2:5
    ///    |
    ///  2 | val y: Int = "hi"
    ///    |     ^^^
    /// ```
    pub fn render(&self, sm: &SourceMap, file: FileId) -> String {
        let header = format!(
            "{}: {}\n --> {}:{}:{}",
            match self.severity {
                ErrorSeverity::Error => "error",
                ErrorSeverity::Warning => "warning",
                ErrorSeverity::Info => "info",
            },
            self.message,
            sm.file(file).name(),
            self.span.start_line,
            self.span.start_col,
        );
        format!("{}\n{}", header, sm.snippet(file, &self.span))
    }

    /// 带源码片段的诊断输出（无 `SourceMap` 时退化为行列信息）
    pub fn render_plain(&self) -> String {
        self.emit_string()
    }

    fn emit_string(&self) -> String {
        format!("{} [{}]: {}", self.severity, self.span, self.message)
    }
}

impl std::fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorSeverity::Error => write!(f, "ERROR"),
            ErrorSeverity::Warning => write!(f, "WARNING"),
            ErrorSeverity::Info => write!(f, "INFO"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Span;
    use crate::source_map::SourceMap;

    fn sample_map() -> (SourceMap, FileId) {
        let mut sm = SourceMap::new();
        let id = sm.add_file("main.aura", "fun f() {\n    val x: Int = \"oops\"\n}\n");
        (sm, id)
    }

    #[test]
    fn test_render_includes_message_and_location() {
        let (sm, id) = sample_map();
        let err = CompileError::new(
            "type mismatch: cannot initialize 'Int' with 'String'",
            Span {
                start: 18,
                end: 35,
                start_line: 2,
                start_col: 17,
                end_line: 2,
                end_col: 34,
            },
        );
        let out = err.render(&sm, id);
        assert!(out.contains("error:"));
        assert!(out.contains("type mismatch: cannot initialize 'Int' with 'String'"));
        assert!(out.contains("main.aura:2:17"));
        // 源码片段
        assert!(out.contains("val x: Int = \"oops\""));
        // 波浪线指示
        assert!(out.contains('^'));
    }

    #[test]
    fn test_render_warning_severity() {
        let (sm, id) = sample_map();
        let err = CompileError::warning(
            "unused variable",
            Span {
                start: 18,
                end: 19,
                start_line: 2,
                start_col: 17,
                end_line: 2,
                end_col: 18,
            },
        );
        let out = err.render(&sm, id);
        assert!(out.contains("warning:"));
        assert!(out.contains("unused variable"));
    }

    #[test]
    fn test_render_plain_fallback() {
        let err = CompileError::new("boom", Span::single(0, 1, 1));
        let out = err.render_plain();
        assert!(out.contains("boom"));
        assert!(out.contains("line 1 col 1"));
    }
}
