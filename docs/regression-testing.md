# Regression Testing Policy

This document describes how AstraeaDB's CI gate decides which crates'
tests must run for a given pull request. It is paired with
[versioning.md](versioning.md) (the workspace-uniform version policy
the gate also enforces) and
[versioning-implementation-plan.md](versioning-implementation-plan.md)
(the file-by-file landing order for the gate itself).

## Summary

AstraeaDB has 15 workspace crates with internal dependencies between
them. A change to a low-level crate like `astraea-core` can break any
crate that depends on it; a change to a leaf crate like `astraea-cli`
affects nothing else. The regression-testing gate exploits the crate
dependency graph: for each pull request, an impact-analysis tool
computes the set of crates transitively impacted by the changed files
and emits that set as a JSON matrix. The GitHub Actions workflow
consumes the matrix and runs `cargo test -p $crate`, `cargo clippy -p
$crate`, and `cargo fmt --check` for each impacted crate in parallel.
This avoids both extremes — running every test on every PR, and
running only the directly-edited crate's tests — and gives a reviewer
a deterministic, predictable matrix.

## The impact-analysis algorithm

The impact tool is a small Rust binary that lives in this repo's
sibling development environment at
`/Users/jimharris/Documents/astraea-development/projects/version-control/`.
It is built once per CI run and invoked by the workflow with the list
of changed file paths on stdin. The tool itself uses an embedded
AstraeaDB instance (`astraea-graph` with `InMemoryStorage`) to model
the crate graph — this is both the simplest reverse-BFS available and
a dogfood demo of the database.

The algorithm has five steps. Each step lines up with a module in the
impact tool:

1. **Discover the workspace** — call
   `cargo_metadata::MetadataCommand::new().exec()` against the
   AstraeaDB repo. The result lists every workspace member, its
   manifest path, and its declared dependencies. See
   `projects/version-control/src/crate_graph.rs`.

2. **Build the crate graph** — create one `Crate` node per workspace
   member in an embedded `astraea_graph::Graph` backed by
   `InMemoryStorage::default()`. Node properties carry the crate's
   `name`, `version`, and `path` (the directory containing its
   `Cargo.toml`). For each `[dependencies]` or `[dev-dependencies]`
   entry in each crate's manifest whose name matches another
   workspace member, add a `DEPENDS_ON` edge from the source crate
   to the target. Dev-dependencies are included on purpose: a
   change to A can break B's tests when B only test-depends on A,
   and the gate exists to catch exactly that. Build dependencies
   (`[build-dependencies]`) are excluded — they change rarely and
   the noise isn't worth the matrix expansion. See
   `projects/version-control/src/crate_graph.rs`.

3. **Map changed files to crates** — read the changed-file list (one
   path per line on stdin, repo-relative). For each path, find the
   workspace member whose `path` property is the longest prefix of
   that file path. Paths that match no crate (for example, top-level
   `Cargo.toml`, `docs/`, or `.github/`) are handled by special rules
   described below. See `projects/version-control/src/impact.rs`.

4. **Reverse-BFS the closure** — starting from the set of directly-
   changed crates, walk
   `neighbors_filtered(node, Direction::Incoming, "DEPENDS_ON")` on
   the embedded graph in a worklist loop until no new crates are
   added. The result is the transitive set of crates that depend on
   any changed crate. The directly-changed crates are included in
   the result. See `projects/version-control/src/impact.rs`.

5. **Emit the matrix** — print the impacted crate set as a JSON array
   on stdout, sorted lexicographically for stable output. See
   `projects/version-control/src/main.rs`.

Special rules for non-crate paths:

- A change to the **workspace root `Cargo.toml`** (for example a
  bumped workspace dependency version) is treated as a change to every
  workspace member.
- A change to a **build-system file** that affects compilation (the
  workspace root `rust-toolchain.toml`, `.cargo/config.toml`, or any
  `build.rs`) is treated as a change to every workspace member.
- A change confined to `docs/`, `CHANGELOG.md`, `.github/`,
  `README.md`, `go/`, `java/`, `python/`, or any other non-Rust
  subtree produces an empty impact set; the workflow short-circuits
  the test fan-out (lint and format gates may still run on the docs
  themselves).
- A change anywhere under `crates/X/` (including
  `crates/X/tests/` or `#[cfg(test)]` blocks inside
  `crates/X/src/`) is treated as a change to `X` and triggers the
  full reverse-BFS closure. The conservative behavior is
  intentional in the first cut: if you only edited a test, the
  matrix may run more downstream tests than strictly necessary,
  but it will not miss any breakage. Suppressing the closure when
  only test files changed is a future optimization tracked
  separately; it requires distinguishing `crates/X/src/` from
  `crates/X/tests/` and parsing `#[cfg(test)]` regions out of
  source files, neither of which the current tool does.

## Granularity

The gate runs tests at **per-crate** granularity:

```
cargo test -p <crate>
```

for each crate in the impacted set, in parallel via the GitHub
Actions matrix. We do not use `cargo nextest` filtersets or any
finer-grained selection.

This choice reflects the current shape of the codebase: 61 source
files contain `#[test]` attributes, all of them inline
`#[cfg(test)]` modules. There are no `crates/*/tests/` integration
directories today, so a per-test selector would buy little and
introduce significant configuration cost (filter strings, regex
maintenance, nextest installation in CI). Per-crate granularity is
also predictable: a contributor can hand-trace which crates will be
tested without running the tool.

**Revisit condition.** The granularity is right while wall-clock time
of the impacted matrix stays under 15 minutes. When that ceiling is
hit, the next iteration should consider:

- caching with `Swatinem/rust-cache` (low cost, big win, do this
  first);
- splitting the matrix into a fast-feedback subset (changed crate
  only) and a full-closure subset (run after fast feedback passes);
- introducing `cargo nextest` for parallelism within a crate;
- finally, dropping to per-test selectors.

Until 15 minutes is breached, keep the matrix coarse.

## Lint and format gates

Lint and format gates fan out the same way as tests, using the same
impacted-crate matrix:

- `cargo clippy -p $crate --all-targets -- -D warnings` — per impacted
  crate. `-D warnings` makes any clippy lint a CI failure.
- `cargo fmt --check` — workspace-wide, run once at the start of the
  workflow. Formatting is cheap and global; a per-crate fan-out buys
  nothing.

`cargo doc` is not gated in v1; the build is slow and the lint signal
is weak. A follow-up may add `cargo doc -p $crate --no-deps` to the
matrix once doc coverage matters.

## CI fan-out

The workflow lives at `.github/workflows/version-gate.yml` in the
AstraeaDB repo. Its shape:

1. A `build-impact-tool` job checks out the AstraeaDB repo and the
   sibling `astraea-development` repo, runs `cargo build --release`
   on `projects/version-control/`, and uploads the binary as a
   workflow artifact.
2. A `compute-impact` job downloads the artifact, runs
   `git diff --name-only origin/main...HEAD` to produce the
   changed-file list, pipes it into the impact tool, and outputs
   the JSON matrix as a job output.
3. A `test-matrix` job declares `strategy.matrix.crate:
   ${{ fromJSON(needs.compute-impact.outputs.crates) }}` and runs
   `cargo test -p ${{ matrix.crate }}` per entry.
4. A `lint-matrix` job uses the same matrix to run clippy per crate.
5. A `fmt` job runs `cargo fmt --check` once.
6. A `version-bump-check` job and a `changelog-check` job consume
   the impact output to enforce the policy in
   [versioning.md](versioning.md).

A small YAML excerpt illustrating the matrix wiring:

```yaml
jobs:
  compute-impact:
    runs-on: ubuntu-latest
    outputs:
      crates: ${{ steps.run.outputs.crates }}
      count: ${{ steps.run.outputs.count }}
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - id: run
        run: |
          changed=$(git diff --name-only origin/main...HEAD)
          impacted=$(printf '%s\n' "$changed" | ./bin/astraea-embedded-template --stdin)
          count=$(printf '%s' "$impacted" | python3 -c 'import sys, json; print(len(json.load(sys.stdin)))')
          { echo "crates=$impacted"; echo "count=$count"; } >> "$GITHUB_OUTPUT"

  test-matrix:
    needs: compute-impact
    if: ${{ needs.compute-impact.outputs.count != '0' }}
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        crate: ${{ fromJSON(needs.compute-impact.outputs.crates) }}
    steps:
      - uses: actions/checkout@v4
      - run: cargo test -p ${{ matrix.crate }}
```

The binary name `astraea-embedded-template` reflects the impact
tool's `Cargo.toml` package name. The gate condition keys off a
separate `count` output rather than comparing the JSON array to the
string `'[]'`; both work, but `count` is robust against trailing
whitespace.

The full workflow with all jobs is described in
[versioning-implementation-plan.md](versioning-implementation-plan.md)
step 3.

## Predictability examples

A reviewer should be able to predict the matrix without running the
tool. The graph rarely changes, and the rules are simple. Three
worked inputs:

### Input A — `changed = {crates/astraea-core/src/lib.rs}`

`astraea-core` is the workspace's root crate; every other crate
depends on it directly or transitively. The impacted set is all 15
crates:

```
astraea-algorithms, astraea-cli, astraea-cluster, astraea-core,
astraea-crypto, astraea-flight, astraea-gnn, astraea-gpu,
astraea-graph, astraea-mcp, astraea-query, astraea-rag,
astraea-server, astraea-storage, astraea-vector
```

That is the maximally-expensive matrix. Any change to
`crates/astraea-core/` should expect this.

### Input B — `changed = {crates/astraea-cli/src/main.rs}`

`astraea-cli` is a leaf binary crate. No other workspace member
depends on it. The impacted set is just:

```
astraea-cli
```

That is the minimally-expensive matrix.

### Input C — `changed = {crates/astraea-core/src/types.rs, crates/astraea-graph/src/lib.rs}`

Both `astraea-core` and `astraea-graph` are in the changed set.
`astraea-graph` depends on `astraea-core` (and `astraea-storage`),
and almost every other crate depends on `astraea-graph`. The
closure is identical to Input A — every crate in the workspace:

```
astraea-algorithms, astraea-cli, astraea-cluster, astraea-core,
astraea-crypto, astraea-flight, astraea-gnn, astraea-gpu,
astraea-graph, astraea-mcp, astraea-query, astraea-rag,
astraea-server, astraea-storage, astraea-vector
```

A reviewer who knows the rough shape of the dependency DAG can call
the matrix in their head: identify the lowest crate in the changed
set, and the impacted matrix is "that crate and everything above it."
For changes confined to leaf crates (`astraea-cli`, `astraea-mcp`,
`astraea-flight`, `astraea-rag`, `astraea-gnn`, `astraea-cluster`,
`astraea-gpu`, `astraea-algorithms`, `astraea-crypto`), the matrix is
usually just the changed crate.
