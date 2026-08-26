//! astraeadb-issues.md #29: keep the `astraeadb` alias package in step.
//!
//! `crates/astraeadb` publishes the same binary under the name people guess,
//! so that `cargo install astraeadb` works alongside `cargo install
//! astraea-cli`. It is excluded from the workspace, because two members
//! emitting a binary of the same name collide in one target directory. The
//! cost of excluding it is that it inherits nothing: its version and its
//! dependency on this crate are literals, and nothing in a normal build would
//! notice them going stale.
//!
//! This test is that notice. It lives here, inside the workspace, so it runs in
//! CI.

use std::path::Path;

fn field(manifest: &str, key: &str) -> Option<String> {
    manifest
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split('"').nth(1).map(str::to_string))
}

#[test]
fn alias_package_version_matches() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let alias_path = here.join("../astraeadb/Cargo.toml");
    let alias = std::fs::read_to_string(&alias_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", alias_path.display()));

    let ours = env!("CARGO_PKG_VERSION");

    let theirs = field(&alias, "version = ").expect("astraeadb has no version");
    assert_eq!(
        theirs, ours,
        "crates/astraeadb is at {theirs} but astraea-cli is at {ours}. \
         The alias package is excluded from the workspace, so it does not \
         inherit the bump. Edit crates/astraeadb/Cargo.toml to match."
    );

    // The dependency pin has to move with it, or a published astraeadb would
    // pull an older astraea-cli from crates.io than the one it was built and
    // tested against here.
    let dep = alias
        .lines()
        .find(|l| l.trim_start().starts_with("astraea-cli = "))
        .expect("astraeadb does not depend on astraea-cli");
    assert!(
        dep.contains(&format!("version = \"{ours}\"")),
        "crates/astraeadb pins astraea-cli as `{}`, but astraea-cli is at {ours}",
        dep.trim()
    );
}

#[test]
fn alias_package_ships_the_same_binary_name() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let alias = std::fs::read_to_string(here.join("../astraeadb/Cargo.toml")).unwrap();
    assert!(
        alias.contains("name = \"astraeadb\""),
        "the alias package must ship a binary called astraeadb; that is the \
         entire point of it"
    );
}
