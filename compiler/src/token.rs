use crate::Span;

/// Token kind enumeration — covers all lexical elements from the Aura language spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum TokenKind {
    // ============ Literals ============
    IntLiteral,    // 123, 0xFF, 0b1100, 1_000_000
    FloatLiteral,  // 3.14, 3.14f, 3.14d
    StringLiteral, // "hello world"
    CharLiteral,   // 'A'
    BoolLiteral,   // true, false
    Ident,         // variable/keyword identifier (raw string)

    // ============ Keywords ============
    Val,
    Var,
    Fun,
    Struct,
    Class,
    Interface,
    Enum,
    Actor,
    Sealed,
    Return,
    If,
    Else,
    When,
    For,
    In,
    While,
    Do,
    Break,
    Continue,
    Try,
    Catch,
    Finally,
    Throw,
    Is,
    As,
    Public,
    Private,
    Protected,
    Null,
    This,
    Super,
    Object,
    Unit,
    Typealias,
    Override,
    Suspend,
    Inline,
    Const,
    Comptime,
    Defer,
    Extern,
    Import,
    Lazy,
    Lateinit,
    Data,
    Value,
    To,
    Await,
    // P10 并发运行时关键字
    Async,
    Select,
    Channel,
    Spawn,
    Send,
    Ask,
    // P7 内存管理关键字
    Box,
    Weak,
    Malloc,
    Free,
    Retain,
    Release,

    // ============ Operators ============
    // Arithmetic
    Plus,        // +
    Minus,       // -
    Star,        // *
    Slash,       // /
    Percent,     // %
    DoublePlus,  // ++
    DoubleMinus, // --
    // Assignment
    Assign,    // =
    PlusEq,    // +=
    MinusEq,   // -=
    StarEq,    // *=
    SlashEq,   // /=
    PercentEq, // %=
    // Comparison
    EqEq, // ==
    Neq,  // !=
    Lt,   // <
    Gt,   // >
    LtEq, // <=
    GtEq, // >=
    // Logical
    AndAnd, // &&
    OrOr,   // ||
    Bang,   // !
    // Bitwise
    Ampersand, // &
    Pipe,      // |
    Caret,     // ^
    LtLt,      // <<
    GtGt,      // >>
    GtGtGt,    // >>>
    // Null safety
    DoubleBang, // !!
    // Special operators
    Arrow,       // ->
    DoubleArrow, // =>
    DoubleDot,   // ..
    TripleDot,   // ...
    At,          // @
    Hash,        // #
    DoubleColon, // ::

    // ============ Delimiters ============
    LParen,       // (
    RParen,       // )
    LBrace,       // {
    RBrace,       // }
    LBracket,     // [
    RBracket,     // ]
    Comma,        // ,
    Semicolon,    // ;
    Colon,        // :
    DoubleColonT, // ::
    Dot,          // .
    DoubleDotOp,  // ..
    TripleDotOp,  // ...
    QuestionMark, // ?
    DoubleQMark,  // ??

    // ============ Interpolation (for string interpolation tracking) ============
    StringPart,        // static text inside a string
    StringInterpStart, // ${ or $
    StringInterpEnd,   // }

    // ============ Documentation ============
    DocComment, // /// ... 或 /** ... */（KDoc 风格文档注释）

    // ============ End of input ============
    EOF,

    // ============ Error token ============
    Error,
}

impl TokenKind {
    /// 人类可读的名称（用于 CLI 展示）
    pub fn display_name(&self) -> &'static str {
        match self {
            TokenKind::EOF => "EOF",
            TokenKind::DocComment => "DocComment",
            _ => "—",
        }
    }

    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            TokenKind::Val
                | TokenKind::Var
                | TokenKind::Fun
                | TokenKind::Struct
                | TokenKind::Class
                | TokenKind::Interface
                | TokenKind::Enum
                | TokenKind::Actor
                | TokenKind::Sealed
                | TokenKind::Return
                | TokenKind::If
                | TokenKind::Else
                | TokenKind::When
                | TokenKind::For
                | TokenKind::In
                | TokenKind::While
                | TokenKind::Do
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Try
                | TokenKind::Catch
                | TokenKind::Finally
                | TokenKind::Throw
                | TokenKind::Is
                | TokenKind::As
                | TokenKind::Public
                | TokenKind::Private
                | TokenKind::Protected
                | TokenKind::Null
                | TokenKind::This
                | TokenKind::Super
                | TokenKind::Object
                | TokenKind::Unit
                | TokenKind::Typealias
                | TokenKind::Override
                | TokenKind::Suspend
                | TokenKind::Inline
                | TokenKind::Const
                | TokenKind::Comptime
                | TokenKind::Defer
                | TokenKind::Extern
                | TokenKind::Import
                | TokenKind::Lazy
                | TokenKind::Lateinit
                | TokenKind::Value
                | TokenKind::Async
                | TokenKind::Select
                | TokenKind::Channel
                | TokenKind::Spawn
                | TokenKind::Send
                | TokenKind::Ask
                | TokenKind::Box
                | TokenKind::Weak
                | TokenKind::Malloc
                | TokenKind::Free
                | TokenKind::Retain
                | TokenKind::Release
        )
    }

    pub fn is_operator(&self) -> bool {
        matches!(
            self,
            TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::DoublePlus
                | TokenKind::DoubleMinus
                | TokenKind::Assign
                | TokenKind::PlusEq
                | TokenKind::MinusEq
                | TokenKind::StarEq
                | TokenKind::SlashEq
                | TokenKind::PercentEq
                | TokenKind::EqEq
                | TokenKind::Neq
                | TokenKind::Lt
                | TokenKind::Gt
                | TokenKind::LtEq
                | TokenKind::GtEq
                | TokenKind::AndAnd
                | TokenKind::OrOr
                | TokenKind::Bang
                | TokenKind::Ampersand
                | TokenKind::Pipe
                | TokenKind::Caret
                | TokenKind::LtLt
                | TokenKind::GtGt
                | TokenKind::GtGtGt
                | TokenKind::DoubleBang
                | TokenKind::Arrow
                | TokenKind::DoubleArrow
                | TokenKind::DoubleDot
                | TokenKind::TripleDot
                | TokenKind::At
                | TokenKind::Hash
                | TokenKind::DoubleColon
                | TokenKind::QuestionMark
                | TokenKind::DoubleQMark
                | TokenKind::Dot
                | TokenKind::DoubleDotOp
                | TokenKind::TripleDotOp
                | TokenKind::DoubleColonT
        )
    }

    pub fn is_delimiter(&self) -> bool {
        matches!(
            self,
            TokenKind::LParen
                | TokenKind::RParen
                | TokenKind::LBrace
                | TokenKind::RBrace
                | TokenKind::LBracket
                | TokenKind::RBracket
                | TokenKind::Comma
                | TokenKind::Semicolon
                | TokenKind::Colon
                | TokenKind::DoubleColonT
        )
    }
}

/// A token produced by the lexer.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    /// Raw string representation of the token (useful for identifiers, literals, errors).
    pub literal: String,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, literal: impl Into<String>, span: Span) -> Self {
        Self {
            kind,
            literal: literal.into(),
            span,
        }
    }

    pub fn ident(name: String, span: Span) -> Self {
        Self {
            kind: TokenKind::Ident,
            literal: name,
            span,
        }
    }

    pub fn is_keyword(&self) -> bool {
        self.kind.is_keyword()
    }

    pub fn is_delimiter(&self) -> bool {
        self.kind.is_delimiter()
    }

    pub fn is_operator(&self) -> bool {
        self.kind.is_operator()
    }

    pub fn display(&self) -> String {
        if self.kind == TokenKind::Ident && !self.literal.is_empty() {
            self.literal.clone()
        } else if self.kind == TokenKind::EOF {
            "<EOF>".to_string()
        } else if self.kind == TokenKind::Error {
            format!("<Error: {}>", self.literal)
        } else if self.kind == TokenKind::StringPart {
            format!("\"{}\"", self.literal)
        } else {
            format!("{:?}", self.kind)
        }
    }
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.display(), self.span)
    }
}

/// Tokenization error.
#[derive(Debug)]
pub struct TokenizeError {
    pub message: String,
    pub span: Span,
}

impl TokenizeError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl std::fmt::Display for TokenizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]: {}", self.span, self.message)
    }
}
