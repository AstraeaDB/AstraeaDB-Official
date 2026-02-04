use astraea_core::{AstraeaError, Result};

use crate::token::{Span, SpannedToken, Token};

/// Tokenizer for the GQL/Cypher query language subset.
pub struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    /// Create a new lexer over the given source string.
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    /// Peek at the current character without consuming it.
    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    /// Peek at the character one position ahead.
    fn peek_next(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    /// Advance by one character, returning it.
    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    /// Skip whitespace and single-line comments (`//`).
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // skip whitespace
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.advance();
            }
            // skip single-line comment
            if self.peek() == Some('/') && self.peek_next() == Some('/') {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.advance();
                }
                continue; // re-check for more whitespace / comments
            }
            break;
        }
    }

    /// Produce the next token from the input.
    pub fn next_token(&mut self) -> Result<SpannedToken> {
        self.skip_whitespace_and_comments();

        let start = self.pos;

        let Some(ch) = self.peek() else {
            return Ok((Token::Eof, Span::new(start, start)));
        };

        // ── String literals ────────────────────────────────
        if ch == '\'' || ch == '"' {
            return self.lex_string(start);
        }

        // ── Numbers ────────────────────────────────────────
        if ch.is_ascii_digit() {
            return self.lex_number(start);
        }

        // ── Identifiers / keywords ─────────────────────────
        if ch.is_ascii_alphabetic() || ch == '_' {
            return self.lex_identifier(start);
        }

        // ── Symbols ────────────────────────────────────────
        self.advance(); // consume `ch`

        let token = match ch {
            '(' => Token::LeftParen,
            ')' => Token::RightParen,
            '[' => Token::LeftBracket,
            ']' => Token::RightBracket,
            '{' => Token::LeftBrace,
            '}' => Token::RightBrace,
            ':' => Token::Colon,
            ',' => Token::Comma,
            '.' => Token::Dot,
            '+' => Token::Plus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '%' => Token::Percent,
            '|' => Token::Pipe,
            '=' => Token::Equals,
            '-' => {
                if self.peek() == Some('>') {
                    self.advance();
                    Token::Arrow
                } else {
                    Token::Dash
                }
            }
            '<' => {
                if self.peek() == Some('>') {
                    self.advance();
                    Token::NotEquals
                } else if self.peek() == Some('=') {
                    self.advance();
                    Token::LessEqual
                } else if self.peek() == Some('-') {
                    self.advance();
                    Token::LeftArrow
                } else {
                    Token::LessThan
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::GreaterEqual
                } else {
                    Token::GreaterThan
                }
            }
            _ => {
                return Err(AstraeaError::ParseError {
                    position: start,
                    message: format!("unexpected character '{ch}'"),
                });
            }
        };

        Ok((token, Span::new(start, self.pos)))
    }

    /// Lex a quoted string literal (single or double quotes).
    fn lex_string(&mut self, start: usize) -> Result<SpannedToken> {
        let quote = self.advance().unwrap(); // consume opening quote
        let mut value = String::new();

        loop {
            match self.advance() {
                None => {
                    return Err(AstraeaError::ParseError {
                        position: start,
                        message: "unterminated string literal".into(),
                    });
                }
                Some('\\') => {
                    // simple escape sequences
                    match self.advance() {
                        Some('n') => value.push('\n'),
                        Some('t') => value.push('\t'),
                        Some('\\') => value.push('\\'),
                        Some(c) if c == quote => value.push(c),
                        Some(c) => {
                            value.push('\\');
                            value.push(c);
                        }
                        None => {
                            return Err(AstraeaError::ParseError {
                                position: self.pos,
                                message: "unterminated escape sequence".into(),
                            });
                        }
                    }
                }
                Some(c) if c == quote => break,
                Some(c) => value.push(c),
            }
        }

        Ok((Token::StringLit(value), Span::new(start, self.pos)))
    }

    /// Lex an integer or float literal.
    fn lex_number(&mut self, start: usize) -> Result<SpannedToken> {
        // consume integer digits
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.advance();
        }

        // check for fractional part
        let is_float = self.peek() == Some('.')
            && self.peek_next().is_some_and(|c| c.is_ascii_digit());

        if is_float {
            self.advance(); // consume '.'
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.advance();
            }
            let text: String = self.input[start..self.pos].iter().collect();
            let value: f64 = text.parse().map_err(|_| AstraeaError::ParseError {
                position: start,
                message: format!("invalid float literal '{text}'"),
            })?;
            Ok((Token::Float(value), Span::new(start, self.pos)))
        } else {
            let text: String = self.input[start..self.pos].iter().collect();
            let value: i64 = text.parse().map_err(|_| AstraeaError::ParseError {
                position: start,
                message: format!("invalid integer literal '{text}'"),
            })?;
            Ok((Token::Integer(value), Span::new(start, self.pos)))
        }
    }

    /// Lex an identifier or keyword (case-insensitive keyword matching).
    fn lex_identifier(&mut self, start: usize) -> Result<SpannedToken> {
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.advance();
        }

        let text: String = self.input[start..self.pos].iter().collect();
        let token = match text.to_ascii_uppercase().as_str() {
            "MATCH" => Token::Match,
            "WHERE" => Token::Where,
            "RETURN" => Token::Return,
            "CREATE" => Token::Create,
            "DELETE" => Token::Delete,
            "SET" => Token::Set,
            "AS" => Token::As,
            "AND" => Token::And,
            "OR" => Token::Or,
            "NOT" => Token::Not,
            "TRUE" => Token::True,
            "FALSE" => Token::False,
            "NULL" => Token::Null,
            "IN" => Token::In,
            "IS" => Token::Is,
            "ORDER" => Token::Order,
            "BY" => Token::By,
            "ASC" => Token::Asc,
            "DESC" => Token::Desc,
            "LIMIT" => Token::Limit,
            "SKIP" => Token::Skip,
            "DISTINCT" => Token::Distinct,
            _ => Token::Identifier(text),
        };

        Ok((token, Span::new(start, self.pos)))
    }

    /// Tokenize the entire input, returning all tokens including Eof.
    pub fn tokenize_all(&mut self) -> Result<Vec<SpannedToken>> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.0 == Token::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }
}

// ───────────────────────────────────────────────────────────
// Tests
// ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: tokenize input and return just the Token variants.
    fn tokens(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input);
        let spanned = lexer.tokenize_all().expect("tokenize failed");
        spanned.into_iter().map(|(t, _)| t).collect()
    }

    #[test]
    fn test_empty_input() {
        assert_eq!(tokens(""), vec![Token::Eof]);
        assert_eq!(tokens("   \n\t  "), vec![Token::Eof]);
    }

    #[test]
    fn test_keywords_case_insensitive() {
        let toks = tokens("MATCH match Match");
        assert_eq!(toks, vec![Token::Match, Token::Match, Token::Match, Token::Eof]);
    }

    #[test]
    fn test_string_literals() {
        let toks = tokens(r#" "hello" 'world' "#);
        assert_eq!(
            toks,
            vec![
                Token::StringLit("hello".into()),
                Token::StringLit("world".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_numbers() {
        let toks = tokens("42 3.14 0 100");
        assert_eq!(
            toks,
            vec![
                Token::Integer(42),
                Token::Float(3.14),
                Token::Integer(0),
                Token::Integer(100),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_symbols_and_arrows() {
        let toks = tokens("()->[]{}:,.-><-<><=>===+-*/%|");
        assert_eq!(
            toks,
            vec![
                Token::LeftParen,
                Token::RightParen,
                Token::Arrow,        // ->
                Token::LeftBracket,
                Token::RightBracket,
                Token::LeftBrace,
                Token::RightBrace,
                Token::Colon,
                Token::Comma,
                Token::Dot,
                Token::Arrow,        // ->
                Token::LeftArrow,    // <-
                Token::NotEquals,    // <>
                Token::LessEqual,    // <=
                Token::GreaterEqual, // >=
                Token::Equals,       // =
                Token::Equals,       // =
                Token::Plus,
                Token::Dash,
                Token::Star,
                Token::Slash,
                Token::Percent,
                Token::Pipe,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_single_line_comment() {
        let toks = tokens("MATCH // this is a comment\nRETURN");
        assert_eq!(toks, vec![Token::Match, Token::Return, Token::Eof]);
    }

    #[test]
    fn test_identifier_with_underscore() {
        let toks = tokens("my_var _private a123");
        assert_eq!(
            toks,
            vec![
                Token::Identifier("my_var".into()),
                Token::Identifier("_private".into()),
                Token::Identifier("a123".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_invalid_character() {
        let mut lexer = Lexer::new("@");
        let result = lexer.next_token();
        assert!(result.is_err());
    }

    #[test]
    fn test_full_match_query_tokens() {
        let input = "MATCH (a:Person)-[:KNOWS]->(b) WHERE a.age > 30 RETURN b.name";
        let toks = tokens(input);
        assert_eq!(
            toks,
            vec![
                Token::Match,
                Token::LeftParen,
                Token::Identifier("a".into()),
                Token::Colon,
                Token::Identifier("Person".into()),
                Token::RightParen,
                Token::Dash,
                Token::LeftBracket,
                Token::Colon,
                Token::Identifier("KNOWS".into()),
                Token::RightBracket,
                Token::Arrow,
                Token::LeftParen,
                Token::Identifier("b".into()),
                Token::RightParen,
                Token::Where,
                Token::Identifier("a".into()),
                Token::Dot,
                Token::Identifier("age".into()),
                Token::GreaterThan,
                Token::Integer(30),
                Token::Return,
                Token::Identifier("b".into()),
                Token::Dot,
                Token::Identifier("name".into()),
                Token::Eof,
            ]
        );
    }
}
