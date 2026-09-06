/// Source location tracking — records byte offset and (line, column) for error diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl Span {
    pub fn single(byte_offset: usize, line: usize, col: usize) -> Self {
        Self {
            start: byte_offset,
            end: byte_offset,
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
        }
    }

    pub fn empty(byte_offset: usize, line: usize, col: usize) -> Self {
        Self {
            start: byte_offset,
            end: byte_offset,
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
        }
    }

    pub fn merge(a: &Span, b: &Span) -> Self {
        Self {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
            start_line: if a.start <= b.start { a.start_line } else { b.start_line },
            start_col: if a.start <= b.start { a.start_col } else { b.start_col },
            end_line: if a.end >= b.end { a.end_line } else { b.end_line },
            end_col: if a.end >= b.end { a.end_col } else { b.end_col },
        }
    }

    pub fn length(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start_line == self.end_line {
            write!(
                f,
                "line {} col {}-{}",
                self.start_line, self.start_col, self.end_col
            )
        } else {
            write!(
                f,
                "line {} col {} to line {} col {}",
                self.start_line, self.start_col, self.end_line, self.end_col
            )
        }
    }
}
