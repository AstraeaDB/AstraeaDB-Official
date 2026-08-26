//! Integration smoke tests for the impact-analysis binary.
//!
//! These tests invoke the compiled binary via `std::process::Command` and
//! assert its stdout against the golden outputs documented in
//! `docs/regression-testing.md` in this repository
//! §"Predictability examples", plus two edge cases.
//!
//!
//! ## A note on the golden lists below
//!
//! Two of them were stale when this tool was vendored into the repository, and
//! nothing had noticed because CI built the tool without running its tests.
//! `astraea-crypto` had been renamed `astraea-encrypt-demo` in 0.2.0, and a
//! change under `astraea-storage` now reaches `astraea-query` as well, because
//! query gained a dependency on `astraea-graph` in c1658b4. Both were the
//! fixtures being out of date rather than the tool being wrong; the dependency
//! chain storage <- graph <- query was checked by hand before the list was
//! widened. If one of these fails, confirm which of the two it is before
//! editing the expectation.
//!
//! Run with:
//!   cargo test --release --test impact_smoke

use std::io::Write as _;
use std::process::{Command, Stdio};

/// Path to the AstraeaDB workspace root used by all invocations.
const ASTRAEADB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Path to the binary under test.
///
/// `CARGO_BIN_EXE_<name>` is set by cargo for integration tests and points at
/// the binary it just built, in whichever profile is running. The previous
/// version hand-assembled `target/release/<name>`, which broke the moment the
/// package was renamed and would also have silently tested a stale binary
/// under `cargo test` without `--release`.
fn binary_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_crate-impact"))
}

/// Run the binary with `--stdin`, piping `input` on stdin.
/// Returns trimmed stdout on success; panics on non-zero exit.
fn run_impact(input: &str) -> String {
    let bin = binary_path();
    assert!(
        bin.exists(),
        "binary not found at {:?} — run `cargo build --release` first",
        bin
    );

    let mut child = Command::new(&bin)
        .arg("--stdin")
        .env("ASTRAEADB_DIR", ASTRAEADB_DIR)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn binary");

    // Write the input and close stdin so the binary sees EOF.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .expect("failed to write to stdin");
    }

    let output = child.wait_with_output().expect("failed to wait on child");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "binary exited non-zero ({}):\nstderr: {}",
            output.status, stderr
        );
    }

    String::from_utf8(output.stdout)
        .expect("non-UTF8 stdout")
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------
// Canonical input A — astraea-core/src/lib.rs → all 15 workspace crates
// ---------------------------------------------------------------------------

#[test]
fn test_core_lib_impacts_all_15_crates() {
    let actual = run_impact("crates/astraea-core/src/lib.rs\n");

    let parsed: Vec<String> =
        serde_json::from_str(&actual).expect("output is not valid JSON array");

    let expected: Vec<&str> = vec![
        "astraea-algorithms",
        "astraea-cli",
        "astraea-cluster",
        "astraea-core",
        "astraea-encrypt-demo",
        "astraea-flight",
        "astraea-gnn",
        "astraea-gpu",
        "astraea-graph",
        "astraea-mcp",
        "astraea-query",
        "astraea-rag",
        "astraea-server",
        "astraea-storage",
        "astraea-vector",
    ];

    assert_eq!(
        parsed.len(),
        15,
        "expected 15 impacted crates, got {}: {actual}",
        parsed.len()
    );

    let mut actual_sorted = parsed.clone();
    actual_sorted.sort();
    let expected_sorted: Vec<String> = expected.iter().map(|s| s.to_string()).collect();

    assert_eq!(
        actual_sorted, expected_sorted,
        "impacted crate list does not match expected 15-crate set.\nActual:   {actual_sorted:?}\nExpected: {expected_sorted:?}"
    );
}

// ---------------------------------------------------------------------------
// Canonical input B — astraea-cli/src/main.rs → exactly ["astraea-cli"]
// ---------------------------------------------------------------------------

#[test]
fn test_cli_main_impacts_only_cli() {
    let actual = run_impact("crates/astraea-cli/src/main.rs\n");

    assert_eq!(
        actual, r#"["astraea-cli"]"#,
        "expected exactly [\"astraea-cli\"], got: {actual}"
    );
}

// ---------------------------------------------------------------------------
// Canonical input C — astraea-storage/src/page.rs → 8-crate set
// (verified by prior dev-graph run, documented in task brief)
// ---------------------------------------------------------------------------

#[test]
fn test_storage_page_impacts_9_crates() {
    let actual = run_impact("crates/astraea-storage/src/page.rs\n");

    let parsed: Vec<String> =
        serde_json::from_str(&actual).expect("output is not valid JSON array");

    let expected: Vec<&str> = vec![
        "astraea-cli",
        "astraea-flight",
        "astraea-gnn",
        "astraea-graph",
        "astraea-mcp",
        "astraea-query",
        "astraea-rag",
        "astraea-server",
        "astraea-storage",
    ];

    // Derived from the list above rather than written out again: the two had
    // already drifted apart once.
    assert_eq!(
        parsed.len(),
        expected.len(),
        "expected {} impacted crates, got {}: {actual}",
        expected.len(),
        parsed.len()
    );

    let mut actual_sorted = parsed.clone();
    actual_sorted.sort();
    let expected_sorted: Vec<String> = expected.iter().map(|s| s.to_string()).collect();

    assert_eq!(
        actual_sorted, expected_sorted,
        "impacted crate list does not match expected 8-crate set.\nActual:   {actual_sorted:?}\nExpected: {expected_sorted:?}"
    );
}

// ---------------------------------------------------------------------------
// Edge case 1 — empty stdin → []
// ---------------------------------------------------------------------------

#[test]
fn test_empty_stdin_returns_empty_array() {
    // Truly empty (no bytes, no newline)
    let actual = run_impact("");

    assert_eq!(actual, "[]", "expected [] for empty stdin, got: {actual}");
}

// ---------------------------------------------------------------------------
// Edge case 2 — non-crate path → []
// ---------------------------------------------------------------------------

#[test]
fn test_non_crate_path_returns_empty_array() {
    let actual = run_impact("docs/versioning.md\n");

    assert_eq!(
        actual, "[]",
        "expected [] for non-crate path docs/versioning.md, got: {actual}"
    );
}
