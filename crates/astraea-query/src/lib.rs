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
