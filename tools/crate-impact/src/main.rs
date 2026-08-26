//! Crate-impact analysis tool for AstraeaDB.
//!
//! Given a set of changed file paths (from stdin or `git diff --name-only`),
//! emits a JSON array of the workspace crate names whose tests must run.
//!
//! # Usage
//!
//! Read changed files from stdin (one path per line):
//! ```sh
//! echo "crates/astraea-storage/src/page.rs" | cargo run --release -- --stdin
//! ```
//!
//! Read changed files from `git diff --name-only origin/main...HEAD` (default):
//! ```sh
//! cargo run --release
//! ```
//!
//! # Environment variables
//!
//! - `ASTRAEADB_DIR` — path to the AstraeaDB workspace root.
//!   Default: the repository containing this tool.
//! - `GIT_DIFF_BASE` — base ref for `git diff --name-only`.
//!   Default: `origin/main`.

mod crate_graph;
mod impact;

use std::io::{self, BufRead};
use std::path::PathBuf;
use std::process::Command;

fn main() -> anyhow::Result<()> {
    // ------------------------------------------------------------------
    // 1. Determine workspace root.
    // ------------------------------------------------------------------
    // Default to the repository this tool is vendored into, worked out at
    // compile time from its own manifest location, rather than to one
    // contributor's home directory as it did when the tool lived elsewhere.
    // CI still sets ASTRAEADB_DIR explicitly, because there the binary is
    // downloaded as an artifact and run from a different checkout.
    let astraeadb_dir = std::env::var("ASTRAEADB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")));
    // Normalise before use. Crate locations come from `cargo metadata` already
    // canonicalised, and changed-file paths are matched against them by prefix,
    // so a directory containing `..` matches nothing and the tool silently
    // reports that a change affects no crates at all. The default above is
    // exactly such a path, and "no crates impacted" means "run no tests",
    // which is the most dangerous way for this tool to be wrong.
    let astraeadb_dir = astraeadb_dir.canonicalize().unwrap_or(astraeadb_dir);

    // ------------------------------------------------------------------
    // 2. Build the crate graph.
    // ------------------------------------------------------------------
    let cg = crate_graph::build(&astraeadb_dir)?;

    // ------------------------------------------------------------------
    // 3. Collect changed file paths.
    // ------------------------------------------------------------------
    let args: Vec<String> = std::env::args().skip(1).collect();
    let use_stdin = args.first().map(|s| s == "--stdin").unwrap_or(false);

    let changed_files: Vec<String> = if use_stdin {
        let stdin = io::stdin();
        stdin
            .lock()
            .lines()
            .filter_map(|line| line.ok())
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    } else {
        let base = std::env::var("GIT_DIFF_BASE").unwrap_or_else(|_| "origin/main".into());
        let range = format!("{base}...HEAD");

        let output = Command::new("git")
            .args(["diff", "--name-only", &range])
            .current_dir(&astraeadb_dir)
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run git diff: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git diff failed: {stderr}");
        }

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    };

    // ------------------------------------------------------------------
    // 4. Map changed files → owning crates via longest-prefix match.
    // ------------------------------------------------------------------
    let changed_crates: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for file_path in &changed_files {
            // Treat the path as relative to astraeadb_dir; resolve to absolute.
            let abs_path = astraeadb_dir.join(file_path);

            // Find the workspace crate whose root path is the longest prefix
            // of the changed file's absolute path.
            let best = cg
                .paths
                .iter()
                .filter(|(_name, crate_root)| abs_path.starts_with(crate_root))
                .max_by_key(|(_name, crate_root)| crate_root.as_os_str().len());

            match best {
                Some((name, _)) => {
                    if seen.insert(name.clone()) {
                        result.push(name.clone());
                    }
                }
                None => {
                    // File outside any workspace crate (docs/*, root README, etc.) — skip.
                }
            }
        }

        result
    };

    // ------------------------------------------------------------------
    // 5. Compute impacted crates and emit JSON.
    // ------------------------------------------------------------------
    if changed_crates.is_empty() {
        println!("[]");
        return Ok(());
    }

    let impacted = impact::impacted_crates(&cg, &changed_crates)?;
    println!("{}", serde_json::to_string(&impacted)?);

    Ok(())
}
