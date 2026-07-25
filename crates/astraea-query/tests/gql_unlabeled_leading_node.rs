//! Acceptance test for GQL #2189: `MATCH` with an unlabeled leading node
//! pattern used to return 0 rows even when matching edges/nodes existed.
//!
//! This deliberately uses the real (non-mock) `astraea_graph::Graph` +
//! `InMemoryStorage` backend rather than `astraea-query`'s own test double
//! in `executor.rs`. The original bug was masked in that crate's unit
//! tests because the mock special-cased `find_by_label("")` as "all
//! nodes" -- a convention production storage engines never honored. This
//! test exercises the exact `GraphOps` implementation used in production
//! (`Graph::list_all_nodes` delegating to `StorageEngine::list_all_nodes`),
//! so it would have caught the regression.

use std::collections::BTreeSet;
use std::sync::Arc;

use astraea_core::traits::GraphOps;
use astraea_graph::Graph;
use astraea_graph::test_utils::InMemoryStorage;
use astraea_query::executor::Executor;

fn new_graph() -> Arc<dyn GraphOps> {
    Arc::new(Graph::new(Box::new(InMemoryStorage::new())))
}

/// Run a GQL query and collect `(a.name, b.name)` pairs from a two-column
/// `RETURN a.name, b.name` projection.
fn run_pairs(executor: &Executor, gql: &str) -> BTreeSet<(String, String)> {
    let stmt = astraea_query::parse(gql).expect("parse failed");
    let result = executor.execute(stmt).expect("execute failed");
    result
        .rows
        .into_iter()
        .map(|row| {
            (
                row[0]
                    .as_str()
                    .expect("a.name should be a string")
                    .to_string(),
                row[1]
                    .as_str()
                    .expect("b.name should be a string")
                    .to_string(),
            )
        })
        .collect()
}

/// Regression test mirroring the exact shape of the original bug report
/// (astraeadb-issues #14 / KG issue 2189): a fixture where every source
/// node with an outgoing `:T` edge shares the same label. The unlabeled-
/// leading and labeled-leading forms of the same pattern must return
/// identical, non-empty rows.
#[test]
fn unlabeled_leading_node_matches_same_rows_as_labeled_when_all_sources_share_label() {
    let graph = new_graph();

    // 5 Person nodes chained by 4 outgoing "T" edges.
    let names = ["Alice", "Bob", "Carol", "Dave", "Erin"];
    let mut ids = Vec::new();
    for name in names {
        let id = graph
            .create_node(
                vec!["Person".into()],
                serde_json::json!({ "name": name }),
                None,
            )
            .unwrap();
        ids.push(id);
    }
    for pair in ids.windows(2) {
        graph
            .create_edge(
                pair[0],
                pair[1],
                "T".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
    }

    let executor = Executor::new(graph);

    let unlabeled = run_pairs(&executor, "MATCH (a)-[r:T]->(b) RETURN a.name, b.name");
    let labeled = run_pairs(
        &executor,
        "MATCH (a:Person)-[r:T]->(b) RETURN a.name, b.name",
    );

    // Regression guard: this used to be empty (GQL #2189).
    assert!(
        !unlabeled.is_empty(),
        "unlabeled leading node pattern returned no rows"
    );
    assert_eq!(unlabeled.len(), 4, "expected one row per chained edge");
    assert_eq!(
        unlabeled, labeled,
        "unlabeled- and labeled-leading forms should return identical rows \
         when every source shares the label"
    );
}

/// Broader assertion: the unlabeled leading node pattern must match every
/// source node with a matching outgoing edge -- including nodes that don't
/// carry the label used by the "labeled" form (or carry no label at all).
/// This proves the fix seeds candidates from a real full scan rather than
/// happening to work only because every node in the first test shares one
/// label.
#[test]
fn unlabeled_leading_node_matches_all_sources_regardless_of_label() {
    let graph = new_graph();

    let alice = graph
        .create_node(
            vec!["Person".into()],
            serde_json::json!({ "name": "Alice" }),
            None,
        )
        .unwrap();
    let bob = graph
        .create_node(
            vec!["Person".into()],
            serde_json::json!({ "name": "Bob" }),
            None,
        )
        .unwrap();
    // Dan carries no labels at all -- the case a `find_by_label("")` hack
    // could never handle correctly even in principle.
    let dan = graph
        .create_node(vec![], serde_json::json!({ "name": "Dan" }), None)
        .unwrap();
    // Zeta carries a label, just not the one used by the "labeled" query.
    let zeta = graph
        .create_node(
            vec!["Robot".into()],
            serde_json::json!({ "name": "Zeta" }),
            None,
        )
        .unwrap();
    let target = graph
        .create_node(
            vec!["Person".into()],
            serde_json::json!({ "name": "Target" }),
            None,
        )
        .unwrap();
    // Noise: a Person with no outgoing `:T` edge at all -- must not appear
    // in either result set.
    graph
        .create_node(
            vec!["Person".into()],
            serde_json::json!({ "name": "Idle" }),
            None,
        )
        .unwrap();

    for src in [alice, bob, dan, zeta] {
        graph
            .create_edge(
                src,
                target,
                "T".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
    }

    let executor = Executor::new(graph);

    let unlabeled = run_pairs(&executor, "MATCH (a)-[r:T]->(b) RETURN a.name, b.name");
    let labeled = run_pairs(
        &executor,
        "MATCH (a:Person)-[r:T]->(b) RETURN a.name, b.name",
    );

    let expected_all: BTreeSet<(String, String)> = [
        ("Alice".to_string(), "Target".to_string()),
        ("Bob".to_string(), "Target".to_string()),
        ("Dan".to_string(), "Target".to_string()),
        ("Zeta".to_string(), "Target".to_string()),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        unlabeled, expected_all,
        "unlabeled leading node pattern must match every source node with a \
         matching outgoing edge, regardless of label"
    );

    // The labeled form is a strict subset: only Alice and Bob carry
    // `:Person`, so Dan (no label) and Zeta (:Robot) are correctly
    // excluded -- confirming the fix didn't loosen labeled-leading
    // filtering.
    let expected_labeled: BTreeSet<(String, String)> = [
        ("Alice".to_string(), "Target".to_string()),
        ("Bob".to_string(), "Target".to_string()),
    ]
    .into_iter()
    .collect();
    assert_eq!(labeled, expected_labeled);
}
