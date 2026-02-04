use astraea_core::{AstraeaError, Result};

use crate::ast::*;
use crate::lexer::Lexer;
use crate::token::{SpannedToken, Token};

/// Recursive-descent parser for the GQL/Cypher subset.
///
/// Uses a single-token lookahead stored in `current`.
pub struct Parser {
    lexer: Lexer,
    current: SpannedToken,
}

impl Parser {
    /// Create a new parser for the given source text.
    pub fn new(input: &str) -> Result<Self> {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token()?;
        Ok(Self { lexer, current })
    }

    // ───────────────────────────── helpers ────────────────────────────

    /// The current token (by reference).
    fn peek(&self) -> &Token {
        &self.current.0
    }

    /// Current token position (start of span).
    fn pos(&self) -> usize {
        self.current.1.start
    }

    /// Advance to the next token, returning the previous one.
    fn advance(&mut self) -> Result<SpannedToken> {
        let next = self.lexer.next_token()?;
        let prev = std::mem::replace(&mut self.current, next);
        Ok(prev)
    }

    /// If the current token matches `expected`, consume and return it.
    fn eat(&mut self, expected: &Token) -> Result<Option<SpannedToken>> {
        if self.peek() == expected {
            Ok(Some(self.advance()?))
        } else {
            Ok(None)
        }
    }

    /// Consume the current token if it matches `expected`, otherwise error.
    fn expect(&mut self, expected: &Token) -> Result<SpannedToken> {
        if self.peek() == expected {
            self.advance()
        } else {
            Err(self.error(format!("expected {expected:?}, found {:?}", self.peek())))
        }
    }

    /// Consume an `Identifier` token, returning its name.
    fn expect_identifier(&mut self) -> Result<String> {
        if let Token::Identifier(_) = self.peek() {
            let (tok, _) = self.advance()?;
            if let Token::Identifier(name) = tok {
                return Ok(name);
            }
        }
        Err(self.error(format!("expected identifier, found {:?}", self.peek())))
    }

    /// Build a `ParseError` at the current position.
    fn error(&self, message: String) -> AstraeaError {
        AstraeaError::ParseError {
            position: self.pos(),
            message,
        }
    }

    // ──────────────────────── top-level parse ─────────────────────────

    /// Parse a single statement.
    pub fn parse(&mut self) -> Result<Statement> {
        let stmt = match self.peek() {
            Token::Match => self.parse_match()?,
            Token::Create => self.parse_create()?,
            Token::Delete => self.parse_delete()?,
            _ => {
                return Err(self.error(format!(
                    "expected MATCH, CREATE, or DELETE, found {:?}",
                    self.peek()
                )));
            }
        };

        // Allow (but don't require) EOF at end
        // Some callers may chain statements later

        Ok(stmt)
    }

    // ──────────────────────── MATCH ───────────────────────────────────

    fn parse_match(&mut self) -> Result<Statement> {
        self.expect(&Token::Match)?;

        let pattern = self.parse_pattern()?;

        // optional WHERE
        let where_clause = if self.eat(&Token::Where)?.is_some() {
            Some(self.parse_expr()?)
        } else {
            None
        };

        // RETURN
        let return_clause = self.parse_return_clause()?;

        // optional ORDER BY
        let order_by = if self.peek() == &Token::Order {
            self.advance()?;
            self.expect(&Token::By)?;
            let mut items = vec![self.parse_order_item()?];
            while self.eat(&Token::Comma)?.is_some() {
                items.push(self.parse_order_item()?);
            }
            Some(items)
        } else {
            None
        };

        // optional SKIP
        let skip = if self.eat(&Token::Skip)?.is_some() {
            Some(self.parse_u64()?)
        } else {
            None
        };

        // optional LIMIT
        let limit = if self.eat(&Token::Limit)?.is_some() {
            Some(self.parse_u64()?)
        } else {
            None
        };

        Ok(Statement::Match(MatchQuery {
            pattern,
            where_clause,
            return_clause,
            order_by,
            skip,
            limit,
        }))
    }

    fn parse_u64(&mut self) -> Result<u64> {
        if let Token::Integer(n) = self.peek() {
            let n = *n;
            if n < 0 {
                return Err(self.error("expected non-negative integer".into()));
            }
            self.advance()?;
            Ok(n as u64)
        } else {
            Err(self.error(format!("expected integer, found {:?}", self.peek())))
        }
    }

    fn parse_order_item(&mut self) -> Result<OrderItem> {
        let expr = self.parse_expr()?;
        let descending = if self.eat(&Token::Desc)?.is_some() {
            true
        } else {
            let _ = self.eat(&Token::Asc)?;
            false
        };
        Ok(OrderItem { expr, descending })
    }

    fn parse_return_clause(&mut self) -> Result<ReturnClause> {
        self.expect(&Token::Return)?;

        let distinct = self.eat(&Token::Distinct)?.is_some();

        let mut items = vec![self.parse_return_item()?];
        while self.eat(&Token::Comma)?.is_some() {
            items.push(self.parse_return_item()?);
        }

        Ok(ReturnClause { items, distinct })
    }

    fn parse_return_item(&mut self) -> Result<ReturnItem> {
        let expr = self.parse_expr()?;
        let alias = if self.eat(&Token::As)?.is_some() {
            Some(self.expect_identifier()?)
        } else {
            None
        };
        Ok(ReturnItem { expr, alias })
    }

    // ──────────────────────── CREATE ──────────────────────────────────

    fn parse_create(&mut self) -> Result<Statement> {
        self.expect(&Token::Create)?;
        let pattern = self.parse_pattern()?;
        Ok(Statement::Create(CreateStatement { pattern }))
    }

    // ──────────────────────── DELETE ──────────────────────────────────

    fn parse_delete(&mut self) -> Result<Statement> {
        self.expect(&Token::Delete)?;
        let mut variables = vec![self.expect_identifier()?];
        while self.eat(&Token::Comma)?.is_some() {
            variables.push(self.expect_identifier()?);
        }
        Ok(Statement::Delete(DeleteStatement { variables }))
    }

    // ──────────────────────── Pattern parsing ─────────────────────────

    /// Parse a graph pattern: `(a:Label)-[r:TYPE]->(b)` etc.
    fn parse_pattern(&mut self) -> Result<Vec<PatternElement>> {
        let mut elements: Vec<PatternElement> = Vec::new();

        // A pattern always starts with a node
        elements.push(PatternElement::Node(self.parse_node_pattern()?));

        // Then alternating edges and nodes
        loop {
            if let Some(edge) = self.try_parse_edge()? {
                elements.push(PatternElement::Edge(edge));
                elements.push(PatternElement::Node(self.parse_node_pattern()?));
            } else {
                break;
            }
        }

        Ok(elements)
    }

    /// Parse a node pattern: `(variable:Label:Label2 {key: value})`.
    fn parse_node_pattern(&mut self) -> Result<NodePattern> {
        self.expect(&Token::LeftParen)?;

        let mut variable = None;
        let mut labels = Vec::new();
        let mut properties = None;

        // optional variable name
        if let Token::Identifier(_) = self.peek() {
            variable = Some(self.expect_identifier()?);
        }

        // optional labels
        while self.eat(&Token::Colon)?.is_some() {
            labels.push(self.expect_identifier()?);
        }

        // optional inline properties
        if self.peek() == &Token::LeftBrace {
            properties = Some(self.parse_property_map()?);
        }

        self.expect(&Token::RightParen)?;

        Ok(NodePattern {
            variable,
            labels,
            properties,
        })
    }

    /// Try to parse an edge pattern.  Returns `None` if there is no edge
    /// starting at the current position.
    ///
    /// Handles:
    /// - `-[r:TYPE]->` (outgoing)
    /// - `<-[r:TYPE]-` (incoming)
    /// - `-[r:TYPE]-`  (undirected)
    /// - `-[:TYPE]->`  (no variable)
    /// - `-->`         (bare outgoing)
    /// - `<--`         (bare incoming)
    /// - `--`          (bare undirected)
    fn try_parse_edge(&mut self) -> Result<Option<EdgePattern>> {
        // incoming edge: starts with `<-`
        if self.peek() == &Token::LeftArrow {
            self.advance()?; // consume `<-`
            let (variable, edge_types, properties) = self.parse_edge_bracket_contents()?;
            self.expect(&Token::Dash)?;
            return Ok(Some(EdgePattern {
                variable,
                edge_types,
                direction: EdgeDirection::Incoming,
                properties,
            }));
        }

        // outgoing or undirected: starts with `-`
        if self.peek() == &Token::Dash {
            self.advance()?; // consume `-`

            let (variable, edge_types, properties) = self.parse_edge_bracket_contents()?;

            // determine direction by trailing symbol
            if self.eat(&Token::Arrow)?.is_some() {
                // was `->` (but since we already consumed the leading `-`,
                // the lexer sees `->` as Arrow)
                return Ok(Some(EdgePattern {
                    variable,
                    edge_types,
                    direction: EdgeDirection::Outgoing,
                    properties,
                }));
            }

            // for undirected we just need another Dash at the end
            // but only if there was a bracket section
            if variable.is_some() || !edge_types.is_empty() || properties.is_some() {
                self.expect(&Token::Dash)?;
            }

            // peek to see if this is actually `->` (outgoing without bracket)
            if self.peek() == &Token::GreaterThan {
                self.advance()?;
                return Ok(Some(EdgePattern {
                    variable,
                    edge_types,
                    direction: EdgeDirection::Outgoing,
                    properties,
                }));
            }

            return Ok(Some(EdgePattern {
                variable,
                edge_types,
                direction: EdgeDirection::Undirected,
                properties,
            }));
        }

        Ok(None)
    }

    /// Parse the `[r:TYPE {props}]` bracket section of an edge.
    /// Returns (variable, edge_types, properties).
    /// If there is no bracket section, returns (None, vec![], None).
    fn parse_edge_bracket_contents(
        &mut self,
    ) -> Result<(Option<String>, Vec<String>, Option<serde_json::Value>)> {
        if self.eat(&Token::LeftBracket)?.is_none() {
            return Ok((None, Vec::new(), None));
        }

        let mut variable = None;
        let mut edge_types = Vec::new();
        let mut properties = None;

        // optional variable
        if let Token::Identifier(_) = self.peek() {
            variable = Some(self.expect_identifier()?);
        }

        // optional edge type(s)
        while self.eat(&Token::Colon)?.is_some() {
            edge_types.push(self.expect_identifier()?);
        }

        // optional properties
        if self.peek() == &Token::LeftBrace {
            properties = Some(self.parse_property_map()?);
        }

        self.expect(&Token::RightBracket)?;

        Ok((variable, edge_types, properties))
    }

    /// Parse `{key: value, key2: value2}` into a serde_json::Value::Object.
    fn parse_property_map(&mut self) -> Result<serde_json::Value> {
        self.expect(&Token::LeftBrace)?;
        let mut map = serde_json::Map::new();

        if self.peek() != &Token::RightBrace {
            loop {
                let key = self.expect_identifier()?;
                self.expect(&Token::Colon)?;
                let value = self.parse_json_value()?;
                map.insert(key, value);

                if self.eat(&Token::Comma)?.is_none() {
                    break;
                }
            }
        }

        self.expect(&Token::RightBrace)?;
        Ok(serde_json::Value::Object(map))
    }

    /// Parse a simple JSON-like value (string, number, bool, null).
    fn parse_json_value(&mut self) -> Result<serde_json::Value> {
        match self.peek().clone() {
            Token::StringLit(s) => {
                let s = s.clone();
                self.advance()?;
                Ok(serde_json::Value::String(s))
            }
            Token::Integer(n) => {
                let n = n;
                self.advance()?;
                Ok(serde_json::json!(n))
            }
            Token::Float(f) => {
                let f = f;
                self.advance()?;
                Ok(serde_json::json!(f))
            }
            Token::True => {
                self.advance()?;
                Ok(serde_json::Value::Bool(true))
            }
            Token::False => {
                self.advance()?;
                Ok(serde_json::Value::Bool(false))
            }
            Token::Null => {
                self.advance()?;
                Ok(serde_json::Value::Null)
            }
            _ => Err(self.error(format!("expected value, found {:?}", self.peek()))),
        }
    }

    // ──────────────── Expression parsing (precedence climbing) ────────

    /// Parse an expression.  Entry point delegates to `parse_or`.
    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    /// OR has the lowest precedence.
    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.peek() == &Token::Or {
            self.advance()?;
            let right = self.parse_and()?;
            left = Expr::BinaryOp(Box::new(left), BinOp::Or, Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_not()?;
        while self.peek() == &Token::And {
            self.advance()?;
            let right = self.parse_not()?;
            left = Expr::BinaryOp(Box::new(left), BinOp::And, Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if self.peek() == &Token::Not {
            self.advance()?;
            let expr = self.parse_not()?;
            Ok(Expr::UnaryOp(UnOp::Not, Box::new(expr)))
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_addition()?;

        loop {
            let op = match self.peek() {
                Token::Equals => BinOp::Eq,
                Token::NotEquals => BinOp::Neq,
                Token::LessThan => BinOp::Lt,
                Token::LessEqual => BinOp::Lte,
                Token::GreaterThan => BinOp::Gt,
                Token::GreaterEqual => BinOp::Gte,
                Token::Is => {
                    // IS NULL / IS NOT NULL
                    self.advance()?;
                    if self.eat(&Token::Not)?.is_some() {
                        self.expect(&Token::Null)?;
                        left = Expr::IsNotNull(Box::new(left));
                    } else {
                        self.expect(&Token::Null)?;
                        left = Expr::IsNull(Box::new(left));
                    }
                    continue;
                }
                _ => break,
            };
            self.advance()?;
            let right = self.parse_addition()?;
            left = Expr::BinaryOp(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn parse_addition(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Dash => BinOp::Sub,
                _ => break,
            };
            self.advance()?;
            let right = self.parse_multiplication()?;
            left = Expr::BinaryOp(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance()?;
            let right = self.parse_unary()?;
            left = Expr::BinaryOp(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.peek() == &Token::Dash {
            self.advance()?;
            let expr = self.parse_unary()?;
            Ok(Expr::UnaryOp(UnOp::Neg, Box::new(expr)))
        } else {
            self.parse_postfix()
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.peek() == &Token::Dot {
                self.advance()?;
                let prop = self.expect_identifier()?;
                expr = Expr::Property(Box::new(expr), prop);
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Token::Integer(n) => {
                self.advance()?;
                Ok(Expr::Literal(Literal::Integer(n)))
            }
            Token::Float(f) => {
                self.advance()?;
                Ok(Expr::Literal(Literal::Float(f)))
            }
            Token::StringLit(s) => {
                let s = s.clone();
                self.advance()?;
                Ok(Expr::Literal(Literal::String(s)))
            }
            Token::True => {
                self.advance()?;
                Ok(Expr::Literal(Literal::Boolean(true)))
            }
            Token::False => {
                self.advance()?;
                Ok(Expr::Literal(Literal::Boolean(false)))
            }
            Token::Null => {
                self.advance()?;
                Ok(Expr::Literal(Literal::Null))
            }
            Token::Identifier(ref name) => {
                let name = name.clone();
                self.advance()?;

                // Could be a function call: name(...)
                if self.peek() == &Token::LeftParen {
                    self.advance()?; // consume '('
                    let mut args = Vec::new();
                    if self.peek() != &Token::RightParen {
                        args.push(self.parse_expr()?);
                        while self.eat(&Token::Comma)?.is_some() {
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RightParen)?;
                    Ok(Expr::FunctionCall(name, args))
                } else {
                    Ok(Expr::Variable(name))
                }
            }
            Token::Star => {
                // Support `count(*)` — treat `*` as a special variable
                self.advance()?;
                Ok(Expr::Variable("*".into()))
            }
            Token::LeftParen => {
                self.advance()?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RightParen)?;
                Ok(expr)
            }
            _ => Err(self.error(format!("expected expression, found {:?}", self.peek()))),
        }
    }
}

// ───────────────────────────────────────────────────────────
// Tests
// ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Convenience: parse a statement from a string.
    fn parse_stmt(input: &str) -> Statement {
        let mut parser = Parser::new(input).expect("parser init failed");
        parser.parse().expect("parse failed")
    }

    // ─── MATCH ──────────────────────────────────────────────

    #[test]
    fn test_parse_simple_match() {
        let stmt = parse_stmt("MATCH (a:Person) RETURN a");

        let Statement::Match(q) = stmt else {
            panic!("expected Match statement");
        };

        assert_eq!(q.pattern.len(), 1);
        let PatternElement::Node(ref node) = q.pattern[0] else {
            panic!("expected Node");
        };
        assert_eq!(node.variable.as_deref(), Some("a"));
        assert_eq!(node.labels, vec!["Person"]);
        assert!(node.properties.is_none());

        assert!(q.where_clause.is_none());
        assert_eq!(q.return_clause.items.len(), 1);
        assert!(!q.return_clause.distinct);
    }

    #[test]
    fn test_parse_match_with_edge() {
        let stmt = parse_stmt(
            "MATCH (a:Person)-[r:KNOWS]->(b:Person) WHERE a.age > 30 RETURN b.name",
        );

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        // pattern: Node, Edge, Node
        assert_eq!(q.pattern.len(), 3);

        let PatternElement::Edge(ref edge) = q.pattern[1] else {
            panic!("expected Edge");
        };
        assert_eq!(edge.variable.as_deref(), Some("r"));
        assert_eq!(edge.edge_types, vec!["KNOWS"]);
        assert_eq!(edge.direction, EdgeDirection::Outgoing);

        // WHERE a.age > 30
        let Some(ref where_expr) = q.where_clause else {
            panic!("expected WHERE clause");
        };
        match where_expr {
            Expr::BinaryOp(left, BinOp::Gt, right) => {
                assert_eq!(
                    **left,
                    Expr::Property(Box::new(Expr::Variable("a".into())), "age".into())
                );
                assert_eq!(**right, Expr::Literal(Literal::Integer(30)));
            }
            _ => panic!("expected BinaryOp Gt"),
        }

        // RETURN b.name
        assert_eq!(q.return_clause.items.len(), 1);
        assert_eq!(
            q.return_clause.items[0].expr,
            Expr::Property(Box::new(Expr::Variable("b".into())), "name".into())
        );
    }

    #[test]
    fn test_parse_match_with_properties() {
        let stmt = parse_stmt(
            r#"MATCH (a:Person {name: "Alice", active: true}) RETURN a"#,
        );

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        let PatternElement::Node(ref node) = q.pattern[0] else {
            panic!("expected Node");
        };

        let props = node.properties.as_ref().unwrap();
        assert_eq!(props["name"], serde_json::json!("Alice"));
        assert_eq!(props["active"], serde_json::json!(true));
    }

    #[test]
    fn test_parse_match_order_limit_skip() {
        let stmt = parse_stmt(
            "MATCH (a:Person) RETURN a.name ORDER BY a.name DESC SKIP 5 LIMIT 10",
        );

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        let order = q.order_by.as_ref().unwrap();
        assert_eq!(order.len(), 1);
        assert!(order[0].descending);

        assert_eq!(q.skip, Some(5));
        assert_eq!(q.limit, Some(10));
    }

    #[test]
    fn test_parse_match_incoming_edge() {
        let stmt = parse_stmt("MATCH (a)<-[:KNOWS]-(b) RETURN a, b");

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        assert_eq!(q.pattern.len(), 3);
        let PatternElement::Edge(ref edge) = q.pattern[1] else {
            panic!("expected Edge");
        };
        assert_eq!(edge.direction, EdgeDirection::Incoming);
        assert_eq!(edge.edge_types, vec!["KNOWS"]);
    }

    #[test]
    fn test_parse_match_return_distinct() {
        let stmt = parse_stmt("MATCH (a) RETURN DISTINCT a.name");

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        assert!(q.return_clause.distinct);
    }

    // ─── CREATE ─────────────────────────────────────────────

    #[test]
    fn test_parse_create_node() {
        let stmt = parse_stmt(r#"CREATE (a:Person {name: "Alice", age: 30})"#);

        let Statement::Create(c) = stmt else {
            panic!("expected Create");
        };

        assert_eq!(c.pattern.len(), 1);
        let PatternElement::Node(ref node) = c.pattern[0] else {
            panic!("expected Node");
        };
        assert_eq!(node.variable.as_deref(), Some("a"));
        assert_eq!(node.labels, vec!["Person"]);
        let props = node.properties.as_ref().unwrap();
        assert_eq!(props["name"], serde_json::json!("Alice"));
        assert_eq!(props["age"], serde_json::json!(30));
    }

    #[test]
    fn test_parse_create_with_edge() {
        let stmt = parse_stmt(
            r#"CREATE (a:Person {name: "Alice"})-[:KNOWS]->(b:Person {name: "Bob"})"#,
        );

        let Statement::Create(c) = stmt else {
            panic!("expected Create");
        };

        // Node, Edge, Node
        assert_eq!(c.pattern.len(), 3);
    }

    // ─── DELETE ─────────────────────────────────────────────

    #[test]
    fn test_parse_delete() {
        let stmt = parse_stmt("DELETE a, b, c");

        let Statement::Delete(d) = stmt else {
            panic!("expected Delete");
        };

        assert_eq!(d.variables, vec!["a", "b", "c"]);
    }

    // ─── Expressions ────────────────────────────────────────

    #[test]
    fn test_parse_boolean_logic() {
        let stmt = parse_stmt(
            "MATCH (a) WHERE a.x = 1 AND a.y = 2 OR a.z = 3 RETURN a",
        );

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        // OR is lower precedence than AND, so the tree should be:
        // OR( AND( x=1, y=2 ), z=3 )
        let where_expr = q.where_clause.unwrap();
        match where_expr {
            Expr::BinaryOp(_, BinOp::Or, _) => { /* correct top-level */ }
            other => panic!("expected OR at top level, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_is_null() {
        let stmt = parse_stmt("MATCH (a) WHERE a.name IS NULL RETURN a");

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        match q.where_clause.unwrap() {
            Expr::IsNull(inner) => {
                assert_eq!(
                    *inner,
                    Expr::Property(Box::new(Expr::Variable("a".into())), "name".into())
                );
            }
            other => panic!("expected IsNull, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_is_not_null() {
        let stmt = parse_stmt("MATCH (a) WHERE a.name IS NOT NULL RETURN a");

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        match q.where_clause.unwrap() {
            Expr::IsNotNull(inner) => {
                assert_eq!(
                    *inner,
                    Expr::Property(Box::new(Expr::Variable("a".into())), "name".into())
                );
            }
            other => panic!("expected IsNotNull, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_function_call() {
        let stmt = parse_stmt("MATCH (a) RETURN count(a)");

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        let item = &q.return_clause.items[0];
        match &item.expr {
            Expr::FunctionCall(name, args) => {
                assert_eq!(name, "count");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], Expr::Variable("a".into()));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_alias() {
        let stmt = parse_stmt("MATCH (a) RETURN a.name AS person_name");

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        assert_eq!(
            q.return_clause.items[0].alias.as_deref(),
            Some("person_name")
        );
    }

    #[test]
    fn test_parse_arithmetic_precedence() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let stmt = parse_stmt("MATCH (a) RETURN 1 + 2 * 3");

        let Statement::Match(q) = stmt else {
            panic!("expected Match");
        };

        let expr = &q.return_clause.items[0].expr;
        match expr {
            Expr::BinaryOp(left, BinOp::Add, right) => {
                assert_eq!(**left, Expr::Literal(Literal::Integer(1)));
                match right.as_ref() {
                    Expr::BinaryOp(a, BinOp::Mul, b) => {
                        assert_eq!(**a, Expr::Literal(Literal::Integer(2)));
                        assert_eq!(**b, Expr::Literal(Literal::Integer(3)));
                    }
                    other => panic!("expected Mul on RHS, got {other:?}"),
                }
            }
            other => panic!("expected Add at top level, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_error_invalid_start() {
        let mut parser = Parser::new("FOOBAR (a) RETURN a").unwrap();
        let result = parser.parse();
        assert!(result.is_err());
    }
}
