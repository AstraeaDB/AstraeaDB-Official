/// Source-position span for error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Byte offset of the start of this token in the source.
    pub start: usize,
    /// Byte offset one past the end of this token in the source.
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// All token types produced by the lexer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // ── Keywords ───────────────────────────────────────────
    Match,
    Where,
    Return,
    Create,
    Delete,
    Set,
    As,
    And,
    Or,
    Not,
    True,
    False,
    Null,
    In,
    Is,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Skip,
    Distinct,

    // ── Literals ───────────────────────────────────────────
    Integer(i64),
    Float(f64),
    StringLit(String),
    Identifier(String),

    // ── Symbols ────────────────────────────────────────────
    LeftParen,    // (
    RightParen,   // )
    LeftBracket,  // [
    RightBracket, // ]
    LeftBrace,    // {
    RightBrace,   // }
    Colon,        // :
    Comma,        // ,
    Dot,          // .
    Arrow,        // ->
    LeftArrow,    // <-
    Dash,         // -
    Equals,       // =
    NotEquals,    // <>
    LessThan,     // <
    LessEqual,    // <=
    GreaterThan,  // >
    GreaterEqual, // >=
    Plus,         // +
    Star,         // *
    Slash,        // /
    Percent,      // %
    Pipe,         // |

    // ── Special ────────────────────────────────────────────
    Eof,
}

/// A token together with its source span.
pub type SpannedToken = (Token, Span);
