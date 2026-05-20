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
