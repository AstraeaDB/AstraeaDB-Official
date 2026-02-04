//! Arrow schema definitions for AstraeaDB data types.
//!
//! These schemas define the wire format for nodes, edges, and query results
//! when transferred via Arrow Flight.

use arrow_schema::{DataType, Field, Schema};

/// Schema for node data exported via Arrow Flight.
///
/// Columns:
/// - `id`: UInt64 -- the node's unique identifier
/// - `labels`: Utf8 -- JSON-encoded array of label strings
/// - `properties`: Utf8 -- JSON-encoded property map
/// - `has_embedding`: Boolean -- whether the node has a vector embedding
pub fn node_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("labels", DataType::Utf8, false),
        Field::new("properties", DataType::Utf8, false),
        Field::new("has_embedding", DataType::Boolean, false),
    ])
}

/// Schema for edge data exported via Arrow Flight.
///
/// Columns:
/// - `id`: UInt64 -- the edge's unique identifier
/// - `source`: UInt64 -- source node ID
/// - `target`: UInt64 -- target node ID
/// - `edge_type`: Utf8 -- relationship type label
/// - `properties`: Utf8 -- JSON-encoded property map
/// - `weight`: Float64 -- learnable edge weight
/// - `valid_from`: Int64 (nullable) -- temporal validity start (epoch ms)
/// - `valid_to`: Int64 (nullable) -- temporal validity end (epoch ms)
pub fn edge_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("source", DataType::UInt64, false),
        Field::new("target", DataType::UInt64, false),
        Field::new("edge_type", DataType::Utf8, false),
        Field::new("properties", DataType::Utf8, false),
        Field::new("weight", DataType::Float64, false),
        Field::new("valid_from", DataType::Int64, true),
        Field::new("valid_to", DataType::Int64, true),
    ])
}

/// Schema for dynamic query results.
///
/// Since GQL query results have dynamic column types, all columns are
/// represented as nullable Utf8 (JSON-encoded values). The column names
/// come from the query's RETURN clause.
pub fn query_result_schema(columns: &[String]) -> Schema {
    let fields: Vec<Field> = columns
        .iter()
        .map(|name| Field::new(name.as_str(), DataType::Utf8, true))
        .collect();
    Schema::new(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_schema_has_expected_fields() {
        let schema = node_schema();
        assert_eq!(schema.fields().len(), 4);
        assert_eq!(schema.field(0).name(), "id");
        assert_eq!(schema.field(1).name(), "labels");
        assert_eq!(schema.field(2).name(), "properties");
        assert_eq!(schema.field(3).name(), "has_embedding");
    }

    #[test]
    fn edge_schema_has_expected_fields() {
        let schema = edge_schema();
        assert_eq!(schema.fields().len(), 8);
        assert_eq!(schema.field(0).name(), "id");
        assert_eq!(schema.field(5).name(), "weight");
        // Temporal fields are nullable.
        assert!(schema.field(6).is_nullable());
        assert!(schema.field(7).is_nullable());
    }

    #[test]
    fn query_result_schema_dynamic_columns() {
        let cols = vec!["n.name".to_string(), "n.age".to_string()];
        let schema = query_result_schema(&cols);
        assert_eq!(schema.fields().len(), 2);
        assert_eq!(schema.field(0).name(), "n.name");
        assert_eq!(schema.field(1).name(), "n.age");
        // All query result columns are nullable.
        assert!(schema.field(0).is_nullable());
    }

    #[test]
    fn query_result_schema_empty() {
        let schema = query_result_schema(&[]);
        assert_eq!(schema.fields().len(), 0);
    }
}
