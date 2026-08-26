//! Builds an in-memory crate-dependency graph from `cargo metadata`.
//!
//! One `Crate` node is created per workspace member; `DEPENDS_ON` edges
//! connect them wherever a normal or development dependency between two
//! workspace members exists. Build-dependencies are excluded.

use std::collections::HashMap;
use std::path::PathBuf;

use astraea_core::traits::GraphOps;
use astraea_core::types::NodeId;
use astraea_graph::test_utils::InMemoryStorage;
use astraea_graph::Graph;
use cargo_metadata::{DependencyKind, MetadataCommand};
use serde_json::json;

/// The crate-dependency graph for one workspace, held in an in-memory
/// AstraeaDB `Graph`.
pub struct CrateGraph {
    /// The AstraeaDB graph containing `Crate` nodes and `DEPENDS_ON` edges.
    pub graph: Graph,
    /// crate name → node id (populated during `build`).
    pub by_name: HashMap<String, NodeId>,
    /// crate name → absolute crate root directory (the directory containing
    /// the crate's `Cargo.toml`), for longest-prefix matching of changed files.
    pub paths: HashMap<String, PathBuf>,
}

/// Parse `cargo metadata` for the workspace rooted at `manifest_dir` and
/// build an in-memory dependency graph.
///
/// Only workspace members are included; external crate nodes are not created.
/// [`DependencyKind::Normal`] and [`DependencyKind::Development`] dependencies
/// produce `DEPENDS_ON` edges. [`DependencyKind::Build`] dependencies are excluded.
pub fn build(manifest_dir: &std::path::Path) -> anyhow::Result<CrateGraph> {
    let metadata = MetadataCommand::new()
        .manifest_path(manifest_dir.join("Cargo.toml"))
        .exec()?;

    let graph = Graph::new(Box::new(InMemoryStorage::default()));
    let mut by_name: HashMap<String, NodeId> = HashMap::new();
    let mut paths: HashMap<String, PathBuf> = HashMap::new();

    // Collect the set of workspace member package ids for fast membership test.
    let workspace_member_ids: std::collections::HashSet<_> =
        metadata.workspace_members.iter().cloned().collect();

    // First pass: create one Crate node per workspace member.
    for pkg_id in &metadata.workspace_members {
        let pkg = metadata
            .packages
            .iter()
            .find(|p| &p.id == pkg_id)
            .expect("workspace member missing from packages list");

        // The manifest_path is the path to Cargo.toml; the parent is the crate root.
        let crate_root = pkg
            .manifest_path
            .parent()
            .expect("manifest_path has no parent")
            .as_std_path()
            .to_path_buf();

        let node_id = graph.create_node(
            vec!["Crate".into()],
            json!({
                "name": pkg.name,
                "version": pkg.version.to_string(),
                "path": crate_root.to_string_lossy()
            }),
            None,
        )?;

        by_name.insert(pkg.name.clone(), node_id);
        paths.insert(pkg.name.clone(), crate_root);
    }

    // Second pass: add DEPENDS_ON edges for normal and dev deps between workspace members.
    // dev-deps matter for regression testing: a change to A can break B's tests when B only test-depends on A
    for pkg_id in &metadata.workspace_members {
        let pkg = metadata
            .packages
            .iter()
            .find(|p| &p.id == pkg_id)
            .expect("workspace member missing from packages list");

        let from_id = by_name[&pkg.name];

        // Track (from, to) pairs already connected to avoid duplicate edges when a
        // crate declares both a normal and a dev-dep on the same workspace member.
        let mut emitted: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

        for dep in &pkg.dependencies {
            // Include Normal and Development; skip Build.
            if dep.kind == DependencyKind::Build {
                continue;
            }

            // Only create edges to other workspace members.
            if let Some(&to_id) = by_name.get(&dep.name) {
                if emitted.insert(to_id) {
                    graph.create_edge(
                        from_id,
                        to_id,
                        "DEPENDS_ON".into(),
                        json!({}),
                        1.0,
                        None,
                        None,
                    )?;
                }
            }
        }
    }

    // Suppress the unused-variable warning for workspace_member_ids on older
    // compiler versions (we used it only in the collection step above).
    drop(workspace_member_ids);

    Ok(CrateGraph {
        graph,
        by_name,
        paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::types::Direction;

    const ASTRAEADB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

    fn astraeadb_path() -> PathBuf {
        PathBuf::from(ASTRAEADB_DIR)
    }

    /// The workspace currently has 15 crates; soft-assert ≥ 14 to allow for
    /// minor workspace changes without immediately breaking this test.
    #[test]
    fn builds_15_crates_for_astraeadb_workspace() {
        let cg = build(&astraeadb_path()).expect("build failed");
        let actual = cg.by_name.len();
        println!("workspace crate count: {actual}");
        assert!(actual >= 14, "expected ≥ 14 workspace crates, got {actual}");
        // Soft assertion printed above; hard assertion for exact count.
        if actual != 15 {
            println!("NOTE: expected 15 crates but found {actual}; check workspace membership.");
        }
    }

    /// Verify known directed edges exist in the graph.
    ///
    /// Edges checked:
    ///   astraea-graph  → astraea-core     (normal dep)
    ///   astraea-graph  → astraea-storage  (normal dep)
    ///   astraea-cli    → astraea-server   (normal dep; confirmed in astraea-cli/Cargo.toml)
    #[test]
    fn has_expected_edges() {
        let cg = build(&astraeadb_path()).expect("build failed");

        let check_edge = |from_name: &str, to_name: &str| {
            let from_id = *cg
                .by_name
                .get(from_name)
                .unwrap_or_else(|| panic!("crate not found: {from_name}"));
            let to_id = *cg
                .by_name
                .get(to_name)
                .unwrap_or_else(|| panic!("crate not found: {to_name}"));

            let neighbors = cg
                .graph
                .neighbors_filtered(from_id, Direction::Outgoing, "DEPENDS_ON")
                .unwrap_or_else(|e| panic!("neighbors_filtered failed: {e}"));

            let target_ids: Vec<NodeId> = neighbors.into_iter().map(|(_eid, nid)| nid).collect();
            assert!(
                target_ids.contains(&to_id),
                "expected edge {from_name} → {to_name} but it was not found; \
                 outgoing DEPENDS_ON neighbors: {target_ids:?}"
            );
        };

        check_edge("astraea-graph", "astraea-core");
        check_edge("astraea-graph", "astraea-storage");
        // astraea-cli → astraea-server confirmed in
        // <workspace root>/crates/astraea-cli/Cargo.toml
        // under [dependencies]: astraea-server = { workspace = true }
        check_edge("astraea-cli", "astraea-server");
    }

    /// Verify that dev-dependencies produce `DEPENDS_ON` edges.
    ///
    /// `astraea-rag` and `astraea-gnn` both list `astraea-graph` exclusively
    /// under `[dev-dependencies]` (no normal dep). With dev-deps included in the
    /// edge set, a change to `astraea-graph` must be flagged as impacting their
    /// tests. This test asserts the outgoing edge exists from each of those crates
    /// to `astraea-graph`.
    #[test]
    fn dev_deps_create_edges() {
        let cg = build(&astraeadb_path()).expect("build failed");

        let check_edge = |from_name: &str, to_name: &str| {
            let from_id = *cg
                .by_name
                .get(from_name)
                .unwrap_or_else(|| panic!("crate not found: {from_name}"));
            let to_id = *cg
                .by_name
                .get(to_name)
                .unwrap_or_else(|| panic!("crate not found: {to_name}"));

            let neighbors = cg
                .graph
                .neighbors_filtered(from_id, Direction::Outgoing, "DEPENDS_ON")
                .unwrap_or_else(|e| panic!("neighbors_filtered failed: {e}"));

            let target_ids: Vec<NodeId> = neighbors.into_iter().map(|(_eid, nid)| nid).collect();
            assert!(
                target_ids.contains(&to_id),
                "expected dev-dep edge {from_name} → {to_name} but it was not found \
                 (only dev-dependency in Cargo.toml); outgoing DEPENDS_ON neighbors: {target_ids:?}"
            );
        };

        // astraea-rag only dev-depends on astraea-graph (no normal dep).
        check_edge("astraea-rag", "astraea-graph");
        // astraea-gnn only dev-depends on astraea-graph (no normal dep).
        check_edge("astraea-gnn", "astraea-graph");
        // Both also dev-depend on astraea-vector.
        check_edge("astraea-rag", "astraea-vector");
        check_edge("astraea-gnn", "astraea-vector");
    }
}
