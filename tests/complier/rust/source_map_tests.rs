//! SourceMap unit tests — multi-file mapping, byte offset conversion, snippet rendering

use compiler::Span;
use compiler::source_map::{FileId, SourceFile, SourceMap};

// ─── SourceFile ─────────────────────────────────────────────────────────────

#[test]
fn test_source_file_line_count() {
    let sf = SourceFile::new("main.aura", "val x = 1\nval y = 2\n");
    assert_eq!(sf.line_count(), 3);
}

#[test]
fn test_source_file_single_line() {
    let sf = SourceFile::new("main.aura", "val x = 1");
    assert_eq!(sf.line_count(), 1);
}

#[test]
fn test_source_file_empty() {
    let sf = SourceFile::new("main.aura", "");
    assert_eq!(sf.line_count(), 1);
}

#[test]
fn test_source_file_trailing_newline() {
    let sf = SourceFile::new("main.aura", "val x = 1\n");
    assert_eq!(sf.line_count(), 2);
}

#[test]
fn test_source_file_line_content() {
    let sf = SourceFile::new("main.aura", "val x = 1\nval y = \"hi\"\n");
    assert_eq!(sf.line(1), Some("val x = 1"));
    assert_eq!(sf.line(2), Some("val y = \"hi\""));
}

#[test]
fn test_source_file_line_out_of_range() {
    let sf = SourceFile::new("main.aura", "val x = 1");
    assert_eq!(sf.line(0), None);
    assert_eq!(sf.line(2), None);
}

#[test]
fn test_source_file_line_start() {
    let sf = SourceFile::new("main.aura", "val x = 1\nval y = 2");
    assert_eq!(sf.line_start(1), Some(0));
    assert_eq!(sf.line_start(2), Some(10));
}

#[test]
fn test_source_file_line_start_out_of_range() {
    let sf = SourceFile::new("main.aura", "val x = 1");
    assert_eq!(sf.line_start(0), None);
    assert_eq!(sf.line_start(2), None);
}

// ─── SourceFile: location mapping ──────────────────────────────────────────

#[test]
fn test_source_file_location_beginning() {
    let sf = SourceFile::new("main.aura", "val x = 1\nval y = 2");
    assert_eq!(sf.location(0), (1, 1));
}

#[test]
fn test_source_file_location_mid_line() {
    let sf = SourceFile::new("main.aura", "val x = 1\nval y = 2");
    // byte 4 is 'x' in "val x"
    assert_eq!(sf.location(4), (1, 5));
}

#[test]
fn test_source_file_location_second_line() {
    let sf = SourceFile::new("main.aura", "val x = 1\nval y = 2");
    // byte 10 is start of second line
    assert_eq!(sf.location(10), (2, 1));
}

#[test]
fn test_source_file_location_end() {
    let sf = SourceFile::new("main.aura", "val x = 1\nval y = 2");
    // byte 19 is the end
    assert_eq!(sf.location(19), (2, 10));
}

#[test]
fn test_source_file_location_beyond_end() {
    let sf = SourceFile::new("main.aura", "val x = 1");
    // Beyond end of source, should clamp
    let (line, col) = sf.location(100);
    assert_eq!(line, 1);
}

#[test]
fn test_source_file_line_index() {
    let sf = SourceFile::new("main.aura", "val x = 1\nval y = 2");
    assert_eq!(sf.line_index(0), 1);
    assert_eq!(sf.line_index(4), 1);
    assert_eq!(sf.line_index(10), 2);
}

// ─── SourceFile: CRLF handling ──────────────────────────────────────────────

#[test]
fn test_source_file_crlf_line_text() {
    let sf = SourceFile::new("crlf.aura", "a\r\nb\r\n");
    assert_eq!(sf.line(1), Some("a"));
    assert_eq!(sf.line(2), Some("b"));
}

// ─── SourceMap: multi-file ──────────────────────────────────────────────────

#[test]
fn test_source_map_add_file() {
    let mut sm = SourceMap::new();
    let id1 = sm.add_file("main.aura", "val x = 1");
    let id2 = sm.add_file("util.aura", "fun add(a: Int, b: Int): Int { return a + b }");

    assert_eq!(sm.file(id1).name(), "main.aura");
    assert_eq!(sm.file(id2).name(), "util.aura");
}

#[test]
fn test_source_map_file_ids_unique() {
    let mut sm = SourceMap::new();
    let id1 = sm.add_file("a.aura", "a");
    let id2 = sm.add_file("b.aura", "b");
    assert_ne!(id1, id2);
}

#[test]
fn test_source_map_source() {
    let mut sm = SourceMap::new();
    let id = sm.add_file("main.aura", "val x = 1\nval y = 2");
    assert_eq!(sm.source(id), "val x = 1\nval y = 2");
}

#[test]
fn test_source_map_location() {
    let mut sm = SourceMap::new();
    let id = sm.add_file("main.aura", "val x = 1\nval y = 2");
    assert_eq!(sm.location(id, 0), (1, 1));
    assert_eq!(sm.location(id, 10), (2, 1));
}

// ─── SourceMap: span_location ──────────────────────────────────────────────

#[test]
fn test_source_map_span_location_single_line() {
    let mut sm = SourceMap::new();
    let id = sm.add_file("main.aura", "val x = 1\nval y = 2");
    let span = Span {
        start: 4,
        end: 5,
        start_line: 1,
        start_col: 5,
        end_line: 1,
        end_col: 6,
    };
    assert_eq!(sm.span_location(id, &span), ((1, 5), (1, 6)));
}

#[test]
fn test_source_map_span_location_multi_line() {
    let mut sm = SourceMap::new();
    let id = sm.add_file("main.aura", "val x = 1\nval y = 2");
    let span = Span {
        start: 0,
        end: 15,
        start_line: 1,
        start_col: 1,
        end_line: 2,
        end_col: 6,
    };
    assert_eq!(sm.span_location(id, &span), ((1, 1), (2, 6)));
}

// ─── SourceMap: snippet rendering ──────────────────────────────────────────

#[test]
fn test_snippet_single_line() {
    let mut sm = SourceMap::new();
    let id = sm.add_file("main.aura", "val x = 1\nval y = 2\n");
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
fn test_snippet_multi_line() {
    let mut sm = SourceMap::new();
    let id = sm.add_file("main.aura", "fun f() {\n    return 1\n}\n");
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
    assert!(out.contains("return 1"));
    assert!(out.contains('}'));
}

#[test]
fn test_snippet_with_underline_markers() {
    // Multi-line span: start line has ^, middle lines have ~, end line has ^
    let mut sm = SourceMap::new();
    let id = sm.add_file("m.aura", "val a = 1\nval b = 2\nval c = 3\n");
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

    // Should have 6 lines: 3 source lines + 3 underline lines
    assert!(lines.len() >= 6);

    // First source line
    assert!(lines[0].contains("val a = 1"));
    // First underline: ^ starting at col 5
    assert!(lines[1].contains('^'));

    // Second source line
    assert!(lines[2].contains("val b = 2"));
    // Second underline: ~ for middle line
    assert!(lines[3].contains('~'));

    // Third source line
    assert!(lines[4].contains("val c = 3"));
    // Third underline: ^
    assert!(lines[5].contains('^'));
}

#[test]
fn test_snippet_with_line_numbers() {
    let mut sm = SourceMap::new();
    let id = sm.add_file("main.aura", "line one\nline two\nline three\n");
    let span = Span {
        start: 0,
        end: 30,
        start_line: 1,
        start_col: 1,
        end_line: 3,
        end_col: 10,
    };
    let out = sm.snippet(id, &span);
    // Should contain line numbers
    assert!(out.contains("1 |"));
    assert!(out.contains("2 |"));
    assert!(out.contains("3 |"));
}

// ─── Span helpers ──────────────────────────────────────────────────────────

#[test]
fn test_span_single() {
    let s = Span::single(5, 1, 5);
    assert_eq!(s.start, 5);
    assert_eq!(s.end, 5);
    assert_eq!(s.start_line, 1);
    assert_eq!(s.start_col, 5);
    assert_eq!(s.end_line, 1);
    assert_eq!(s.end_col, 5);
}

#[test]
fn test_span_empty() {
    let s = Span::empty(0, 1, 1);
    assert!(s.is_empty());
    assert_eq!(s.length(), 0);
}

#[test]
fn test_span_merge() {
    let a = Span {
        start: 0,
        end: 5,
        start_line: 1,
        start_col: 1,
        end_line: 1,
        end_col: 6,
    };
    let b = Span {
        start: 10,
        end: 15,
        start_line: 2,
        start_col: 1,
        end_line: 2,
        end_col: 6,
    };
    let merged = Span::merge(&a, &b);
    assert_eq!(merged.start, 0);
    assert_eq!(merged.end, 15);
    assert_eq!(merged.start_line, 1);
    assert_eq!(merged.end_line, 2);
}

#[test]
fn test_span_length() {
    let s = Span {
        start: 5,
        end: 10,
        start_line: 1,
        start_col: 1,
        end_line: 1,
        end_col: 6,
    };
    assert_eq!(s.length(), 5);
}

#[test]
fn test_span_display_single_line() {
    let s = Span {
        start: 0,
        end: 5,
        start_line: 1,
        start_col: 1,
        end_line: 1,
        end_col: 6,
    };
    assert_eq!(s.to_string(), "line 1 col 1-6");
}

#[test]
fn test_span_display_multi_line() {
    let s = Span {
        start: 0,
        end: 15,
        start_line: 1,
        start_col: 1,
        end_line: 2,
        end_col: 6,
    };
    assert_eq!(s.to_string(), "line 1 col 1 to line 2 col 6");
}

// ─── SourceFile: name and source ───────────────────────────────────────────

#[test]
fn test_source_file_name() {
    let sf = SourceFile::new("test.aura", "content");
    assert_eq!(sf.name(), "test.aura");
}

#[test]
fn test_source_file_source() {
    let sf = SourceFile::new("test.aura", "val x = 1");
    assert_eq!(sf.source(), "val x = 1");
}
