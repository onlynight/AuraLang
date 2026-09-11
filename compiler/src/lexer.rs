//! 词法分析器（Lexer）
//!
//! 将源码字符串转换为 Token 流。手写实现，覆盖 Aura 语言所有语法元素：
//! - 关键字、标识符
//! - 整数 / 浮点字面量（十进制、十六进制、二进制、带下划线分隔符）
//! - 字符串字面量（含插值 `$var` / `${expr}`）
//! - 单引号字符字面量、布尔字面量
//! - 单行 `//` 和多行 `/* */` 注释（含文档注释）
//! - 所有运算符（含复合赋值、比较、逻辑、位运算、空安全操作符）
//! - 分隔符
//!
//! 设计要点：
//! - 单遍扫描 O(n)
//! - 错误恢复：遇到非法字符时生成 Error token，继续扫描后续内容
//! - 字符串插值时递归调用 next_token 来解析 `${expr}` 内部

use crate::span::Span;
use crate::token::{Token, TokenKind, TokenizeError};

// ─────────────────────────────────────────────────────────────────────────────
// 关键字表
// ─────────────────────────────────────────────────────────────────────────────

const KEYWORDS: &[(&str, TokenKind)] = &[
    ("val", TokenKind::Val),
    ("var", TokenKind::Var),
    ("fun", TokenKind::Fun),
    ("struct", TokenKind::Struct),
    ("class", TokenKind::Class),
    ("interface", TokenKind::Interface),
    ("enum", TokenKind::Enum),
    ("actor", TokenKind::Actor),
    ("sealed", TokenKind::Sealed),
    ("return", TokenKind::Return),
    ("if", TokenKind::If),
    ("else", TokenKind::Else),
    ("when", TokenKind::When),
    ("for", TokenKind::For),
    ("in", TokenKind::In),
    ("while", TokenKind::While),
    ("do", TokenKind::Do),
    ("break", TokenKind::Break),
    ("continue", TokenKind::Continue),
    ("try", TokenKind::Try),
    ("catch", TokenKind::Catch),
    ("finally", TokenKind::Finally),
    ("throw", TokenKind::Throw),
    ("is", TokenKind::Is),
    ("as", TokenKind::As),
    ("public", TokenKind::Public),
    ("private", TokenKind::Private),
    ("protected", TokenKind::Protected),
    ("null", TokenKind::Null),
    ("this", TokenKind::This),
    ("super", TokenKind::Super),
    ("object", TokenKind::Object),
    ("unit", TokenKind::Unit),
    ("typealias", TokenKind::Typealias),
    ("override", TokenKind::Override),
    ("suspend", TokenKind::Suspend),
    ("inline", TokenKind::Inline),
    ("const", TokenKind::Const),
    ("comptime", TokenKind::Comptime),
    ("defer", TokenKind::Defer),
    ("extern", TokenKind::Extern),
    ("import", TokenKind::Import),
    ("lazy", TokenKind::Lazy),
    ("lateinit", TokenKind::Lateinit),
    ("data", TokenKind::Data),
    ("value", TokenKind::Value),
    ("await", TokenKind::Await),
    ("to", TokenKind::To),
    // P10 并发运行时关键字（async 和 select 保留为关键字；spawn/send/ask/channel 作为普通标识符）
    ("async", TokenKind::Async),
    ("select", TokenKind::Select),
    // P7 内存管理关键字
    ("box", TokenKind::Box),
    ("weak", TokenKind::Weak),
    ("malloc", TokenKind::Malloc),
    ("free", TokenKind::Free),
    ("retain", TokenKind::Retain),
    ("release", TokenKind::Release),
    // 基本类型关键字
    ("Int", TokenKind::Ident),
    ("Long", TokenKind::Ident),
    ("Short", TokenKind::Ident),
    ("Byte", TokenKind::Ident),
    ("Float", TokenKind::Ident),
    ("Double", TokenKind::Ident),
    ("Boolean", TokenKind::Ident),
    ("Char", TokenKind::Ident),
    ("String", TokenKind::Ident),
    ("Any", TokenKind::Ident),
    ("Nothing", TokenKind::Ident),
    ("Unit", TokenKind::Unit),
    ("true", TokenKind::BoolLiteral),
    ("false", TokenKind::BoolLiteral),
    ("it", TokenKind::Ident),
    ("by", TokenKind::Ident),
    ("in", TokenKind::In),
];

/// 判断字符串是否为关键字，返回对应的 TokenKind
fn lookup_keyword(s: &str) -> Option<TokenKind> {
    for &(word, kind) in KEYWORDS.iter() {
        if s == word {
            return Some(kind);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Lexer 结构体
// ─────────────────────────────────────────────────────────────────────────────

pub struct Lexer {
    source: String,
    /// 当前字符迭代器（用于多字节 Unicode 处理）
    chars: Vec<(char, usize)>, // (char, byte_offset_of_char)
    char_pos: usize, // 当前在 chars 中的索引
    line: usize,
    col: usize,
    /// 缓存的 Token（用于 next_token 的延迟消费）
    peek: Option<Token>,
    /// 字符串插值产生的额外 Token 队列（StringLiteral + StringInterpStart 序列）
    pending: Vec<Token>,
    /// 错误列表
    errors: Vec<TokenizeError>,
}

impl Lexer {
    /// 从源码字符串创建 Lexer
    pub fn new(source: &str) -> Self {
        let chars: Vec<(char, usize)> = source.char_indices().map(|(idx, ch)| (ch, idx)).collect();

        Self {
            source: source.to_string(),
            chars,
            char_pos: 0,
            line: 1,
            col: 1,
            peek: None,
            pending: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// 返回所有错误
    pub fn errors(&self) -> &[TokenizeError] {
        &self.errors
    }

    /// 返回所有 Token（含 EOF）
    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            tokens.push(tok.clone());
            if tok.kind == TokenKind::EOF {
                break;
            }
        }
        tokens
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Token 生成入口
    // ─────────────────────────────────────────────────────────────────────────

    /// 获取下一个 Token（含插值递归支持）
    pub fn next_token(&mut self) -> Token {
        if let Some(tok) = self.peek.take() {
            return tok;
        }
        // 字符串插值序列中的后续 Token 优先输出
        if !self.pending.is_empty() {
            return self.pending.remove(0);
        }

        self.skip_whitespace_and_comments();

        if self.char_pos >= self.chars.len() {
            return self.eof_token();
        }

        let ch = self.chars[self.char_pos].0;
        let start_span = self.current_span();

        let tok = match ch {
            // ── 分隔符 ──
            '(' => self.delim_token(TokenKind::LParen),
            ')' => self.delim_token(TokenKind::RParen),
            '{' => self.delim_token(TokenKind::LBrace),
            '}' => self.delim_token(TokenKind::RBrace),
            '[' => self.delim_token(TokenKind::LBracket),
            ']' => self.delim_token(TokenKind::RBracket),
            ',' => self.delim_token(TokenKind::Comma),
            ';' => self.delim_token(TokenKind::Semicolon),
            ':' => self.parse_colon(),
            '.' => self.parse_dot(),
            '?' => self.parse_question(),
            '@' => self.delim_token(TokenKind::At),
            '#' => self.delim_token(TokenKind::Hash),
            '|' => self.parse_pipe(),
            '&' => self.parse_ampersand(),
            '^' => self.delim_token(TokenKind::Caret),
            '+' => self.parse_plus(),
            '-' => self.parse_minus(),
            '*' => self.parse_star(),
            '/' => self.parse_slash_or_comment(),
            '=' => self.parse_equal(),
            '!' => self.parse_bang(),
            '<' => self.parse_lt(),
            '>' => self.parse_gt(),
            '%' => self.parse_percent(),
            '$' => self.parse_dollar(start_span),
            '"' => {
                // 原始（多行）字符串："""..."""
                if self.peek_n(1) == Some('"') && self.peek_n(2) == Some('"') {
                    self.parse_raw_string(start_span)
                } else {
                    // 普通字符串可能产生多 Token（$var / ${expr} 插值序列）
                    let mut toks: Vec<Token> = Vec::new();
                    self.parse_string_tokens(start_span, &mut toks);
                    let first = toks.remove(0);
                    self.pending = toks;
                    first
                }
            }
            '\'' => self.parse_char(start_span),
            // ── 数字 ──
            c if c.is_ascii_digit() => self.parse_number(start_span),
            // ── 标识符 / 关键字 ──
            c if is_ident_start(c) => self.parse_identifier_or_keyword(start_span),
            // ── 非法字符 ──
            c => {
                self.advance();
                let span = Span::merge(&start_span, &self.current_span());
                self.errors.push(TokenizeError::new(
                    format!("Unexpected character: '{}'", c),
                    span,
                ));
                self.error_token(c.to_string(), span)
            }
        };

        tok
    }

    /// 消费一个 Token，返回它
    pub fn consume(&mut self, expected_kind: TokenKind) -> Result<Token, TokenizeError> {
        let tok = self.next_token();
        if tok.kind == expected_kind {
            Ok(tok)
        } else {
            Err(TokenizeError::new(
                format!("Expected {:?} but got {:?}", expected_kind, tok.kind),
                tok.span,
            ))
        }
    }

    /// 匹配特定 Token 类型，匹配则消耗返回 Ok，否则放回
    pub fn match_kind(&mut self, expected: TokenKind) -> bool {
        let tok = self.peek.take().unwrap_or_else(|| self.next_token());
        if tok.kind == expected {
            self.peek = None;
            true
        } else {
            self.peek = Some(tok);
            false
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 基础工具方法
    // ─────────────────────────────────────────────────────────────────────────

    fn current_span(&self) -> Span {
        if self.char_pos < self.chars.len() {
            let byte_offset = self.chars[self.char_pos].1;
            Span::single(byte_offset, self.line, self.col)
        } else {
            Span::single(self.source.len(), self.line, self.col)
        }
    }

    fn eof_span(&self) -> Span {
        Span::single(self.source.len(), self.line, self.col)
    }

    fn eof_token(&self) -> Token {
        Token::new(TokenKind::EOF, "", self.eof_span())
    }

    fn error_token(&self, msg: String, span: Span) -> Token {
        Token::new(TokenKind::Error, msg, span)
    }

    fn delim_token(&mut self, kind: TokenKind) -> Token {
        let ch = self.chars[self.char_pos].0;
        let byte_offset = self.chars[self.char_pos].1;
        let span = Span::single(byte_offset, self.line, self.col);
        self.advance();
        let literal = ch.to_string();
        Token::new(kind, literal, span)
    }

    fn advance(&mut self) {
        if self.char_pos < self.chars.len() {
            let ch = self.chars[self.char_pos].0;
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.char_pos += 1;
        }
    }

    /// 返回当前字符（不消耗）
    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.char_pos).map(|&(ch, _)| ch)
    }

    /// 向前看 N 个字符
    fn peek_n(&self, n: usize) -> Option<char> {
        self.chars.get(self.char_pos + n).map(|&(ch, _)| ch)
    }

    /// 跳过空白字符（包括注释）
    fn skip_whitespace_and_comments(&mut self) {
        while self.char_pos < self.chars.len() {
            let ch = self.chars[self.char_pos].0;
            if ch.is_whitespace() {
                if ch == '\n' {
                    self.line += 1;
                    self.col = 1;
                }
                self.char_pos += 1;
                self.col += 1;
            } else if ch == '/' {
                let next = self.peek_n(1);
                // 文档注释（/// ... 与 /** ... */）保留为 Token，交由 next_token 处理
                let is_doc_comment = (next == Some('/') && self.peek_n(2) == Some('/'))
                    || (next == Some('*')
                        && self.peek_n(2) == Some('*')
                        && self.peek_n(3) != Some('/'));
                if is_doc_comment {
                    break;
                }
                if next == Some('/') {
                    // 单行注释 — 跳到行尾
                    self.char_pos += 2;
                    self.col += 2;
                    while self.char_pos < self.chars.len() && self.chars[self.char_pos].0 != '\n' {
                        self.char_pos += 1;
                        self.col += 1;
                    }
                } else if next == Some('*') {
                    // 多行注释 /* ... */
                    let start_span = self.current_span();
                    self.char_pos += 2;
                    self.col += 2;
                    let mut found_close = false;
                    while self.char_pos < self.chars.len() {
                        let c = self.chars[self.char_pos].0;
                        if c == '\n' {
                            self.line += 1;
                            self.col = 1;
                        } else {
                            self.col += 1;
                        }
                        self.char_pos += 1;
                        if c == '*' {
                            let nn = self.peek_char();
                            if nn == Some('/') {
                                self.char_pos += 1;
                                self.col += 1;
                                found_close = true;
                                break;
                            }
                        }
                    }
                    // 如果没有找到 */，报错（span 覆盖整个注释区间）
                    if !found_close {
                        let span = Span::merge(&start_span, &self.eof_span());
                        self.errors.push(TokenizeError::new("Unterminated block comment", span));
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 数字解析
    // ─────────────────────────────────────────────────────────────────────────

    fn parse_number(&mut self, start: Span) -> Token {
        let mut buf = String::new();
        let mut is_float = false;
        let mut has_suffix = false;

        // 检测十六进制
        if self.chars[self.char_pos].0 == '0' {
            buf.push('0');
            self.advance();
            if let Some(next) = self.peek_char() {
                if next == 'x' || next == 'X' {
                    buf.push(self.chars[self.char_pos].0);
                    self.advance();
                    while let Some(ch) = self.peek_char() {
                        if is_hex_digit(ch) {
                            buf.push(ch);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    // 后缀检查
                    self.maybe_suffix(&mut buf, &mut has_suffix);
                    let span = Span::merge(&start, &self.current_span());
                    return Token::new(TokenKind::IntLiteral, buf, span);
                }
                if next == 'b' || next == 'B' {
                    buf.push(self.chars[self.char_pos].0);
                    self.advance();
                    while let Some(ch) = self.peek_char() {
                        if ch == '0' || ch == '1' {
                            buf.push(ch);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.maybe_suffix(&mut buf, &mut has_suffix);
                    let span = Span::merge(&start, &self.current_span());
                    return Token::new(TokenKind::IntLiteral, buf, span);
                }
            }
        }

        // 十进制整数部分
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() || ch == '_' {
                buf.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // 小数点？
        if self.peek_char() == Some('.') && !has_suffix {
            // 确保后面跟着数字才当作小数
            if let Some(next) = self.peek_n(1) {
                if next.is_ascii_digit() {
                    is_float = true;
                    buf.push('.');
                    self.advance();
                    while let Some(ch) = self.peek_char() {
                        if ch.is_ascii_digit() || ch == '_' {
                            buf.push(ch);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        // 科学计数法
        if let Some(ch) = self.peek_char() {
            if (ch == 'e' || ch == 'E') && !has_suffix {
                is_float = true;
                buf.push(ch);
                self.advance();
                if let Some(sign) = self.peek_char() {
                    if sign == '+' || sign == '-' {
                        buf.push(sign);
                        self.advance();
                    }
                }
                while let Some(ch) = self.peek_char() {
                    if ch.is_ascii_digit() {
                        buf.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        // 后缀：L, f, F, d, D, u, U, l
        self.maybe_suffix(&mut buf, &mut has_suffix);

        let kind = if is_float { TokenKind::FloatLiteral } else { TokenKind::IntLiteral };
        let span = Span::merge(&start, &self.current_span());
        Token::new(kind, buf, span)
    }

    fn maybe_suffix(&mut self, buf: &mut String, has_suffix: &mut bool) {
        if let Some(ch) = self.peek_char() {
            if matches!(ch, 'L' | 'l' | 'f' | 'F' | 'd' | 'D' | 'u' | 'U') && !ch.is_ascii_digit() {
                buf.push(ch);
                self.advance();
                *has_suffix = true;
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 标识符 / 关键字
    // ─────────────────────────────────────────────────────────────────────────

    fn parse_identifier_or_keyword(&mut self, start: Span) -> Token {
        let mut buf = String::new();
        let mut end_span = start; // 记录最后一个字符的位置

        while let Some(ch) = self.peek_char() {
            if is_ident_part(ch) {
                buf.push(ch);
                end_span = self.current_span(); // 捕获当前字符位置（advance 之前）
                self.advance();
            } else {
                break;
            }
        }

        let kind = lookup_keyword(&buf).unwrap_or(TokenKind::Ident);
        let span = if buf.is_empty() { start } else { Span::merge(&start, &end_span) };
        Token::new(kind, buf, span)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 字符串解析（含插值）
    // ─────────────────────────────────────────────────────────────────────────

    /// 原始（多行）字符串：`"""..."""`
    ///
    /// - 不处理转义序列（`\n` 原样保留）
    /// - 不做 `$` 插值
    /// - 允许跨行，并正确维护 line / col
    fn parse_raw_string(&mut self, start: Span) -> Token {
        // 消费三个开引号
        for _ in 0..3 {
            self.advance();
        }

        let mut buf = String::new();
        loop {
            if self.char_pos >= self.chars.len() {
                let span = Span::merge(&start, &self.current_span());
                self.errors.push(TokenizeError::new("Unterminated raw string literal", span));
                return Token::new(TokenKind::StringLiteral, buf, span);
            }

            let ch = self.chars[self.char_pos].0;
            // 结束引号："""
            if ch == '"' && self.peek_n(1) == Some('"') && self.peek_n(2) == Some('"') {
                for _ in 0..3 {
                    self.advance();
                }
                break;
            }

            buf.push(ch);
            self.advance();
        }

        let span = Span::merge(&start, &self.current_span());
        Token::new(TokenKind::StringLiteral, buf, span)
    }

    /// 解析普通字符串（含 $var / ${expr} 插值）。
    ///
    /// 输出一个 Token 序列到 `out`：
    /// - 无插值：单个 StringLiteral（可能为空字符串）
    /// - 有插值：StringLiteral(前缀) + StringInterpStart("$var" / "${expr}") + ... + StringLiteral(尾部)
    ///   解析器据此重建插值表达式（Expr::StrInterp）
    fn parse_string_tokens(&mut self, start: Span, out: &mut Vec<Token>) {
        self.advance(); // 跳过开引号 "
        let mut buf = String::new();
        let mut has_interpolation = false;

        macro_rules! flush_lit {
            () => {
                if !buf.is_empty() {
                    let span = Span::merge(&start, &self.current_span());
                    out.push(Token::new(TokenKind::StringLiteral, buf.clone(), span));
                    buf.clear();
                }
            };
        }

        loop {
            if self.char_pos >= self.chars.len() {
                let span = self.eof_span();
                self.errors.push(TokenizeError::new("Unterminated string literal", span));
                let span2 = Span::merge(&start, &self.current_span());
                out.push(Token::new(TokenKind::StringLiteral, buf, span2));
                return;
            }

            let ch = self.chars[self.char_pos].0;

            // 普通字符串不允许跨行（多行请使用 """..."""）
            if ch == '\n' {
                let span = Span::merge(&start, &self.current_span());
                self.errors.push(TokenizeError::new(
                    "Unterminated string literal: newline in string, use \"\"\"...\"\"\" for multi-line strings",
                    span,
                ));
                out.push(Token::new(TokenKind::StringLiteral, buf, span));
                return;
            }

            if ch == '"' {
                self.advance();
                break;
            }

            if ch == '\\' {
                // 转义序列
                self.advance();
                if self.char_pos < self.chars.len() {
                    let esc = self.chars[self.char_pos].0;
                    match esc {
                        'n' => buf.push('\n'),
                        't' => buf.push('\t'),
                        'r' => buf.push('\r'),
                        '\\' => buf.push('\\'),
                        '\'' => buf.push('\''),
                        '"' => buf.push('"'),
                        '0' => buf.push('\0'),
                        // JSON/通用转义：退格 / 换页 / 垂直制表
                        'b' => buf.push('\u{0008}'),
                        'f' => buf.push('\u{000C}'),
                        'v' => buf.push('\u{000B}'),
                        // JSON 允许转义斜杠，等价于普通斜杠
                        '/' => buf.push('/'),
                        _ => {
                            buf.push('\\');
                            buf.push(esc);
                            self.errors.push(TokenizeError::new(
                                format!("Invalid escape sequence: \\{}", esc),
                                self.current_span(),
                            ));
                        }
                    }
                    self.advance();
                }
            } else if ch == '$' {
                // 字符串插值 $var 或 ${expr}（查看 $ 的下一个字符）
                let next = self.peek_n(1);
                let is_interp = next == Some('{')
                    || next.map(|c| is_ident_start(c) && c != '$').unwrap_or(false);
                if is_interp {
                    flush_lit!();
                    has_interpolation = true;
                    self.advance(); // 跳过 $
                    if self.peek_char() == Some('{') {
                        self.advance(); // 跳过 {
                        let mut expr = String::new();
                        loop {
                            if self.char_pos >= self.chars.len() {
                                self.errors.push(TokenizeError::new(
                                    "Unterminated string interpolation",
                                    self.eof_span(),
                                ));
                                break;
                            }
                            let c = self.chars[self.char_pos].0;
                            if c == '}' {
                                self.advance();
                                break;
                            }
                            expr.push(c);
                            self.advance();
                        }
                        let span = Span::merge(&start, &self.current_span());
                        out.push(Token::new(
                            TokenKind::StringInterpStart,
                            format!("${{{}}}", expr),
                            span,
                        ));
                    } else {
                        // 简单插值 $varName
                        let mut name = String::new();
                        while let Some(c) = self.peek_char() {
                            if is_ident_part(c) {
                                name.push(c);
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        let span = Span::merge(&start, &self.current_span());
                        out.push(Token::new(
                            TokenKind::StringInterpStart,
                            format!("${}", name),
                            span,
                        ));
                    }
                } else {
                    // 非插值 $：字面量
                    buf.push(ch);
                    self.advance();
                }
            } else {
                buf.push(ch);
                self.advance();
            }
        }

        let span = Span::merge(&start, &self.current_span());
        // 尾部字面量；若整串无插值则始终输出一个 StringLiteral（保住 "" 空串）
        if !buf.is_empty() || !has_interpolation {
            out.push(Token::new(TokenKind::StringLiteral, buf, span));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 字符字面量
    // ─────────────────────────────────────────────────────────────────────────

    fn parse_char(&mut self, start: Span) -> Token {
        self.advance(); // 跳过开引号 '
        let mut buf = String::new();

        if self.char_pos < self.chars.len() {
            let ch = self.chars[self.char_pos].0;
            if ch == '\\' {
                self.advance();
                if self.char_pos < self.chars.len() {
                    let esc = self.chars[self.char_pos].0;
                    match esc {
                        'n' => buf.push('\n'),
                        't' => buf.push('\t'),
                        'r' => buf.push('\r'),
                        '\\' => buf.push('\\'),
                        '\'' => buf.push('\''),
                        '"' => buf.push('"'),
                        '0' => buf.push('\0'),
                        // JSON/通用转义：退格 / 换页 / 垂直制表
                        'b' => buf.push('\u{0008}'),
                        'f' => buf.push('\u{000C}'),
                        'v' => buf.push('\u{000B}'),
                        // JSON 允许转义斜杠，等价于普通斜杠
                        '/' => buf.push('/'),
                        _ => buf.push(esc),
                    }
                    self.advance();
                }
            } else {
                buf.push(ch);
                self.advance();
            }
        }

        // 期待闭引号
        if self.peek_char() == Some('\'') {
            self.advance();
        } else {
            self.errors.push(TokenizeError::new(
                "Unterminated character literal",
                self.current_span(),
            ));
        }

        let span = Span::merge(&start, &self.current_span());
        Token::new(TokenKind::CharLiteral, buf, span)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 运算符解析
    // ─────────────────────────────────────────────────────────────────────────

    fn parse_plus(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // +
        match self.peek_char() {
            Some('=') => {
                self.advance();
                Token::new(
                    TokenKind::PlusEq,
                    "+=",
                    Span::merge(&start, &self.current_span()),
                )
            }
            Some('+') => {
                self.advance();
                Token::new(
                    TokenKind::DoublePlus,
                    "++",
                    Span::merge(&start, &self.current_span()),
                )
            }
            _ => Token::new(TokenKind::Plus, "+", start),
        }
    }

    fn parse_minus(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // -
        match self.peek_char() {
            Some('=') => {
                self.advance();
                Token::new(
                    TokenKind::MinusEq,
                    "-=",
                    Span::merge(&start, &self.current_span()),
                )
            }
            Some('-') => {
                self.advance();
                Token::new(
                    TokenKind::DoubleMinus,
                    "--",
                    Span::merge(&start, &self.current_span()),
                )
            }
            Some('>') => {
                self.advance();
                Token::new(
                    TokenKind::Arrow,
                    "->",
                    Span::merge(&start, &self.current_span()),
                )
            }
            _ => Token::new(TokenKind::Minus, "-", start),
        }
    }

    fn parse_star(&mut self) -> Token {
        let start = self.current_span();
        self.advance();
        if self.peek_char() == Some('=') {
            self.advance();
            Token::new(
                TokenKind::StarEq,
                "*=",
                Span::merge(&start, &self.current_span()),
            )
        } else {
            Token::new(TokenKind::Star, "*", start)
        }
    }

    fn parse_slash_or_comment(&mut self) -> Token {
        let start = self.current_span();

        // 单行文档注释：/// ...（保留到行尾）
        if self.peek_n(1) == Some('/') && self.peek_n(2) == Some('/') {
            for _ in 0..3 {
                self.advance();
            }
            let mut text = String::new();
            while self.char_pos < self.chars.len() && self.chars[self.char_pos].0 != '\n' {
                text.push(self.chars[self.char_pos].0);
                self.advance();
            }
            return Token::new(
                TokenKind::DocComment,
                text.trim(),
                Span::merge(&start, &self.current_span()),
            );
        }

        // 块文档注释：/** ... */（排除空注释 /**/）
        if self.peek_n(1) == Some('*') && self.peek_n(2) == Some('*') && self.peek_n(3) != Some('/')
        {
            for _ in 0..3 {
                self.advance();
            }
            let mut text = String::new();
            loop {
                if self.char_pos >= self.chars.len() {
                    let span = Span::merge(&start, &self.current_span());
                    self.errors.push(TokenizeError::new(
                        "Unterminated documentation comment",
                        span,
                    ));
                    break;
                }
                let c = self.chars[self.char_pos].0;
                if c == '*' && self.peek_n(1) == Some('/') {
                    self.advance();
                    self.advance();
                    break;
                }
                text.push(c);
                self.advance();
            }
            // 去掉每行前导的 `*` 与多余空白
            let content: String = text
                .lines()
                .map(|line| line.trim().trim_start_matches('*').trim_start())
                .collect::<Vec<_>>()
                .join("\n");
            return Token::new(
                TokenKind::DocComment,
                content.trim().to_string(),
                Span::merge(&start, &self.current_span()),
            );
        }

        // 普通注释已在 skip_whitespace_and_comments 中跳过，这里只处理 /
        self.advance();
        if self.peek_char() == Some('=') {
            self.advance();
            Token::new(
                TokenKind::SlashEq,
                "/=",
                Span::merge(&start, &self.current_span()),
            )
        } else {
            Token::new(TokenKind::Slash, "/", start)
        }
    }

    fn parse_equal(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // =
        match self.peek_char() {
            Some('=') => {
                self.advance();
                Token::new(
                    TokenKind::EqEq,
                    "==",
                    Span::merge(&start, &self.current_span()),
                )
            }
            Some('>') => {
                self.advance();
                Token::new(
                    TokenKind::DoubleArrow,
                    "=>",
                    Span::merge(&start, &self.current_span()),
                )
            }
            _ => Token::new(TokenKind::Assign, "=", start),
        }
    }

    fn parse_bang(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // !
        match self.peek_char() {
            Some('=') => {
                self.advance();
                Token::new(
                    TokenKind::Neq,
                    "!=",
                    Span::merge(&start, &self.current_span()),
                )
            }
            Some('!') => {
                self.advance();
                Token::new(
                    TokenKind::DoubleBang,
                    "!!",
                    Span::merge(&start, &self.current_span()),
                )
            }
            _ => Token::new(TokenKind::Bang, "!", start),
        }
    }

    fn parse_lt(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // <
        match self.peek_char() {
            Some('=') => {
                self.advance();
                Token::new(
                    TokenKind::LtEq,
                    "<=",
                    Span::merge(&start, &self.current_span()),
                )
            }
            Some('<') => {
                self.advance();
                if self.peek_char() == Some('<') {
                    // 不是合法语法，但容错
                    self.advance();
                    Token::new(
                        TokenKind::Error,
                        "<<<",
                        Span::merge(&start, &self.current_span()),
                    )
                } else {
                    Token::new(
                        TokenKind::LtLt,
                        "<<",
                        Span::merge(&start, &self.current_span()),
                    )
                }
            }
            _ => Token::new(TokenKind::Lt, "<", start),
        }
    }

    fn parse_gt(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // >
        match self.peek_char() {
            Some('=') => {
                self.advance();
                Token::new(
                    TokenKind::GtEq,
                    ">=",
                    Span::merge(&start, &self.current_span()),
                )
            }
            Some('>') => {
                self.advance();
                if self.peek_char() == Some('>') {
                    self.advance();
                    Token::new(
                        TokenKind::GtGtGt,
                        ">>>",
                        Span::merge(&start, &self.current_span()),
                    )
                } else {
                    Token::new(
                        TokenKind::GtGt,
                        ">>",
                        Span::merge(&start, &self.current_span()),
                    )
                }
            }
            _ => Token::new(TokenKind::Gt, ">", start),
        }
    }

    fn parse_percent(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // %
        if self.peek_char() == Some('=') {
            self.advance();
            Token::new(
                TokenKind::PercentEq,
                "%=",
                Span::merge(&start, &self.current_span()),
            )
        } else {
            Token::new(TokenKind::Percent, "%", start)
        }
    }

    fn parse_pipe(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // |
        if self.peek_char() == Some('|') {
            self.advance();
            Token::new(
                TokenKind::OrOr,
                "||",
                Span::merge(&start, &self.current_span()),
            )
        } else {
            Token::new(TokenKind::Pipe, "|", start)
        }
    }

    fn parse_ampersand(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // &
        if self.peek_char() == Some('&') {
            self.advance();
            Token::new(
                TokenKind::AndAnd,
                "&&",
                Span::merge(&start, &self.current_span()),
            )
        } else {
            Token::new(TokenKind::Ampersand, "&", start)
        }
    }

    fn parse_colon(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // :
        match self.peek_char() {
            Some(':') => {
                self.advance();
                Token::new(
                    TokenKind::DoubleColon,
                    "::",
                    Span::merge(&start, &self.current_span()),
                )
            }
            Some('=') => {
                self.advance();
                Token::new(
                    TokenKind::DoubleColonT,
                    "::=",
                    Span::merge(&start, &self.current_span()),
                )
            }
            _ => Token::new(TokenKind::Colon, ":", start),
        }
    }

    fn parse_dot(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // .
        if self.peek_char() == Some('.') {
            self.advance();
            if self.peek_char() == Some('.') {
                self.advance();
                Token::new(
                    TokenKind::TripleDotOp,
                    "...",
                    Span::merge(&start, &self.current_span()),
                )
            } else {
                Token::new(
                    TokenKind::DoubleDotOp,
                    "..",
                    Span::merge(&start, &self.current_span()),
                )
            }
        } else {
            Token::new(TokenKind::Dot, ".", start)
        }
    }

    fn parse_question(&mut self) -> Token {
        let start = self.current_span();
        self.advance(); // ?
        if self.peek_char() == Some('?') {
            self.advance();
            Token::new(
                TokenKind::DoubleQMark,
                "??",
                Span::merge(&start, &self.current_span()),
            )
        } else {
            Token::new(TokenKind::QuestionMark, "?", start)
        }
    }

    fn parse_dollar(&mut self, _start: Span) -> Token {
        // 单独的 $ 在字符串外不常见，但容错处理
        let start = self.current_span();
        self.advance();
        if self.peek_char() == Some('{') {
            self.advance();
            // 跳过直到 }
            while self.char_pos < self.chars.len() && self.chars[self.char_pos].0 != '}' {
                self.advance();
            }
            if self.char_pos < self.chars.len() {
                self.advance(); // 跳过 }
            }
            Token::new(
                TokenKind::StringInterpStart,
                "${...}",
                Span::merge(&start, &self.current_span()),
            )
        } else {
            Token::new(TokenKind::Ident, "$", start)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 辅助函数
// ─────────────────────────────────────────────────────────────────────────────

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

fn is_hex_digit(c: char) -> bool {
    c.is_ascii_hexdigit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(src: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(src);
        lexer.tokenize()
    }

    fn kind(tok: &Token) -> TokenKind {
        tok.kind
    }

    #[test]
    fn test_basic_keywords() {
        let tokens = tokenize("val x = 100");
        assert_eq!(kind(&tokens[0]), TokenKind::Val);
        assert_eq!(tokens[0].literal, "val");
        assert_eq!(kind(&tokens[1]), TokenKind::Ident);
        assert_eq!(tokens[1].literal, "x");
        assert_eq!(kind(&tokens[2]), TokenKind::Assign);
        assert_eq!(kind(&tokens[3]), TokenKind::IntLiteral);
        assert_eq!(tokens[3].literal, "100");
    }

    #[test]
    fn test_identifier_and_keyword_disambiguation() {
        let tokens = tokenize("myVar");
        assert_eq!(kind(&tokens[0]), TokenKind::Ident);
        assert_eq!(tokens[0].literal, "myVar");

        let tokens = tokenize("fun main() {}");
        assert_eq!(kind(&tokens[0]), TokenKind::Fun);
        assert_eq!(tokens[0].literal, "fun");
        assert_eq!(kind(&tokens[1]), TokenKind::Ident);
        assert_eq!(tokens[1].literal, "main");
        assert_eq!(kind(&tokens[2]), TokenKind::LParen);
    }

    #[test]
    fn test_integer_literals() {
        let tokens = tokenize("123 0xFF 0b1100 1_000_000");
        assert_eq!(kind(&tokens[0]), TokenKind::IntLiteral);
        assert_eq!(tokens[0].literal, "123");
        assert_eq!(kind(&tokens[1]), TokenKind::IntLiteral);
        assert_eq!(tokens[1].literal, "0xFF");
        assert_eq!(kind(&tokens[2]), TokenKind::IntLiteral);
        assert_eq!(tokens[2].literal, "0b1100");
        assert_eq!(kind(&tokens[3]), TokenKind::IntLiteral);
        assert_eq!(tokens[3].literal, "1_000_000");
    }

    #[test]
    fn test_float_literals() {
        let tokens = tokenize("3.14 2.0f 1.5e10 1.2E-3");
        assert_eq!(kind(&tokens[0]), TokenKind::FloatLiteral);
        assert_eq!(tokens[0].literal, "3.14");
        assert_eq!(kind(&tokens[1]), TokenKind::FloatLiteral);
        assert_eq!(tokens[1].literal, "2.0f");
        assert_eq!(kind(&tokens[2]), TokenKind::FloatLiteral);
        assert_eq!(tokens[2].literal, "1.5e10");
        assert_eq!(kind(&tokens[3]), TokenKind::FloatLiteral);
        assert_eq!(tokens[3].literal, "1.2E-3");
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize("\"hello world\"");
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(tokens[0].literal, "hello world");
    }

    #[test]
    fn test_string_with_escapes() {
        let tokens = tokenize("\"hello\\nworld\"");
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(tokens[0].literal, "hello\nworld");
    }

    // Phase 8：JSON 等场景需要的转义（退格 / 换页 / 垂直制表 / 斜杠）
    #[test]
    fn test_string_with_json_escapes() {
        let mut lexer = Lexer::new("\"a\\bb\\fc\\vd\\/e\"");
        let tokens = lexer.tokenize();
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(tokens[0].literal, "a\u{8}b\u{c}c\u{b}d/e");
    }

    #[test]
    fn test_raw_string_literal() {
        let tokens = tokenize("\"\"\"hello world\"\"\"");
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(tokens[0].literal, "hello world");
    }

    #[test]
    fn test_raw_string_multiline() {
        let tokens = tokenize("\"\"\"line1\nline2\"\"\"");
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(tokens[0].literal, "line1\nline2");
        // 跨行后行号应正确推进到下一行
        assert_eq!(tokens[0].span.end_line, 2);
    }

    #[test]
    fn test_raw_string_no_escape() {
        // 原始字符串不做转义处理
        let tokens = tokenize("\"\"\"a\\nb\"\"\"");
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(tokens[0].literal, "a\\nb");
    }

    #[test]
    fn test_raw_string_no_interpolation() {
        // 原始字符串不做 $ 插值
        let tokens = tokenize("\"\"\"cost is $name\"\"\"");
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(tokens[0].literal, "cost is $name");
    }

    #[test]
    fn test_unterminated_raw_string() {
        let mut lexer = Lexer::new("\"\"\"abc");
        let tokens = lexer.tokenize();
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(lexer.errors().len(), 1);
        assert!(lexer.errors()[0].message.contains("Unterminated raw string"));
    }

    #[test]
    fn test_newline_in_plain_string() {
        // 普通字符串不允许跨行
        let mut lexer = Lexer::new("\"abc\ndef\"");
        let tokens = lexer.tokenize();
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        // 换行报错后会继续扫描，后续内容可能再产生未闭合字符串错误
        assert!(!lexer.errors().is_empty());
        assert!(lexer.errors()[0].message.contains("newline in string"));
    }

    #[test]
    fn test_string_interpolation_var() {
        // 新行为：字符串插值被拆分为多个 token
        // "hello $name" → StringLiteral("hello ") + StringInterpStart($) + Identifier(name) + ...
        let tokens = tokenize("\"hello $name\"");
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(tokens[0].literal, "hello ");
    }

    #[test]
    fn test_string_interpolation_expr() {
        // 新行为：${expr} 被拆分为多个 token
        // "${1 + 2}" → StringInterpStart(${) + IntLiteral(1) + Plus + IntLiteral(2) + ...
        let tokens = tokenize("\"${1 + 2}\"");
        assert_eq!(kind(&tokens[0]), TokenKind::StringInterpStart);
    }

    #[test]
    fn test_doc_comment_line() {
        // 文档注释保留为 Token（普通注释被跳过）
        let tokens = tokenize("/// Adds two numbers");
        assert_eq!(kind(&tokens[0]), TokenKind::DocComment);
        assert_eq!(tokens[0].literal, "Adds two numbers");
    }

    #[test]
    fn test_doc_comment_block() {
        let tokens = tokenize("/** Adds two numbers */");
        assert_eq!(kind(&tokens[0]), TokenKind::DocComment);
        assert_eq!(tokens[0].literal, "Adds two numbers");
    }

    #[test]
    fn test_plain_comments_are_still_skipped() {
        let tokens = tokenize("// plain comment\n/* block */\nval x = 1");
        assert_eq!(kind(&tokens[0]), TokenKind::Val);
    }

    #[test]
    fn test_char_literal() {
        let tokens = tokenize("'A'");
        assert_eq!(kind(&tokens[0]), TokenKind::CharLiteral);
        assert_eq!(tokens[0].literal, "A");
    }

    #[test]
    fn test_bool_literals() {
        let tokens = tokenize("true false");
        assert_eq!(kind(&tokens[0]), TokenKind::BoolLiteral);
        assert_eq!(tokens[0].literal, "true");
        assert_eq!(kind(&tokens[1]), TokenKind::BoolLiteral);
        assert_eq!(tokens[1].literal, "false");
    }

    #[test]
    fn test_operators() {
        let src = "+ - * / % ++ -- += -= *= /= %= == != < > <= >= && || ! << >> >>> ?? !!";
        let tokens = tokenize(src);
        let expected: Vec<TokenKind> = vec![
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::DoublePlus,
            TokenKind::DoubleMinus,
            TokenKind::PlusEq,
            TokenKind::MinusEq,
            TokenKind::StarEq,
            TokenKind::SlashEq,
            TokenKind::PercentEq,
            TokenKind::EqEq,
            TokenKind::Neq,
            TokenKind::Lt,
            TokenKind::Gt,
            TokenKind::LtEq,
            TokenKind::GtEq,
            TokenKind::AndAnd,
            TokenKind::OrOr,
            TokenKind::Bang,
            TokenKind::LtLt,
            TokenKind::GtGt,
            TokenKind::GtGtGt,
            TokenKind::DoubleQMark,
            TokenKind::DoubleBang,
        ];
        for (i, (tok, exp)) in tokens.iter().zip(expected.iter()).enumerate() {
            assert_eq!(
                tok.kind, *exp,
                "token[{}] at position {}: expected {:?}, got {:?} (literal='{}')",
                i, i, exp, tok.kind, tok.literal
            );
        }
    }

    #[test]
    fn test_delimiters() {
        let src = "( ) { } [ ] , ; : .";
        let tokens = tokenize(src);
        let kinds: Vec<TokenKind> =
            tokens.iter().filter(|t| t.kind != TokenKind::EOF).map(|t| t.kind).collect();
        let expected = vec![
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::LBracket,
            TokenKind::RBracket,
            TokenKind::Comma,
            TokenKind::Semicolon,
            TokenKind::Colon,
            TokenKind::Dot,
        ];
        assert_eq!(kinds, expected);
    }

    #[test]
    fn test_single_line_comment() {
        let tokens = tokenize("val x = 1 // this is a comment\nval y = 2");
        assert_eq!(kind(&tokens[0]), TokenKind::Val);
        assert_eq!(kind(&tokens[3]), TokenKind::IntLiteral);
        assert_eq!(tokens[3].literal, "1");
        // 注释后应该直接是 val
        assert_eq!(kind(&tokens[4]), TokenKind::Val);
        assert_eq!(tokens[4].literal, "val");
        assert_eq!(kind(&tokens[7]), TokenKind::IntLiteral);
        assert_eq!(tokens[7].literal, "2");
    }

    #[test]
    fn test_block_comment() {
        let tokens = tokenize("val x = 1 /* comment */ val y = 2");
        assert_eq!(kind(&tokens[0]), TokenKind::Val);
        assert_eq!(kind(&tokens[4]), TokenKind::Val);
        assert_eq!(kind(&tokens[7]), TokenKind::IntLiteral);
        assert_eq!(tokens[7].literal, "2");
    }

    #[test]
    fn test_block_comment_multiline() {
        let tokens = tokenize("val x = 1 /* multi\nline\ncomment */ val y = 2");
        assert_eq!(kind(&tokens[0]), TokenKind::Val);
        assert_eq!(kind(&tokens[4]), TokenKind::Val);
        assert_eq!(kind(&tokens[7]), TokenKind::IntLiteral);
        assert_eq!(tokens[7].literal, "2");
    }

    #[test]
    fn test_unterminated_string_error() {
        let tokens = tokenize("\"hello");
        assert_eq!(kind(&tokens[0]), TokenKind::StringLiteral);
        assert_eq!(tokens[0].literal, "hello");
        // 应有错误
        let mut lexer = Lexer::new("\"hello");
        let _ = lexer.tokenize();
        assert!(lexer.errors().len() > 0);
    }

    #[test]
    fn test_arrow_operators() {
        let tokens = tokenize("-> => .. ... :: @");
        assert_eq!(kind(&tokens[0]), TokenKind::Arrow);
        assert_eq!(kind(&tokens[1]), TokenKind::DoubleArrow);
        assert_eq!(kind(&tokens[2]), TokenKind::DoubleDotOp);
        assert_eq!(kind(&tokens[3]), TokenKind::TripleDotOp);
        assert_eq!(kind(&tokens[4]), TokenKind::DoubleColon);
        assert_eq!(kind(&tokens[5]), TokenKind::At);
    }

    #[test]
    fn test_question_mark_and_elvis() {
        let tokens = tokenize("? ??:");
        assert_eq!(kind(&tokens[0]), TokenKind::QuestionMark);
        assert_eq!(kind(&tokens[1]), TokenKind::DoubleQMark);
        assert_eq!(kind(&tokens[2]), TokenKind::Colon);
    }

    #[test]
    fn test_special_keywords() {
        let src = "actor sealed suspend inline comptime defer extern lateinit lazy";
        let tokens = tokenize(src);
        assert_eq!(kind(&tokens[0]), TokenKind::Actor);
        assert_eq!(kind(&tokens[1]), TokenKind::Sealed);
        assert_eq!(kind(&tokens[2]), TokenKind::Suspend);
        assert_eq!(kind(&tokens[3]), TokenKind::Inline);
        assert_eq!(kind(&tokens[4]), TokenKind::Comptime);
        assert_eq!(kind(&tokens[5]), TokenKind::Defer);
        assert_eq!(kind(&tokens[6]), TokenKind::Extern);
        assert_eq!(kind(&tokens[7]), TokenKind::Lateinit);
        assert_eq!(kind(&tokens[8]), TokenKind::Lazy);
    }

    #[test]
    fn test_span_tracking() {
        let tokens = tokenize("val x = 100");
        // "val" 应该在第 1 行第 1 列
        assert_eq!(tokens[0].span.start_line, 1);
        assert_eq!(tokens[0].span.start_col, 1);
        assert_eq!(tokens[0].span.end_line, 1);
        // "x" 应该在 col 5
        assert_eq!(tokens[1].span.start_col, 5);
        assert_eq!(tokens[1].span.end_col, 5);
    }

    #[test]
    fn test_double_dot_and_triple_dot() {
        let tokens = tokenize(".. ...");
        assert_eq!(kind(&tokens[0]), TokenKind::DoubleDotOp);
        assert_eq!(tokens[0].literal, "..");
        assert_eq!(kind(&tokens[1]), TokenKind::TripleDotOp);
        assert_eq!(tokens[1].literal, "...");
    }

    // ─── 多行注释 /* ... */ ──────────────────────────────────────────────

    /// 空的多行注释不应报错
    #[test]
    fn test_block_comment_empty() {
        let mut lexer = Lexer::new("/**/");
        let tokens = lexer.tokenize();
        assert_eq!(tokens.len(), 1);
        assert_eq!(kind(&tokens[0]), TokenKind::EOF);
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
    }

    /// 多行注释在文件末尾（*/ 紧接 EOF）不应误报 unterminated
    #[test]
    fn test_block_comment_at_eof() {
        let mut lexer = Lexer::new("val x = 1 /* comment */");
        let tokens = lexer.tokenize();
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::EOF
            ]
        );
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
    }

    /// 仅含一个空格的多行注释
    #[test]
    fn test_block_comment_single_space() {
        let mut lexer = Lexer::new("val x = 1 /* */ val y = 2");
        let tokens = lexer.tokenize();
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::EOF
            ]
        );
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
    }

    /// 多行注释出现在表达式中间
    #[test]
    fn test_block_comment_mid_expression() {
        let mut lexer = Lexer::new("val x = 1 + /* mid */ 2");
        let tokens = lexer.tokenize();
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::Plus,
                TokenKind::IntLiteral,
                TokenKind::EOF
            ]
        );
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
    }

    /// 多行注释出现在关键字之间
    #[test]
    fn test_block_comment_between_keywords() {
        let mut lexer = Lexer::new("fun /* comment */ main() {}");
        let tokens = lexer.tokenize();
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Fun,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::EOF
            ]
        );
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
    }

    /// 多行注释跨越多行（新增的更严格断言版本）
    #[test]
    fn test_block_comment_spanning_lines() {
        let mut lexer = Lexer::new("val x = 1 /* multi\nline\ncomment */ val y = 2");
        let tokens = lexer.tokenize();
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::EOF
            ]
        );
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
    }

    /// 注释内容中含有 * 与 / 但不是紧邻的 **/ 时不应终止
    #[test]
    fn test_block_comment_with_star_slash_content() {
        let mut lexer = Lexer::new("val x = 1 /* * / */ val y = 2");
        let tokens = lexer.tokenize();
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::EOF
            ]
        );
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
    }

    /// 多行注释之间可以夹带代码
    #[test]
    fn test_multiple_block_comments() {
        let mut lexer = Lexer::new("/* a */ val x = 1 /* b */ val y = 2 /* c */");
        let tokens = lexer.tokenize();
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::EOF,
            ]
        );
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
    }

    /// 未闭合的多行注释应报错
    #[test]
    fn test_unterminated_block_comment() {
        let mut lexer = Lexer::new("val x = 1 /* unclosed");
        let tokens = lexer.tokenize();
        // tokens 不应包含被吞掉的后续内容
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Val,
                TokenKind::Ident,
                TokenKind::Assign,
                TokenKind::IntLiteral,
                TokenKind::EOF
            ]
        );
        assert_eq!(lexer.errors().len(), 1);
        assert_eq!(lexer.errors()[0].message, "Unterminated block comment");
    }

    /// 注释内的换行不影响后续 token 的行/列
    #[test]
    fn test_block_comment_line_tracking() {
        let mut lexer = Lexer::new("val x = 1 /* multi\nline */ val y = 2");
        let tokens = lexer.tokenize();
        // val y = 2 应该在第 2 行
        let val_y = tokens.iter().find(|t| t.literal == "y").unwrap();
        assert_eq!(val_y.span.start_line, 2);
        assert!(lexer.errors().is_empty(), "errors: {:?}", lexer.errors());
    }
}
