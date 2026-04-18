//! GQL/Cypher-style query engine over `astraea_core::GraphOps`.
//!
//! `parse(input)` is the single entry point and returns a `Statement`
//! (`Match` / `Create` / `Delete`). The recursive-descent `Parser`
//! consumes a `Lexer` token stream into the `ast` types
//! (`MatchQuery`, `NodePattern`, `EdgePattern`, `Expr`, etc.); the
//! `Executor` then runs the statement against an `Arc<dyn GraphOps>`
//! and produces a `QueryResult` plus `QueryStats`.
//!
//! Invariants: patterns must start with a node, then alternate edge /
//! node / edge / node. The MATCH pipeline is fixed: pattern → WHERE →
//! ORDER BY → RETURN → DISTINCT → SKIP → LIMIT, and ORDER BY runs on
//! pre-projection bindings (so RETURN aliases are not visible to it).
//! Standalone `DELETE var` only works when `var` parses as `u64` — there
//! is no MATCH...DELETE chaining yet.

// GQL query engine — Steps 3.1, 3.2, 3.3

pub mod ast;
pub mod executor;
pub mod lexer;
pub mod parser;
pub mod token;

// Re-export commonly used items.
pub use ast::*;
pub use parser::Parser;

/// Parse a GQL/Cypher query string into a [`Statement`] AST.
///
/// This is the main entry point for the query parser.
///
/// # Errors
///
/// Returns `AstraeaError::ParseError` if the input contains syntax errors.
pub fn parse(input: &str) -> astraea_core::Result<Statement> {
    let mut parser = Parser::new(input)?;
    parser.parse()
}
