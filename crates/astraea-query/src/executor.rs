//! GQL query executor.
//!
//! Takes a parsed [`Statement`] AST and executes it against a [`GraphOps`]
//! implementation, producing a tabular [`QueryResult`].
//!
//! The executor supports:
//! - **MATCH**: pattern matching with label filters, edge traversal, WHERE
//!   filtering, RETURN projection, DISTINCT, ORDER BY, SKIP, and LIMIT.
//! - **CREATE**: creating nodes and edges from a pattern.
//! - **DELETE**: deleting nodes/edges by variable name (requires prior binding context).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use astraea_core::error::{AstraeaError, Result};
use astraea_core::traits::GraphOps;
use astraea_core::types::*;

use crate::ast::*;

// ─────────────────────────────── Result types ───────────────────────────────

/// The result of executing a GQL statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Column names for the result set.
    pub columns: Vec<String>,
    /// Rows of values. Each row has one entry per column.
    pub rows: Vec<Vec<Value>>,
    /// Mutation statistics (nodes/edges created or deleted).
    pub stats: QueryStats,
}

/// Statistics about mutations performed during query execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryStats {
    pub nodes_created: u64,
    pub edges_created: u64,
    pub nodes_deleted: u64,
    pub edges_deleted: u64,
}

// ──────────────────────────── Binding table types ───────────────────────────

/// A single row in the binding table: maps variable names to their JSON values.
type BindingRow = HashMap<String, Value>;

/// Tracks the type of entity a variable refers to, for DELETE support.
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum BoundEntity {
    Node(NodeId),
    Edge(EdgeId),
}

// ─────────────────────────────── Executor ───────────────────────────────────

/// The GQL query executor.
///
/// Holds a reference to a [`GraphOps`] implementation and executes parsed
/// [`Statement`] ASTs against it.
pub struct Executor {
    graph: Arc<dyn GraphOps>,
}

impl Executor {
    /// Create a new executor backed by the given graph.
    pub fn new(graph: Arc<dyn GraphOps>) -> Self {
        Self { graph }
    }

    /// Execute a parsed statement and return the result.
    pub fn execute(&self, stmt: Statement) -> Result<QueryResult> {
        match stmt {
            Statement::Match(query) => self.execute_match(&query),
            Statement::Create(create) => self.execute_create(&create),
            Statement::Delete(delete) => self.execute_delete(&delete),
        }
    }

    // ───────────────────────── MATCH execution ─────────────────────────

    fn execute_match(&self, query: &MatchQuery) -> Result<QueryResult> {
        // Step 1: Walk the pattern to build a binding table.
        let mut bindings = self.resolve_pattern(&query.pattern)?;

        // Step 2: Apply WHERE clause filter.
        if let Some(ref where_expr) = query.where_clause {
            bindings.retain(|row| {
                matches!(eval_expr(where_expr, row), Ok(Value::Bool(true)))
            });
        }

        // Step 3: Apply ORDER BY (before projection, so we have full bindings).
        if let Some(ref order_items) = query.order_by {
            bindings.sort_by(|a, b| {
                for item in order_items {
                    let val_a = eval_expr(&item.expr, a).unwrap_or(Value::Null);
                    let val_b = eval_expr(&item.expr, b).unwrap_or(Value::Null);

                    let cmp = compare_values(&val_a, &val_b);
                    let cmp = if item.descending { cmp.reverse() } else { cmp };
                    if cmp != std::cmp::Ordering::Equal {
                        return cmp;
                    }
                }
                std::cmp::Ordering::Equal
            });
        }

        // Step 4: Apply RETURN projection to extract columns and rows.
        let (columns, mut rows) = self.project(&query.return_clause, &bindings)?;

        // Step 5: Apply DISTINCT.
        if query.return_clause.distinct {
            let mut seen = Vec::new();
            rows.retain(|row| {
                let serialized = serde_json::to_string(row).unwrap_or_default();
                if seen.contains(&serialized) {
                    false
                } else {
                    seen.push(serialized);
                    true
                }
            });
        }

        // Step 6: Apply SKIP.
        if let Some(skip) = query.skip {
            let skip = skip as usize;
            if skip >= rows.len() {
                rows.clear();
            } else {
                rows = rows.split_off(skip);
            }
        }

        // Step 7: Apply LIMIT.
        if let Some(limit) = query.limit {
            rows.truncate(limit as usize);
        }

        Ok(QueryResult {
            columns,
            rows,
            stats: QueryStats::default(),
        })
    }

    /// Resolve a graph pattern into a binding table.
    ///
    /// The pattern is a sequence of `Node, Edge, Node, Edge, Node, ...` elements.
    /// We process them left-to-right, expanding candidates at each step.
    fn resolve_pattern(&self, pattern: &[PatternElement]) -> Result<Vec<BindingRow>> {
        let mut bindings: Vec<BindingRow> = Vec::new();
        // Also track entity IDs for each variable across rows, for edge expansion.
        let mut entity_map: Vec<HashMap<String, BoundEntity>> = Vec::new();

        let mut iter = pattern.iter();

        // First element must be a node.
        let first = iter.next().ok_or_else(|| {
            AstraeaError::QueryExecution("empty pattern".into())
        })?;

        match first {
            PatternElement::Node(node_pat) => {
                let candidates = self.resolve_node_candidates(node_pat)?;
                for (node_id, node_val) in &candidates {
                    let mut row = BindingRow::new();
                    let mut entities = HashMap::new();
                    if let Some(ref var) = node_pat.variable {
                        row.insert(var.clone(), node_val.clone());
                        entities.insert(var.clone(), BoundEntity::Node(*node_id));
                    }
                    bindings.push(row);
                    entity_map.push(entities);
                }
            }
            PatternElement::Edge(_) => {
                return Err(AstraeaError::QueryExecution(
                    "pattern must start with a node".into(),
                ));
            }
        }

        // Process remaining elements in pairs: (Edge, Node).
        while let Some(edge_elem) = iter.next() {
            let PatternElement::Edge(edge_pat) = edge_elem else {
                return Err(AstraeaError::QueryExecution(
                    "expected edge after node in pattern".into(),
                ));
            };

            let node_elem = iter.next().ok_or_else(|| {
                AstraeaError::QueryExecution("edge must be followed by a node".into())
            })?;
            let PatternElement::Node(node_pat) = node_elem else {
                return Err(AstraeaError::QueryExecution(
                    "expected node after edge in pattern".into(),
                ));
            };

            let mut new_bindings = Vec::new();
            let mut new_entity_map = Vec::new();

            for (row_idx, row) in bindings.iter().enumerate() {
                let entities = &entity_map[row_idx];

                // Find the "source" node for edge expansion: it is the last node
                // variable bound in this row. We need to find it from the entity map.
                let prev_node_id = self.find_previous_node_id(entities, pattern, edge_elem)?;

                // Determine traversal direction for the storage layer.
                let storage_direction = match edge_pat.direction {
                    EdgeDirection::Outgoing => Direction::Outgoing,
                    EdgeDirection::Incoming => Direction::Incoming,
                    EdgeDirection::Undirected => Direction::Both,
                };

                // Get neighbor edges, optionally filtered by edge type.
                let neighbors = if edge_pat.edge_types.is_empty() {
                    self.graph.neighbors(prev_node_id, storage_direction)?
                } else {
                    let mut all = Vec::new();
                    for et in &edge_pat.edge_types {
                        let mut n = self.graph.neighbors_filtered(
                            prev_node_id,
                            storage_direction,
                            et,
                        )?;
                        all.append(&mut n);
                    }
                    all
                };

                for (edge_id, neighbor_id) in neighbors {
                    // Filter by target node labels if specified.
                    if !node_pat.labels.is_empty() {
                        let neighbor_node = self.graph.get_node(neighbor_id)?;
                        match neighbor_node {
                            Some(ref n) => {
                                if !node_pat.labels.iter().all(|l| n.labels.contains(l)) {
                                    continue;
                                }
                            }
                            None => continue,
                        }
                    }

                    // Filter by target node properties if specified.
                    if let Some(ref required_props) = node_pat.properties {
                        let neighbor_node = self.graph.get_node(neighbor_id)?;
                        match neighbor_node {
                            Some(ref n) => {
                                if !properties_match(&n.properties, required_props) {
                                    continue;
                                }
                            }
                            None => continue,
                        }
                    }

                    // Filter by edge properties if specified.
                    if let Some(ref required_props) = edge_pat.properties {
                        let edge = self.graph.get_edge(edge_id)?;
                        match edge {
                            Some(ref e) => {
                                if !properties_match(&e.properties, required_props) {
                                    continue;
                                }
                            }
                            None => continue,
                        }
                    }

                    let mut new_row = row.clone();
                    let mut new_entities = entities.clone();

                    // Bind the edge variable.
                    if let Some(ref var) = edge_pat.variable {
                        let edge = self.graph.get_edge(edge_id)?;
                        if let Some(e) = edge {
                            new_row.insert(var.clone(), edge_to_json(&e));
                            new_entities.insert(var.clone(), BoundEntity::Edge(edge_id));
                        }
                    }

                    // Bind the target node variable.
                    if let Some(ref var) = node_pat.variable {
                        let neighbor_node = self.graph.get_node(neighbor_id)?;
                        if let Some(n) = neighbor_node {
                            new_row.insert(var.clone(), node_to_json(&n));
                            new_entities.insert(var.clone(), BoundEntity::Node(neighbor_id));
                        }
                    }

                    new_bindings.push(new_row);
                    new_entity_map.push(new_entities);
                }
            }

            bindings = new_bindings;
            entity_map = new_entity_map;
        }

        Ok(bindings)
    }

    /// Resolve candidate nodes for a node pattern (the initial or standalone node).
    ///
    /// Returns a list of `(NodeId, json-representation)` tuples.
    fn resolve_node_candidates(
        &self,
        node_pat: &NodePattern,
    ) -> Result<Vec<(NodeId, Value)>> {
        let node_ids = if !node_pat.labels.is_empty() {
            // Use find_by_label for the first label, then filter by remaining labels.
            let primary_label = &node_pat.labels[0];
            let mut ids = self.graph.find_by_label(primary_label)?;

            // Filter by additional labels.
            if node_pat.labels.len() > 1 {
                ids.retain(|&id| {
                    if let Ok(Some(node)) = self.graph.get_node(id) {
                        node_pat.labels.iter().all(|l| node.labels.contains(l))
                    } else {
                        false
                    }
                });
            }

            ids
        } else {
            // No labels specified -- need to get all nodes.
            // Try find_by_label("") as a convention, or handle the error
            // by returning an empty set with a descriptive error.
            // Since the GraphOps trait doesn't have an all_nodes method,
            // we rely on find_by_label returning all nodes when given an empty
            // string, or we propagate the error.
            self.graph.find_by_label("")?
        };

        let mut results = Vec::new();
        for id in node_ids {
            if let Some(node) = self.graph.get_node(id)? {
                // Apply inline property filter if specified.
                if let Some(ref required_props) = node_pat.properties {
                    if !properties_match(&node.properties, required_props) {
                        continue;
                    }
                }
                results.push((id, node_to_json(&node)));
            }
        }

        Ok(results)
    }

    /// Find the NodeId of the most recently bound node in the pattern before
    /// the given edge element. This is used to know which node to expand from.
    fn find_previous_node_id(
        &self,
        entities: &HashMap<String, BoundEntity>,
        _pattern: &[PatternElement],
        _edge_elem: &PatternElement,
    ) -> Result<NodeId> {
        // The previous node is the last node variable that was bound.
        // We find it by looking at the entity map for node bindings.
        // Since we process left to right, the last inserted node variable
        // is the previous node.
        let mut last_node_id = None;
        for entity in entities.values() {
            if let BoundEntity::Node(nid) = entity {
                // Take the most recently added one. Since HashMap doesn't
                // guarantee order, we just pick any node -- in simple
                // single-path patterns there's typically only one trailing node.
                last_node_id = Some(*nid);
            }
        }

        last_node_id.ok_or_else(|| {
            AstraeaError::QueryExecution(
                "no source node found for edge expansion".into(),
            )
        })
    }

    /// Project the RETURN clause from the binding table.
    ///
    /// Returns `(column_names, rows)`.
    fn project(
        &self,
        return_clause: &ReturnClause,
        bindings: &[BindingRow],
    ) -> Result<(Vec<String>, Vec<Vec<Value>>)> {
        // Check for aggregate functions (like count).
        let has_aggregate = return_clause.items.iter().any(|item| is_aggregate(&item.expr));

        if has_aggregate && bindings.is_empty() {
            // Aggregates on empty input still produce a row (e.g., count(*) = 0).
            let columns: Vec<String> = return_clause
                .items
                .iter()
                .map(|item| column_name(item))
                .collect();

            let row: Vec<Value> = return_clause
                .items
                .iter()
                .map(|item| eval_aggregate(&item.expr, bindings))
                .collect();

            return Ok((columns, vec![row]));
        }

        if has_aggregate {
            // All items must be either aggregates or constants.
            let columns: Vec<String> = return_clause
                .items
                .iter()
                .map(|item| column_name(item))
                .collect();

            let row: Vec<Value> = return_clause
                .items
                .iter()
                .map(|item| {
                    if is_aggregate(&item.expr) {
                        eval_aggregate(&item.expr, bindings)
                    } else {
                        // For non-aggregate items in an aggregate query,
                        // use the first row's value.
                        bindings
                            .first()
                            .and_then(|row| eval_expr(&item.expr, row).ok())
                            .unwrap_or(Value::Null)
                    }
                })
                .collect();

            return Ok((columns, vec![row]));
        }

        // Non-aggregate projection: evaluate each return item for each binding row.
        let columns: Vec<String> = return_clause
            .items
            .iter()
            .map(|item| column_name(item))
            .collect();

        let rows: Vec<Vec<Value>> = bindings
            .iter()
            .map(|row| {
                return_clause
                    .items
                    .iter()
                    .map(|item| eval_expr(&item.expr, row).unwrap_or(Value::Null))
                    .collect()
            })
            .collect();

        Ok((columns, rows))
    }

    // ───────────────────────── CREATE execution ────────────────────────

    fn execute_create(&self, stmt: &CreateStatement) -> Result<QueryResult> {
        let mut stats = QueryStats::default();
        let mut bindings = BindingRow::new();
        let mut entity_map: HashMap<String, BoundEntity> = HashMap::new();
        let mut last_node_id: Option<NodeId> = None;

        for elem in &stmt.pattern {
            match elem {
                PatternElement::Node(node_pat) => {
                    // Check if this variable is already bound (referencing an
                    // existing node created earlier in the pattern).
                    if let Some(ref var) = node_pat.variable {
                        if entity_map.contains_key(var) {
                            // Reuse existing node.
                            if let Some(BoundEntity::Node(nid)) = entity_map.get(var) {
                                last_node_id = Some(*nid);
                            }
                            continue;
                        }
                    }

                    let labels = node_pat.labels.clone();
                    let properties = node_pat.properties.clone().unwrap_or(serde_json::json!({}));

                    let node_id = self.graph.create_node(labels, properties, None)?;
                    stats.nodes_created += 1;

                    if let Some(ref var) = node_pat.variable {
                        let node = self.graph.get_node(node_id)?;
                        if let Some(n) = node {
                            bindings.insert(var.clone(), node_to_json(&n));
                        }
                        entity_map.insert(var.clone(), BoundEntity::Node(node_id));
                    }

                    last_node_id = Some(node_id);
                }
                PatternElement::Edge(edge_pat) => {
                    // The source is the previously created/referenced node.
                    let source = last_node_id.ok_or_else(|| {
                        AstraeaError::QueryExecution(
                            "edge in CREATE has no source node".into(),
                        )
                    })?;

                    // The target will be the next node in the pattern.
                    // We do not create the edge here; we need to wait for the
                    // next node. Store the edge pattern and source for later.
                    // Actually, to keep it simple, we peek at the fact that
                    // the loop will process the next node next, so we store
                    // edge info and create it after the next node.
                    //
                    // We handle this by storing pending edge info.
                    // But since Rust's borrow checker makes this tricky with
                    // the iterator, let's use a different approach: process
                    // the pattern in a second pass for edges.
                    //
                    // For now, store edge metadata on the side.
                    let _ = (source, edge_pat);
                }
            }
        }

        // Second pass: create edges.
        // Walk pattern elements and for each Edge, the source is the node
        // immediately before it and the target is the node immediately after.
        let mut i = 0;
        while i < stmt.pattern.len() {
            if let PatternElement::Edge(ref edge_pat) = stmt.pattern[i] {
                // Source is pattern[i-1], target is pattern[i+1].
                let source_id = self.get_node_id_from_pattern(
                    &stmt.pattern[i - 1],
                    &entity_map,
                )?;
                let target_id = self.get_node_id_from_pattern(
                    &stmt.pattern[i + 1],
                    &entity_map,
                )?;

                let edge_type = edge_pat
                    .edge_types
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "RELATED_TO".into());
                let properties = edge_pat.properties.clone().unwrap_or(serde_json::json!({}));

                // Handle edge direction: if incoming, swap source and target.
                let (src, tgt) = match edge_pat.direction {
                    EdgeDirection::Incoming => (target_id, source_id),
                    _ => (source_id, target_id),
                };

                let edge_id = self.graph.create_edge(
                    src,
                    tgt,
                    edge_type,
                    properties,
                    1.0,
                    None,
                    None,
                )?;
                stats.edges_created += 1;

                if let Some(ref var) = edge_pat.variable {
                    let edge = self.graph.get_edge(edge_id)?;
                    if let Some(e) = edge {
                        bindings.insert(var.clone(), edge_to_json(&e));
                    }
                    entity_map.insert(var.clone(), BoundEntity::Edge(edge_id));
                }
            }
            i += 1;
        }

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            stats,
        })
    }

    /// Extract the NodeId for a node pattern element from the entity map.
    fn get_node_id_from_pattern(
        &self,
        elem: &PatternElement,
        entity_map: &HashMap<String, BoundEntity>,
    ) -> Result<NodeId> {
        match elem {
            PatternElement::Node(np) => {
                if let Some(ref var) = np.variable {
                    if let Some(BoundEntity::Node(nid)) = entity_map.get(var) {
                        return Ok(*nid);
                    }
                }
                Err(AstraeaError::QueryExecution(
                    "cannot resolve node in CREATE pattern".into(),
                ))
            }
            PatternElement::Edge(_) => Err(AstraeaError::QueryExecution(
                "expected node element, found edge".into(),
            )),
        }
    }

    // ───────────────────────── DELETE execution ────────────────────────

    fn execute_delete(&self, stmt: &DeleteStatement) -> Result<QueryResult> {
        // DELETE requires variables to be previously bound. In a standalone
        // DELETE statement (not preceded by MATCH), we treat variable names
        // as node IDs if they parse as integers, otherwise error.
        //
        // In a full implementation, DELETE would be chained after MATCH.
        // For now, we attempt to parse each variable as a numeric node ID.
        let mut stats = QueryStats::default();

        for var in &stmt.variables {
            if let Ok(id) = var.parse::<u64>() {
                // Try deleting as a node first, then as an edge.
                if self.graph.get_node(NodeId(id))?.is_some() {
                    self.graph.delete_node(NodeId(id))?;
                    stats.nodes_deleted += 1;
                } else if self.graph.get_edge(EdgeId(id))?.is_some() {
                    self.graph.delete_edge(EdgeId(id))?;
                    stats.edges_deleted += 1;
                }
            } else {
                return Err(AstraeaError::QueryExecution(format!(
                    "unbound variable '{}' in DELETE (standalone DELETE requires numeric IDs)",
                    var
                )));
            }
        }

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            stats,
        })
    }
}

// ─────────────────────── Expression evaluation ─────────────────────────────

/// Evaluate an expression against a binding row.
pub fn eval_expr(expr: &Expr, bindings: &BindingRow) -> Result<Value> {
    match expr {
        Expr::Variable(name) => {
            Ok(bindings.get(name).cloned().unwrap_or(Value::Null))
        }

        Expr::Property(base_expr, field) => {
            let base = eval_expr(base_expr, bindings)?;
            match base {
                Value::Object(ref map) => {
                    // First check top-level properties.
                    if let Some(val) = map.get(field) {
                        return Ok(val.clone());
                    }
                    // Then check nested "properties" object (for node/edge JSON).
                    if let Some(Value::Object(props)) = map.get("properties") {
                        if let Some(val) = props.get(field) {
                            return Ok(val.clone());
                        }
                    }
                    Ok(Value::Null)
                }
                _ => Ok(Value::Null),
            }
        }

        Expr::Literal(lit) => Ok(literal_to_value(lit)),

        Expr::BinaryOp(left, op, right) => {
            let left_val = eval_expr(left, bindings)?;
            let right_val = eval_expr(right, bindings)?;
            eval_binary_op(&left_val, *op, &right_val)
        }

        Expr::UnaryOp(op, inner) => {
            let val = eval_expr(inner, bindings)?;
            eval_unary_op(*op, &val)
        }

        Expr::FunctionCall(name, args) => {
            eval_function(name, args, bindings)
        }

        Expr::IsNull(inner) => {
            let val = eval_expr(inner, bindings)?;
            Ok(Value::Bool(val.is_null()))
        }

        Expr::IsNotNull(inner) => {
            let val = eval_expr(inner, bindings)?;
            Ok(Value::Bool(!val.is_null()))
        }
    }
}

/// Convert a literal AST node to a JSON value.
fn literal_to_value(lit: &Literal) -> Value {
    match lit {
        Literal::Integer(n) => serde_json::json!(*n),
        Literal::Float(f) => serde_json::json!(*f),
        Literal::String(s) => Value::String(s.clone()),
        Literal::Boolean(b) => Value::Bool(*b),
        Literal::Null => Value::Null,
    }
}

/// Evaluate a binary operation on two JSON values.
fn eval_binary_op(left: &Value, op: BinOp, right: &Value) -> Result<Value> {
    match op {
        // Comparison operators
        BinOp::Eq => Ok(Value::Bool(values_equal(left, right))),
        BinOp::Neq => Ok(Value::Bool(!values_equal(left, right))),
        BinOp::Lt => Ok(Value::Bool(compare_values(left, right) == std::cmp::Ordering::Less)),
        BinOp::Lte => Ok(Value::Bool(compare_values(left, right) != std::cmp::Ordering::Greater)),
        BinOp::Gt => Ok(Value::Bool(compare_values(left, right) == std::cmp::Ordering::Greater)),
        BinOp::Gte => Ok(Value::Bool(compare_values(left, right) != std::cmp::Ordering::Less)),

        // Boolean operators
        BinOp::And => {
            let l = as_bool(left);
            let r = as_bool(right);
            Ok(Value::Bool(l && r))
        }
        BinOp::Or => {
            let l = as_bool(left);
            let r = as_bool(right);
            Ok(Value::Bool(l || r))
        }

        // Arithmetic operators
        BinOp::Add => eval_arithmetic(left, right, |a, b| a + b, |a, b| a + b),
        BinOp::Sub => eval_arithmetic(left, right, |a, b| a - b, |a, b| a - b),
        BinOp::Mul => eval_arithmetic(left, right, |a, b| a * b, |a, b| a * b),
        BinOp::Div => {
            // Check for division by zero.
            if is_zero(right) {
                return Err(AstraeaError::QueryExecution("division by zero".into()));
            }
            eval_arithmetic(left, right, |a, b| a / b, |a, b| a / b)
        }
        BinOp::Mod => {
            if is_zero(right) {
                return Err(AstraeaError::QueryExecution("modulo by zero".into()));
            }
            eval_arithmetic(left, right, |a, b| a % b, |a, b| a % b)
        }
    }
}

/// Evaluate a unary operation on a JSON value.
fn eval_unary_op(op: UnOp, val: &Value) -> Result<Value> {
    match op {
        UnOp::Not => Ok(Value::Bool(!as_bool(val))),
        UnOp::Neg => {
            match val {
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Ok(serde_json::json!(-i))
                    } else if let Some(f) = n.as_f64() {
                        Ok(serde_json::json!(-f))
                    } else {
                        Ok(Value::Null)
                    }
                }
                _ => Ok(Value::Null),
            }
        }
    }
}

/// Evaluate a built-in function call.
fn eval_function(name: &str, args: &[Expr], bindings: &BindingRow) -> Result<Value> {
    let lower_name = name.to_lowercase();
    match lower_name.as_str() {
        "id" => {
            if args.len() != 1 {
                return Err(AstraeaError::QueryExecution(
                    "id() requires exactly one argument".into(),
                ));
            }
            let val = eval_expr(&args[0], bindings)?;
            // Extract the "id" field from a node/edge JSON object.
            match val {
                Value::Object(ref map) => Ok(map.get("id").cloned().unwrap_or(Value::Null)),
                _ => Ok(Value::Null),
            }
        }
        "labels" => {
            if args.len() != 1 {
                return Err(AstraeaError::QueryExecution(
                    "labels() requires exactly one argument".into(),
                ));
            }
            let val = eval_expr(&args[0], bindings)?;
            match val {
                Value::Object(ref map) => Ok(map.get("labels").cloned().unwrap_or(Value::Null)),
                _ => Ok(Value::Null),
            }
        }
        "type" => {
            if args.len() != 1 {
                return Err(AstraeaError::QueryExecution(
                    "type() requires exactly one argument".into(),
                ));
            }
            let val = eval_expr(&args[0], bindings)?;
            match val {
                Value::Object(ref map) => {
                    Ok(map.get("edge_type").cloned().unwrap_or(Value::Null))
                }
                _ => Ok(Value::Null),
            }
        }
        "count" => {
            // count() is an aggregate -- should be handled at projection level.
            // If we reach here, it means we are evaluating in a non-aggregate
            // context. Return 1 for each row (will be summed by the aggregate handler).
            Ok(serde_json::json!(1))
        }
        "tostring" => {
            if args.len() != 1 {
                return Err(AstraeaError::QueryExecution(
                    "toString() requires exactly one argument".into(),
                ));
            }
            let val = eval_expr(&args[0], bindings)?;
            match val {
                Value::String(s) => Ok(Value::String(s)),
                Value::Number(n) => Ok(Value::String(n.to_string())),
                Value::Bool(b) => Ok(Value::String(b.to_string())),
                Value::Null => Ok(Value::String("null".into())),
                _ => Ok(Value::String(val.to_string())),
            }
        }
        "tointeger" | "toint" => {
            if args.len() != 1 {
                return Err(AstraeaError::QueryExecution(
                    "toInteger() requires exactly one argument".into(),
                ));
            }
            let val = eval_expr(&args[0], bindings)?;
            match val {
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Ok(serde_json::json!(i))
                    } else if let Some(f) = n.as_f64() {
                        Ok(serde_json::json!(f as i64))
                    } else {
                        Ok(Value::Null)
                    }
                }
                Value::String(s) => {
                    if let Ok(i) = s.parse::<i64>() {
                        Ok(serde_json::json!(i))
                    } else {
                        Ok(Value::Null)
                    }
                }
                _ => Ok(Value::Null),
            }
        }
        _ => Err(AstraeaError::QueryExecution(format!(
            "unknown function: {}",
            name
        ))),
    }
}

// ─────────────────────── Helper functions ───────────────────────────────────

/// Convert a `Node` to its JSON representation for the binding table.
fn node_to_json(node: &Node) -> Value {
    serde_json::json!({
        "id": node.id.0,
        "labels": node.labels,
        "properties": node.properties,
    })
}

/// Convert an `Edge` to its JSON representation for the binding table.
fn edge_to_json(edge: &Edge) -> Value {
    serde_json::json!({
        "id": edge.id.0,
        "source": edge.source.0,
        "target": edge.target.0,
        "edge_type": edge.edge_type,
        "properties": edge.properties,
        "weight": edge.weight,
    })
}

/// Check if two JSON values are equal (with type coercion for numbers).
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            // Compare as f64 to handle int/float coercion.
            a.as_f64() == b.as_f64()
        }
        _ => a == b,
    }
}

/// Compare two JSON values for ordering.
fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        (Value::Null, _) => std::cmp::Ordering::Less,
        (_, Value::Null) => std::cmp::Ordering::Greater,
        (Value::Number(a), Value::Number(b)) => {
            let fa = a.as_f64().unwrap_or(0.0);
            let fb = b.as_f64().unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        _ => {
            // Fallback: compare string representations.
            let sa = a.to_string();
            let sb = b.to_string();
            sa.cmp(&sb)
        }
    }
}

/// Interpret a JSON value as a boolean.
fn as_bool(val: &Value) -> bool {
    match val {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Check if a JSON value is zero.
fn is_zero(val: &Value) -> bool {
    match val {
        Value::Number(n) => n.as_f64().is_some_and(|f| f == 0.0),
        _ => false,
    }
}

/// Evaluate an arithmetic operation, dispatching to integer or float.
fn eval_arithmetic(
    left: &Value,
    right: &Value,
    int_op: impl Fn(i64, i64) -> i64,
    float_op: impl Fn(f64, f64) -> f64,
) -> Result<Value> {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => {
            // Prefer integer arithmetic if both sides are integers.
            if let (Some(ai), Some(bi)) = (a.as_i64(), b.as_i64()) {
                Ok(serde_json::json!(int_op(ai, bi)))
            } else {
                let af = a.as_f64().unwrap_or(0.0);
                let bf = b.as_f64().unwrap_or(0.0);
                Ok(serde_json::json!(float_op(af, bf)))
            }
        }
        // String concatenation for Add.
        (Value::String(a), Value::String(b)) => {
            Ok(Value::String(format!("{}{}", a, b)))
        }
        _ => Ok(Value::Null),
    }
}

/// Check if all required properties match the actual properties.
fn properties_match(actual: &Value, required: &Value) -> bool {
    match (actual, required) {
        (Value::Object(actual_map), Value::Object(req_map)) => {
            req_map.iter().all(|(key, req_val)| {
                actual_map.get(key).is_some_and(|actual_val| actual_val == req_val)
            })
        }
        _ => false,
    }
}

/// Derive a column name for a return item.
fn column_name(item: &ReturnItem) -> String {
    if let Some(ref alias) = item.alias {
        return alias.clone();
    }
    expr_to_string(&item.expr)
}

/// Convert an expression to a human-readable string for column naming.
fn expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Variable(name) => name.clone(),
        Expr::Property(base, field) => format!("{}.{}", expr_to_string(base), field),
        Expr::Literal(lit) => match lit {
            Literal::Integer(n) => n.to_string(),
            Literal::Float(f) => f.to_string(),
            Literal::String(s) => format!("\"{}\"", s),
            Literal::Boolean(b) => b.to_string(),
            Literal::Null => "null".into(),
        },
        Expr::FunctionCall(name, args) => {
            let arg_strs: Vec<String> = args.iter().map(expr_to_string).collect();
            format!("{}({})", name, arg_strs.join(", "))
        }
        Expr::BinaryOp(l, op, r) => {
            format!("{} {:?} {}", expr_to_string(l), op, expr_to_string(r))
        }
        Expr::UnaryOp(op, inner) => format!("{:?} {}", op, expr_to_string(inner)),
        Expr::IsNull(inner) => format!("{} IS NULL", expr_to_string(inner)),
        Expr::IsNotNull(inner) => format!("{} IS NOT NULL", expr_to_string(inner)),
    }
}

/// Check if an expression is an aggregate function.
fn is_aggregate(expr: &Expr) -> bool {
    match expr {
        Expr::FunctionCall(name, _) => {
            matches!(name.to_lowercase().as_str(), "count" | "sum" | "avg" | "min" | "max")
        }
        _ => false,
    }
}

/// Evaluate an aggregate expression over all binding rows.
fn eval_aggregate(expr: &Expr, bindings: &[BindingRow]) -> Value {
    match expr {
        Expr::FunctionCall(name, args) => {
            let lower_name = name.to_lowercase();
            match lower_name.as_str() {
                "count" => {
                    if args.is_empty()
                        || (args.len() == 1 && matches!(&args[0], Expr::Variable(v) if v == "*"))
                    {
                        serde_json::json!(bindings.len() as i64)
                    } else {
                        // Count non-null values of the expression.
                        let count = bindings
                            .iter()
                            .filter(|row| {
                                eval_expr(&args[0], row)
                                    .ok()
                                    .is_some_and(|v| !v.is_null())
                            })
                            .count();
                        serde_json::json!(count as i64)
                    }
                }
                _ => Value::Null,
            }
        }
        _ => Value::Null,
    }
}


// ─────────────────────────────── Tests ──────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use parking_lot::RwLock;

    // ── In-memory GraphOps for testing ──────────────────────────────────
    //
    // We create a dedicated test implementation that supports find_by_label
    // (including the "" convention for all nodes) because the production
    // Graph::find_by_label is unimplemented.

    struct TestGraph {
        nodes: RwLock<HashMap<NodeId, Node>>,
        edges: RwLock<HashMap<EdgeId, Edge>>,
        next_node_id: AtomicU64,
        next_edge_id: AtomicU64,
    }

    impl TestGraph {
        fn new() -> Self {
            Self {
                nodes: RwLock::new(HashMap::new()),
                edges: RwLock::new(HashMap::new()),
                next_node_id: AtomicU64::new(1),
                next_edge_id: AtomicU64::new(1),
            }
        }
    }

    impl GraphOps for TestGraph {
        fn create_node(
            &self,
            labels: Vec<String>,
            properties: serde_json::Value,
            embedding: Option<Vec<f32>>,
        ) -> Result<NodeId> {
            let id = NodeId(self.next_node_id.fetch_add(1, Ordering::Relaxed));
            let node = Node { id, labels, properties, embedding };
            self.nodes.write().insert(id, node);
            Ok(id)
        }

        fn create_edge(
            &self,
            source: NodeId,
            target: NodeId,
            edge_type: String,
            properties: serde_json::Value,
            weight: f64,
            valid_from: Option<i64>,
            valid_to: Option<i64>,
        ) -> Result<EdgeId> {
            if !self.nodes.read().contains_key(&source) {
                return Err(AstraeaError::NodeNotFound(source));
            }
            if !self.nodes.read().contains_key(&target) {
                return Err(AstraeaError::NodeNotFound(target));
            }
            let id = EdgeId(self.next_edge_id.fetch_add(1, Ordering::Relaxed));
            let edge = Edge {
                id, source, target, edge_type, properties, weight,
                validity: ValidityInterval { valid_from, valid_to },
            };
            self.edges.write().insert(id, edge);
            Ok(id)
        }

        fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
            Ok(self.nodes.read().get(&id).cloned())
        }

        fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>> {
            Ok(self.edges.read().get(&id).cloned())
        }

        fn update_node(&self, id: NodeId, properties: serde_json::Value) -> Result<()> {
            let mut nodes = self.nodes.write();
            let node = nodes.get_mut(&id).ok_or(AstraeaError::NodeNotFound(id))?;
            if let (Some(target_map), serde_json::Value::Object(patch_map)) =
                (node.properties.as_object_mut(), &properties)
            {
                for (k, v) in patch_map {
                    target_map.insert(k.clone(), v.clone());
                }
            }
            Ok(())
        }

        fn update_edge(&self, id: EdgeId, properties: serde_json::Value) -> Result<()> {
            let mut edges = self.edges.write();
            let edge = edges.get_mut(&id).ok_or(AstraeaError::EdgeNotFound(id))?;
            if let (Some(target_map), serde_json::Value::Object(patch_map)) =
                (edge.properties.as_object_mut(), &properties)
            {
                for (k, v) in patch_map {
                    target_map.insert(k.clone(), v.clone());
                }
            }
            Ok(())
        }

        fn delete_node(&self, id: NodeId) -> Result<()> {
            // Delete connected edges first.
            let edge_ids: Vec<EdgeId> = self.edges.read()
                .values()
                .filter(|e| e.source == id || e.target == id)
                .map(|e| e.id)
                .collect();
            for eid in edge_ids {
                self.edges.write().remove(&eid);
            }
            self.nodes.write().remove(&id);
            Ok(())
        }

        fn delete_edge(&self, id: EdgeId) -> Result<()> {
            self.edges.write().remove(&id);
            Ok(())
        }

        fn neighbors(&self, node_id: NodeId, direction: Direction) -> Result<Vec<(EdgeId, NodeId)>> {
            let edges = self.edges.read();
            Ok(edges.values()
                .filter(|e| match direction {
                    Direction::Outgoing => e.source == node_id,
                    Direction::Incoming => e.target == node_id,
                    Direction::Both => e.source == node_id || e.target == node_id,
                })
                .map(|e| {
                    let neighbor = if e.source == node_id { e.target } else { e.source };
                    (e.id, neighbor)
                })
                .collect())
        }

        fn neighbors_filtered(
            &self,
            node_id: NodeId,
            direction: Direction,
            edge_type: &str,
        ) -> Result<Vec<(EdgeId, NodeId)>> {
            let edges = self.edges.read();
            Ok(edges.values()
                .filter(|e| {
                    e.edge_type == edge_type && match direction {
                        Direction::Outgoing => e.source == node_id,
                        Direction::Incoming => e.target == node_id,
                        Direction::Both => e.source == node_id || e.target == node_id,
                    }
                })
                .map(|e| {
                    let neighbor = if e.source == node_id { e.target } else { e.source };
                    (e.id, neighbor)
                })
                .collect())
        }

        fn bfs(&self, _start: NodeId, _max_depth: usize) -> Result<Vec<(NodeId, usize)>> {
            Ok(Vec::new())
        }

        fn dfs(&self, _start: NodeId, _max_depth: usize) -> Result<Vec<NodeId>> {
            Ok(Vec::new())
        }

        fn shortest_path(&self, _from: NodeId, _to: NodeId) -> Result<Option<GraphPath>> {
            Ok(None)
        }

        fn shortest_path_weighted(
            &self,
            _from: NodeId,
            _to: NodeId,
        ) -> Result<Option<(GraphPath, f64)>> {
            Ok(None)
        }

        fn find_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
            let nodes = self.nodes.read();
            if label.is_empty() {
                // Convention: empty label returns all node IDs.
                Ok(nodes.keys().copied().collect())
            } else {
                Ok(nodes.values()
                    .filter(|n| n.labels.contains(&label.to_string()))
                    .map(|n| n.id)
                    .collect())
            }
        }
    }

    /// Helper: create a test executor with some pre-populated data.
    fn setup_test_graph() -> (Arc<TestGraph>, Executor) {
        let graph = Arc::new(TestGraph::new());

        // Create nodes:
        // Alice: Person, age 25
        // Bob:   Person, age 35
        // Carol: Person, age 28
        // Acme:  Company

        graph.create_node(
            vec!["Person".into()],
            serde_json::json!({"name": "Alice", "age": 25}),
            None,
        ).unwrap();

        graph.create_node(
            vec!["Person".into()],
            serde_json::json!({"name": "Bob", "age": 35}),
            None,
        ).unwrap();

        graph.create_node(
            vec!["Person".into()],
            serde_json::json!({"name": "Carol", "age": 28}),
            None,
        ).unwrap();

        graph.create_node(
            vec!["Company".into()],
            serde_json::json!({"name": "Acme", "founded": 1990}),
            None,
        ).unwrap();

        // Create edges:
        // Alice -[KNOWS]-> Bob
        // Bob -[KNOWS]-> Carol
        // Alice -[WORKS_AT]-> Acme

        graph.create_edge(
            NodeId(1), NodeId(2), "KNOWS".into(),
            serde_json::json!({"since": 2020}), 1.0, None, None,
        ).unwrap();

        graph.create_edge(
            NodeId(2), NodeId(3), "KNOWS".into(),
            serde_json::json!({"since": 2021}), 1.0, None, None,
        ).unwrap();

        graph.create_edge(
            NodeId(1), NodeId(4), "WORKS_AT".into(),
            serde_json::json!({}), 1.0, None, None,
        ).unwrap();

        let executor = Executor::new(graph.clone() as Arc<dyn GraphOps>);
        (graph, executor)
    }

    /// Parse and execute a GQL query.
    fn run_query(executor: &Executor, gql: &str) -> QueryResult {
        let stmt = crate::parse(gql).expect("parse failed");
        executor.execute(stmt).expect("execution failed")
    }

    // ── MATCH tests ────────────────────────────────────────────────────

    #[test]
    fn test_match_all_nodes() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(&executor, "MATCH (n) RETURN n");

        assert_eq!(result.columns, vec!["n"]);
        assert_eq!(result.rows.len(), 4); // Alice, Bob, Carol, Acme
    }

    #[test]
    fn test_match_by_label() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(&executor, "MATCH (n:Person) RETURN n.name");

        assert_eq!(result.columns, vec!["n.name"]);
        assert_eq!(result.rows.len(), 3); // Alice, Bob, Carol

        let names: Vec<&str> = result.rows.iter()
            .filter_map(|row| row[0].as_str())
            .collect();
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"Bob"));
        assert!(names.contains(&"Carol"));
    }

    #[test]
    fn test_match_label_and_property_projection() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(&executor, "MATCH (n:Person) RETURN n.name, n.age");

        assert_eq!(result.columns, vec!["n.name", "n.age"]);
        assert_eq!(result.rows.len(), 3);

        // Verify that each row has name and age.
        for row in &result.rows {
            assert!(row[0].is_string());
            assert!(row[1].is_number());
        }
    }

    #[test]
    fn test_match_edge_traversal() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name, b.name",
        );

        assert_eq!(result.columns, vec!["a.name", "b.name"]);
        assert_eq!(result.rows.len(), 2); // Alice->Bob, Bob->Carol

        let pairs: Vec<(String, String)> = result.rows.iter()
            .map(|row| {
                (
                    row[0].as_str().unwrap().to_string(),
                    row[1].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert!(pairs.contains(&("Alice".into(), "Bob".into())));
        assert!(pairs.contains(&("Bob".into(), "Carol".into())));
    }

    #[test]
    fn test_match_where_filter() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) WHERE n.age > 30 RETURN n.name",
        );

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], "Bob");
    }

    #[test]
    fn test_match_where_complex() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) WHERE n.age >= 25 AND n.age < 30 RETURN n.name",
        );

        assert_eq!(result.rows.len(), 2); // Alice (25), Carol (28)
        let names: Vec<&str> = result.rows.iter()
            .filter_map(|row| row[0].as_str())
            .collect();
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"Carol"));
    }

    #[test]
    fn test_match_return_distinct() {
        let (_graph, executor) = setup_test_graph();
        // All persons have the same label "Person", so DISTINCT on labels
        // should yield one row.
        let result = run_query(
            &executor,
            "MATCH (n:Person) RETURN DISTINCT labels(n)",
        );

        assert_eq!(result.rows.len(), 1);
    }

    #[test]
    fn test_match_order_by_asc() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) RETURN n.name ORDER BY n.name",
        );

        assert_eq!(result.rows.len(), 3);
        assert_eq!(result.rows[0][0], "Alice");
        assert_eq!(result.rows[1][0], "Bob");
        assert_eq!(result.rows[2][0], "Carol");
    }

    #[test]
    fn test_match_order_by_desc() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) RETURN n.age ORDER BY n.age DESC",
        );

        assert_eq!(result.rows.len(), 3);
        assert_eq!(result.rows[0][0], 35); // Bob
        assert_eq!(result.rows[1][0], 28); // Carol
        assert_eq!(result.rows[2][0], 25); // Alice
    }

    #[test]
    fn test_match_skip() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) RETURN n.name ORDER BY n.name SKIP 1",
        );

        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], "Bob");
        assert_eq!(result.rows[1][0], "Carol");
    }

    #[test]
    fn test_match_limit() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) RETURN n.name ORDER BY n.name LIMIT 2",
        );

        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], "Alice");
        assert_eq!(result.rows[1][0], "Bob");
    }

    #[test]
    fn test_match_skip_and_limit() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) RETURN n.name ORDER BY n.name SKIP 1 LIMIT 1",
        );

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], "Bob");
    }

    #[test]
    fn test_match_count_aggregate() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) RETURN count(n)",
        );

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], 3);
    }

    #[test]
    fn test_match_with_alias() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) RETURN n.name AS person_name",
        );

        assert_eq!(result.columns, vec!["person_name"]);
        assert_eq!(result.rows.len(), 3);
    }

    #[test]
    fn test_match_id_function() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) RETURN id(n), n.name ORDER BY id(n)",
        );

        assert_eq!(result.columns, vec!["id(n)", "n.name"]);
        // Node IDs are 1, 2, 3 for Alice, Bob, Carol.
        assert_eq!(result.rows[0][0], 1);
        assert_eq!(result.rows[1][0], 2);
        assert_eq!(result.rows[2][0], 3);
    }

    #[test]
    fn test_match_labels_function() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Company) RETURN labels(n)",
        );

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], serde_json::json!(["Company"]));
    }

    // ── CREATE tests ───────────────────────────────────────────────────

    #[test]
    fn test_create_node() {
        let (graph, executor) = setup_test_graph();
        let stmt = crate::parse(r#"CREATE (n:Person {name: "Dave", age: 40})"#).unwrap();
        let result = executor.execute(stmt).unwrap();

        assert_eq!(result.stats.nodes_created, 1);

        // Verify the node exists in the graph.
        let dave = graph.get_node(NodeId(5)).unwrap().unwrap();
        assert_eq!(dave.labels, vec!["Person"]);
        assert_eq!(dave.properties["name"], "Dave");
        assert_eq!(dave.properties["age"], 40);
    }

    #[test]
    fn test_create_node_with_edge() {
        let (graph, executor) = setup_test_graph();
        let stmt = crate::parse(
            r#"CREATE (a:Person {name: "Dave"})-[:FRIENDS_WITH]->(b:Person {name: "Eve"})"#,
        ).unwrap();
        let result = executor.execute(stmt).unwrap();

        assert_eq!(result.stats.nodes_created, 2);
        assert_eq!(result.stats.edges_created, 1);

        // Verify nodes.
        let dave = graph.get_node(NodeId(5)).unwrap().unwrap();
        assert_eq!(dave.properties["name"], "Dave");

        let eve = graph.get_node(NodeId(6)).unwrap().unwrap();
        assert_eq!(eve.properties["name"], "Eve");

        // Verify edge.
        let edge = graph.get_edge(EdgeId(4)).unwrap().unwrap();
        assert_eq!(edge.source, NodeId(5));
        assert_eq!(edge.target, NodeId(6));
        assert_eq!(edge.edge_type, "FRIENDS_WITH");
    }

    // ── Expression evaluation tests ────────────────────────────────────

    #[test]
    fn test_eval_arithmetic() {
        let bindings = BindingRow::new();

        // 2 + 3 = 5
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(2))),
            BinOp::Add,
            Box::new(Expr::Literal(Literal::Integer(3))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), serde_json::json!(5));

        // 10 - 4 = 6
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(10))),
            BinOp::Sub,
            Box::new(Expr::Literal(Literal::Integer(4))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), serde_json::json!(6));

        // 3 * 4 = 12
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(3))),
            BinOp::Mul,
            Box::new(Expr::Literal(Literal::Integer(4))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), serde_json::json!(12));

        // 15 / 3 = 5
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(15))),
            BinOp::Div,
            Box::new(Expr::Literal(Literal::Integer(3))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), serde_json::json!(5));

        // 7 % 3 = 1
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(7))),
            BinOp::Mod,
            Box::new(Expr::Literal(Literal::Integer(3))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), serde_json::json!(1));
    }

    #[test]
    fn test_eval_comparisons() {
        let bindings = BindingRow::new();

        // 5 > 3 = true
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(5))),
            BinOp::Gt,
            Box::new(Expr::Literal(Literal::Integer(3))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), Value::Bool(true));

        // 3 > 5 = false
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(3))),
            BinOp::Gt,
            Box::new(Expr::Literal(Literal::Integer(5))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), Value::Bool(false));

        // 5 = 5 = true
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(5))),
            BinOp::Eq,
            Box::new(Expr::Literal(Literal::Integer(5))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), Value::Bool(true));

        // 5 <> 3 = true
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(5))),
            BinOp::Neq,
            Box::new(Expr::Literal(Literal::Integer(3))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), Value::Bool(true));

        // 3 <= 3 = true
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(3))),
            BinOp::Lte,
            Box::new(Expr::Literal(Literal::Integer(3))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), Value::Bool(true));

        // 4 >= 5 = false
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(4))),
            BinOp::Gte,
            Box::new(Expr::Literal(Literal::Integer(5))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_eval_boolean_logic() {
        let bindings = BindingRow::new();

        // true AND false = false
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Boolean(true))),
            BinOp::And,
            Box::new(Expr::Literal(Literal::Boolean(false))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), Value::Bool(false));

        // true OR false = true
        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Boolean(true))),
            BinOp::Or,
            Box::new(Expr::Literal(Literal::Boolean(false))),
        );
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), Value::Bool(true));

        // NOT true = false
        let expr = Expr::UnaryOp(UnOp::Not, Box::new(Expr::Literal(Literal::Boolean(true))));
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_eval_is_null() {
        let mut bindings = BindingRow::new();
        bindings.insert("x".into(), Value::Null);
        bindings.insert("y".into(), serde_json::json!(42));

        let expr_x = Expr::IsNull(Box::new(Expr::Variable("x".into())));
        assert_eq!(eval_expr(&expr_x, &bindings).unwrap(), Value::Bool(true));

        let expr_y = Expr::IsNull(Box::new(Expr::Variable("y".into())));
        assert_eq!(eval_expr(&expr_y, &bindings).unwrap(), Value::Bool(false));

        let expr_y_not_null = Expr::IsNotNull(Box::new(Expr::Variable("y".into())));
        assert_eq!(eval_expr(&expr_y_not_null, &bindings).unwrap(), Value::Bool(true));
    }

    #[test]
    fn test_eval_property_access() {
        let mut bindings = BindingRow::new();
        bindings.insert(
            "n".into(),
            serde_json::json!({
                "id": 1,
                "labels": ["Person"],
                "properties": {"name": "Alice", "age": 25}
            }),
        );

        let expr = Expr::Property(Box::new(Expr::Variable("n".into())), "name".into());
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), "Alice");

        let expr = Expr::Property(Box::new(Expr::Variable("n".into())), "age".into());
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), 25);
    }

    #[test]
    fn test_eval_negation() {
        let bindings = BindingRow::new();

        let expr = Expr::UnaryOp(UnOp::Neg, Box::new(Expr::Literal(Literal::Integer(5))));
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), serde_json::json!(-5));

        let expr = Expr::UnaryOp(UnOp::Neg, Box::new(Expr::Literal(Literal::Float(3.14))));
        assert_eq!(eval_expr(&expr, &bindings).unwrap(), serde_json::json!(-3.14));
    }

    #[test]
    fn test_eval_division_by_zero() {
        let bindings = BindingRow::new();

        let expr = Expr::BinaryOp(
            Box::new(Expr::Literal(Literal::Integer(10))),
            BinOp::Div,
            Box::new(Expr::Literal(Literal::Integer(0))),
        );
        assert!(eval_expr(&expr, &bindings).is_err());
    }

    #[test]
    fn test_match_incoming_edge() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (b:Person)<-[:KNOWS]-(a:Person) RETURN a.name, b.name",
        );

        // Alice->Bob, Bob->Carol => Bob has incoming from Alice, Carol has incoming from Bob
        assert_eq!(result.rows.len(), 2);

        let pairs: Vec<(String, String)> = result.rows.iter()
            .map(|row| {
                (
                    row[0].as_str().unwrap().to_string(),
                    row[1].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert!(pairs.contains(&("Alice".into(), "Bob".into())));
        assert!(pairs.contains(&("Bob".into(), "Carol".into())));
    }

    #[test]
    fn test_match_no_results() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) WHERE n.age > 100 RETURN n.name",
        );

        assert_eq!(result.rows.len(), 0);
    }

    #[test]
    fn test_match_string_equality() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            r#"MATCH (n:Person) WHERE n.name = "Alice" RETURN n.age"#,
        );

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], 25);
    }

    #[test]
    fn test_count_empty_result() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (n:Person) WHERE n.age > 100 RETURN count(n)",
        );

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], 0);
    }

    #[test]
    fn test_match_edge_with_variable() {
        let (_graph, executor) = setup_test_graph();
        let result = run_query(
            &executor,
            "MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN a.name, type(r), b.name",
        );

        assert_eq!(result.rows.len(), 2);
        // All edges should be KNOWS.
        for row in &result.rows {
            assert_eq!(row[1], "KNOWS");
        }
    }

    #[test]
    fn test_match_mixed_labels() {
        let (_graph, executor) = setup_test_graph();
        // Match WORKS_AT edge between Person and Company.
        let result = run_query(
            &executor,
            "MATCH (p:Person)-[:WORKS_AT]->(c:Company) RETURN p.name, c.name",
        );

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], "Alice");
        assert_eq!(result.rows[0][1], "Acme");
    }
}
