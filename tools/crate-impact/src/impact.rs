//! Reverse-dependency closure over a `CrateGraph`.
//!
//! Given a set of changed crate names, `impacted_crates` walks
//! `DEPENDS_ON` edges in the **incoming** direction (i.e. "who depends
//! on me?") until no new crates are discovered (fixpoint), then returns
//! the full impacted set as a sorted list.

use std::collections::{HashMap, HashSet};

use astraea_core::traits::GraphOps;
use astraea_core::types::{Direction, NodeId};

use crate::crate_graph::CrateGraph;

/// Return the set of workspace crates whose tests must run when the given
/// crate names have changed.
///
/// The result always includes every name in `changed` that is a known
/// workspace crate; it also includes every crate that transitively depends
/// on any of them (walking `DEPENDS_ON` edges in the incoming direction).
///
/// Names in `changed` that are not found in the workspace are logged to
/// stderr and silently skipped.
pub fn impacted_crates(cg: &CrateGraph, changed: &[String]) -> anyhow::Result<Vec<String>> {
    // Build a reverse lookup: NodeId → crate name.
    let id_to_name: HashMap<NodeId, &str> = cg
        .by_name
        .iter()
        .map(|(name, &id)| (id, name.as_str()))
        .collect();

    // Seed the result set from the changed crate names.
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut frontier: Vec<NodeId> = Vec::new();

    for name in changed {
        match cg.by_name.get(name.as_str()) {
            Some(&nid) => {
                if visited.insert(nid) {
                    frontier.push(nid);
                }
            }
            None => {
                eprintln!(
                    "warning: changed crate '{name}' is not a known workspace member; skipping"
                );
            }
        }
    }

    // Fixpoint loop: expand incoming DEPENDS_ON edges until stable.
    // In a 15-crate workspace this converges in ≤ 15 iterations.
    while !frontier.is_empty() {
        let current = std::mem::take(&mut frontier);
        for nid in current {
            let reverse_deps =
                cg.graph
                    .neighbors_filtered(nid, Direction::Incoming, "DEPENDS_ON")?;
            for (_eid, dep_id) in reverse_deps {
                if visited.insert(dep_id) {
                    frontier.push(dep_id);
                }
            }
        }
    }

    // Map node ids back to names and sort for stable output.
    let mut result: Vec<String> = visited
        .iter()
        .filter_map(|nid| id_to_name.get(nid).map(|s| s.to_string()))
        .collect();
    result.sort();

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crate_graph;
    use std::path::PathBuf;

    const ASTRAEADB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

    fn astraeadb_path() -> PathBuf {
        PathBuf::from(ASTRAEADB_DIR)
    }

    /// Changing `astraea-core` must impact every other workspace crate because
    /// every crate depends on it (directly or transitively), including via
    /// dev-dependencies.
    ///
    /// With dev-deps included in the `DEPENDS_ON` edge set, all 15 workspace
    /// crates are reachable from `astraea-core` via incoming edges:
    ///   - direct normal deps account for 13 crates (all except astraea-core itself)
    ///   - astraea-rag and astraea-gnn both normally depend on astraea-core and
    ///     also transitively reach it via their dev-dep on astraea-graph
    ///
    /// Hard assertions: result contains `astraea-graph`, `astraea-cli`,
    /// `astraea-rag`, and `astraea-gnn`.
    /// Hard assertion: result contains all 15 workspace crates.
    #[test]
    fn core_change_impacts_all_downstream() {
        let cg = crate_graph::build(&astraeadb_path()).expect("build failed");
        let changed = vec!["astraea-core".to_string()];
        let impacted = impacted_crates(&cg, &changed).expect("impacted_crates failed");

        println!(
            "impacted by astraea-core change ({} total):",
            impacted.len()
        );
        for name in &impacted {
            println!("  {name}");
        }

        assert!(
            impacted.contains(&"astraea-graph".to_string()),
            "astraea-graph should be in the impacted set"
        );
        assert!(
            impacted.contains(&"astraea-cli".to_string()),
            "astraea-cli should be in the impacted set"
        );
        assert!(
            impacted.contains(&"astraea-rag".to_string()),
            "astraea-rag should be in the impacted set (depends on astraea-core directly)"
        );
        assert!(
            impacted.contains(&"astraea-gnn".to_string()),
            "astraea-gnn should be in the impacted set (depends on astraea-core directly)"
        );
        // All 15 workspace crates depend on astraea-core directly or transitively.
        assert_eq!(
            impacted.len(),
            15,
            "expected all 15 workspace crates in impacted set, got {}: {impacted:?}",
            impacted.len()
        );
    }

    /// Changing a leaf crate (one that no other workspace crate depends on)
    /// must impact only itself.
    ///
    /// `astraea-cli` is a leaf: no workspace crate lists it as a normal dep.
    #[test]
    fn leaf_change_impacts_only_self() {
        let cg = crate_graph::build(&astraeadb_path()).expect("build failed");
        let changed = vec!["astraea-cli".to_string()];
        let impacted = impacted_crates(&cg, &changed).expect("impacted_crates failed");

        println!("impacted by astraea-cli change: {impacted:?}");
        assert_eq!(
            impacted,
            vec!["astraea-cli"],
            "astraea-cli is a leaf; only itself should be impacted"
        );
    }

    /// An unknown crate name should produce a warning on stderr but must
    /// not panic or return an error; the result should be empty.
    #[test]
    fn unknown_crate_warns_but_does_not_error() {
        let cg = crate_graph::build(&astraeadb_path()).expect("build failed");
        let changed = vec!["nonexistent".to_string()];
        let impacted = impacted_crates(&cg, &changed).expect("should not error");
        assert!(
            impacted.is_empty(),
            "unknown crate should yield empty impacted set, got {impacted:?}"
        );
    }
}
