/// Top-level parsed statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Match(MatchQuery),
    Create(CreateStatement),
    Delete(DeleteStatement),
}

/// A MATCH ... WHERE ... RETURN ... ORDER BY ... SKIP ... LIMIT ... query.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchQuery {
    pub pattern: Vec<PatternElement>,
    pub where_clause: Option<Expr>,
    pub return_clause: ReturnClause,
    pub order_by: Option<Vec<OrderItem>>,
    pub skip: Option<u64>,
    pub limit: Option<u64>,
}

/// An element in a graph pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternElement {
    Node(NodePattern),
    Edge(EdgePattern),
}

/// A node pattern, e.g. `(a:Person {name: "Alice"})`.
#[derive(Debug, Clone, PartialEq)]
pub struct NodePattern {
    /// Binding variable, e.g. `a` in `(a:Person)`.
    pub variable: Option<String>,
    /// Labels, e.g. `["Person"]` in `(a:Person)`.
    pub labels: Vec<String>,
    /// Inline property map, e.g. `{name: "Alice"}`.
    pub properties: Option<serde_json::Value>,
}

/// An edge pattern, e.g. `-[r:KNOWS {since: 2020}]->`.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgePattern {
    /// Binding variable, e.g. `r` in `-[r:KNOWS]->`.
    pub variable: Option<String>,
    /// Edge types, e.g. `["KNOWS"]` in `-[:KNOWS]->`.
    pub edge_types: Vec<String>,
    /// Direction of the edge.
    pub direction: EdgeDirection,
    /// Inline property map.
    pub properties: Option<serde_json::Value>,
}

/// Direction of an edge in a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeDirection {
    /// `-[...]->`
    Outgoing,
    /// `<-[...]-`
    Incoming,
    /// `-[...]-`
    Undirected,
}

/// RETURN clause.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnClause {
    pub items: Vec<ReturnItem>,
    pub distinct: bool,
}

/// A single item in a RETURN clause.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnItem {
    pub expr: Expr,
    pub alias: Option<String>,
}

/// Expression AST node.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A variable reference, e.g. `a`.
    Variable(String),
    /// Property access, e.g. `a.name`.
    Property(Box<Expr>, String),
    /// A literal value.
    Literal(Literal),
    /// A binary operation, e.g. `a.age > 30`.
    BinaryOp(Box<Expr>, BinOp, Box<Expr>),
    /// A unary operation, e.g. `NOT expr` or `-expr`.
    UnaryOp(UnOp, Box<Expr>),
    /// A function call, e.g. `count(a)`.
    FunctionCall(String, Vec<Expr>),
    /// `expr IS NULL`.
    IsNull(Box<Expr>),
    /// `expr IS NOT NULL`.
    IsNotNull(Box<Expr>),
}

/// Literal values.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Null,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
}

/// A CREATE statement.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateStatement {
    pub pattern: Vec<PatternElement>,
}

/// A DELETE statement.
#[derive(Debug, Clone, PartialEq)]
pub struct DeleteStatement {
    pub variables: Vec<String>,
}

/// An item in an ORDER BY clause.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem {
    pub expr: Expr,
    pub descending: bool,
}
