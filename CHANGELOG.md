# Changelog

All notable changes to AstraeaDB are recorded here. This file follows
[Keep a Changelog](https://keepachangelog.com/) conventions, adapted
for AstraeaDB's workspace-uniform versioning scheme — one version
number for all 15 workspace crates. See
[docs/versioning.md](docs/versioning.md) for the bump policy and
[docs/regression-testing.md](docs/regression-testing.md) for what the
gate enforces.

The format of each release entry is:

```
## [X.Y.Z] - YYYY-MM-DD

### Added | Changed | Fixed | Removed
- **crate-name:** short imperative description of the change.
```

Use the H2 release header `## [X.Y.Z] - YYYY-MM-DD` exactly — the
`changelog-check` job in `.github/workflows/version-gate.yml` greps
for `^## \[$head_ver\]`. Crate-keyed bullets group changes for
readers; the gate does not validate bullet content.

## [Unreleased]

### Added
- (next release notes go here — keep this section as the working
  draft, then rename to `## [X.Y.Z] - YYYY-MM-DD` at release time.)

## [0.1.10] - 2026-06-15

### Added
- **astraea-core:** `GraphOps::flush()` default method (no-op) so any
  backend can opt into a real flush without breaking existing
  implementors.
- **astraea-graph:** `Graph::flush()` override delegating to
  `storage.flush()` (buffer-pool flush + WAL checkpoint).
- **astraea-cli:** SIGTERM/SIGINT handler in `Commands::Serve` that
  calls `graph.flush()` then `conn_mgr.initiate_shutdown()` so the
  server drains and exits cleanly. Closes the durability gap on
  issue #1.
- **astraea-storage:** `test_durability_across_drop_and_reopen` —
  writes 50 nodes + 49 edges with a buffer pool small enough to force
  evictions, drops the engine without flush(), reopens, asserts every
  node and edge recovers from WAL replay.

### Fixed
- **astraea-server:** server restart no longer drops dirty buffer-pool
  pages on a clean shutdown. WAL replay was already wired; the missing
  piece was the SIGTERM/SIGINT handler.

## [0.1.9] - 2026-05-21

### Added
- **astraea-cli:** configurable embedding vector dimension and distance
  metric via a `[vector]` config section (`dimension`, `metric`),
  replacing the hard-coded 128-dim cosine index. Omitting `[vector]`
  keeps the previous 128-dim cosine default for back-compatibility.
- **astraea-vector:** `load_from_file_with_dimension` and
  `HnswIndex::load_expecting_dimension` to enforce an expected dimension
  on load (returns `DimensionMismatch` on mismatch).

### Fixed
- **astraea-vector:** persisted-index dimension is now written via a
  checked `u32::try_from` cast, returning a `Serialization` error instead
  of silently truncating when the dimension exceeds `u32::MAX`.

## [0.1.8] - 2026-05-20

### Changed
- **astraea-query:** clear five clippy errors —
  `clippy::while_let_loop` in `src/parser.rs:231` (collapse
  `loop { if let Some(edge) = ... { ... } else { break; } }` to
  `while let Some(edge) = ... { ... }`); two
  `clippy::redundant_locals` in `src/parser.rs:411,416` (delete
  the no-op `let n = n;` / `let f = f;` shadowings introduced by
  earlier refactors); and three `clippy::approx_constant` false
  positives in test data — `src/lexer.rs:313`,
  `src/executor.rs:1787/1790` used `3.14` as arbitrary float
  literals which clippy reads as approximations of
  `std::f32::consts::PI`. Replaced with `2.5` (with a one-line
  comment at each site explaining the avoidance); the lexer
  parsing test and the negation evaluator test are equally
  served by `2.5`.

## [0.1.7] - 2026-05-20

### Changed
- **astraea-rag:** introduce `type HttpFn = Box<dyn Fn(&str,
  &serde_json::Value) -> Result<String> + Send + Sync>` in
  `src/llm.rs` and apply it at four field declarations
  (`OpenAiProvider::http_fn`, `AnthropicProvider::http_fn`,
  `OllamaProvider::http_fn`, `OllamaProvider::embed_http_fn`).
  Silences four `clippy::type_complexity` errors and makes the
  field signatures readable. No public API change — the underlying
  type is unchanged, just named.

## [0.1.6] - 2026-05-20

### Changed
- **astraea-gnn:** clear clippy debt — refactor two
  `needless_range_loop` sites to iterators
  (`crates/astraea-gnn/src/sparse.rs:213` row-scaling, and
  `src/tensor.rs:255` `matvec` outer loop), add
  `#[allow(clippy::needless_range_loop)]` with parallel-array
  rationale to two sites where iterator rewrites would only push
  the index burden onto a sibling array
  (`src/tensor.rs:284` `transpose_matvec` inner loop and
  `src/sparse.rs:362` test comparison loop), and replace the
  `3.14` test data in `test_tensor_from_scalar` with `2.5` to
  avoid `clippy::approx_constant` (clippy reads `3.14` as an
  approximation of PI; the test only needs an arbitrary scalar).

## [0.1.5] - 2026-05-20

### Changed
- **astraea-gpu:** add `#[allow(clippy::needless_range_loop)]` to
  two parallel-array indexing sites in `src/cpu.rs:50` (PageRank
  inner loop) and `src/csr.rs:120` (sparse matrix-vector product).
  Clippy's suggested `.iter_mut().enumerate()` rewrite would make
  the code worse because each loop body indexes a second array
  (`transposed.row_ptr[i]`, `self.row_ptr[i+1]`) that has no
  iterator on the iterating side — `i` is unavoidable. Each allow
  carries a one-paragraph comment explaining the parallel-array
  reasoning so future readers don't try to "fix" it.

## [0.1.4] - 2026-05-20

### Changed
- **astraea-storage:** fix `clippy::doc_overindented_list_items`
  warning in `object_store_cold.rs:85` — the second line of a
  bulleted list item was indented 13 spaces past the bullet
  (rustfmt-acceptable but flagged by clippy 1.95). Reduced to the
  conventional 2-space continuation. Doc-only change, no behavior
  impact, but `crates/astraea-storage/src/` is source-bearing so
  the version-gate requires a bump.

## [0.1.3] - 2026-05-20

### Changed
- **workspace:** auto-fixable clippy sweep via
  `cargo clippy --fix --workspace --all-targets`. 27 files
  touched across cli, cluster, gnn, gpu, graph, mcp, query, rag,
  server, storage, vector. Lints cleared (where present per crate):
  `clippy::collapsible_if`, `clippy::unwrap_or_default`,
  `clippy::unnecessary_sort_by`, plus opportunistic mechanical
  rewrites picked up by `--fix` for `clippy::redundant_closure`,
  `clippy::needless_range_loop` (partial), and others. All rewrites
  are behavior-preserving by clippy's machine-applicable
  classification; `cargo test --workspace` passes locally.
- **astraea-graph:** replace the empty `criterion_main!()`
  invocation in `benches/traversal_bench.rs` with a no-op `main` so
  the bench target compiles under `cargo clippy --all-targets`.
  The bench is wired into `Cargo.toml` but had no defined groups;
  this preserves the no-op intent while unblocking the
  `Clippy astraea-graph` job in the version-gate.

### Notes
- Per-crate clippy debt remains in astraea-gnn, astraea-gpu,
  astraea-query, astraea-rag, and astraea-storage. These are
  manual-fix lints (`approx_constant`, `type_complexity`,
  `needless_range_loop` residuals, `while_let_loop`,
  `redundant_locals`, `doc_overindented_list_items`) and will land
  as Wave 2 follow-up PRs per `docs/clippy-cleanup-plan.md`
  (local-only, not committed). The `lint-matrix` jobs for those
  crates will still be red after this PR.

## [0.1.2] - 2026-05-20

### Changed
- **workspace:** apply `cargo fmt --all` across the workspace.
  No API change, no behavioral change — purely whitespace and
  formatting normalization to bring the tree in line with rustfmt
  defaults. Required to unblock the `fmt` job in the
  `.github/workflows/version-gate.yml` gate.

## [0.1.1] - 2026-05-20

### Changed
- **astraea-core:** annotate `GraphOps::create_edge` with
  `#[allow(clippy::too_many_arguments)]`. The trait method takes 8
  parameters, which exceeds clippy's default threshold of 7;
  without the allow, every crate that depends on astraea-core (i.e.
  every other workspace crate) fails `cargo clippy -- -D warnings`.
  A future PR may refactor `create_edge` to take a builder/options
  struct; the allow is the stopgap that unblocks the version-gate's
  `lint-matrix` jobs.

## [0.1.0] - 2026-05-19

Baseline release. This entry catalogues the state of the workspace
at the time the version gate was introduced; it does not enumerate
prior development history.

### Added
- **astraea-core:** initial release — `NodeId`, `EdgeId`, `Direction`,
  the `GraphOps` trait, and shared type definitions used by every
  other crate.
- **astraea-storage:** initial release — pluggable storage engine
  interfaces (`StorageEngine`), page-based on-disk layout, write-
  ahead log, and the storage iterators consumed by the graph layer.
- **astraea-graph:** initial release — `Graph::new`, `create_node`,
  `create_edge`, `neighbors_filtered`, `bfs`, and the
  `test_utils::InMemoryStorage` backend used by embedded demos.
- **astraea-query:** initial release — query parser, planner, and
  executor over the graph and storage layers.
- **astraea-vector:** initial release — HNSW-based vector index and
  similarity-search APIs integrated with the graph layer.
- **astraea-server:** initial release — single-node gRPC/HTTP server
  binary, configuration loader, and runtime supervisor.
- **astraea-flight:** initial release — Apache Arrow Flight transport
  for high-throughput client/server interchange.
- **astraea-cli:** initial release — `astraea` command-line client
  for launching servers, running queries, and inspecting state.
- **astraea-rag:** initial release — retrieval-augmented generation
  helpers (chunkers, embedders, retrievers) that sit on top of the
  graph + vector layers.
- **astraea-gnn:** initial release — graph neural network primitives
  and training-loop scaffolding for AstraeaDB-resident graphs.
- **astraea-cluster:** initial release — multi-node coordination
  primitives (consensus client, shard placement) for future cluster
  deployments.
- **astraea-gpu:** initial release — GPU-accelerated kernels and
  feature-gated bindings consumed by the vector and gnn crates.
- **astraea-algorithms:** initial release — graph algorithms library
  (BFS/DFS extensions, PageRank, community detection) built against
  `GraphOps`.
- **astraea-crypto:** initial release — cryptographic helpers (hash,
  sign, verify) used by storage integrity and cluster authentication.
- **astraea-mcp:** initial release — Model Context Protocol server
  exposing AstraeaDB to MCP-aware clients (Claude Code, etc.).
