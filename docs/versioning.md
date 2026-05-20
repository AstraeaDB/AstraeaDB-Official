# Versioning Policy

This document is the source of truth for how AstraeaDB tracks versions
across its 15 workspace crates. It is paired with
[regression-testing.md](regression-testing.md) (which describes how the
CI gate selects which tests to run) and
[versioning-implementation-plan.md](versioning-implementation-plan.md)
(which describes how this policy lands in the repo, file by file).

## Summary

AstraeaDB uses a **workspace-uniform** version. There is exactly one
version number for the entire workspace, declared at
`workspace.package.version` in the repo-root `Cargo.toml`. Every
member crate inherits that version via `version.workspace = true` in
its own `Cargo.toml`. A release bumps the single version field, tags
the commit on the `official` remote, and records the change in the
top-level `CHANGELOG.md`. Per-crate independent semver is explicitly
deferred until external consumers begin pinning individual AstraeaDB
crates from crates.io; switching to per-crate versions is a future
migration and is out of scope for this policy.

Cross-language clients (`go/`, `java/`, `python/` directories) version
independently from the Rust workspace. Their release cadence and
semver scheme are not governed by this document.

## Version bump rules

AstraeaDB is currently at `0.1.0`, which is pre-1.0. The bump rules
differ before and after the 1.0 boundary.

### Pre-1.0 (current era, `0.MINOR.PATCH`)

While the version's major component is `0`, the workspace follows the
Cargo / SemVer pre-1.0 convention:

- **MINOR bump** (`0.1.0` -> `0.2.0`) — any breaking change to a
  public Rust API in any crate. This includes:
  - removing or renaming a public item (`pub fn`, `pub struct`,
    `pub trait`, `pub enum`, `pub mod`, public type alias, public
    constant);
  - changing the signature of a public function or method;
  - changing the fields, variants, or generic parameters of a public
    type in a way that breaks downstream callers;
  - changing on-disk or wire formats (storage page layout, Flight
    schema, MCP message shape) in a non-backward-compatible way;
  - removing or renaming a Cargo feature flag.
- **PATCH bump** (`0.1.0` -> `0.1.1`) — any additive or non-breaking
  change. This includes:
  - adding a new public item to any crate;
  - adding a new method to an existing public trait that has a default
    implementation;
  - adding a new Cargo feature flag that defaults to off;
  - bug fixes that do not change public signatures;
  - performance improvements with no API change;
  - upgrading an internal dependency in a way that does not change
    the surface API of any AstraeaDB crate.

Note: under standard Cargo SemVer, adding a new public item is
strictly additive and qualifies as a PATCH bump in the pre-1.0 era.
We adopt that convention here.

### Post-1.0 (future, `MAJOR.MINOR.PATCH`)

Once the workspace ships `1.0.0`, the standard SemVer rules apply:

- **MAJOR bump** — any breaking change to any public API in any crate,
  or any non-backward-compatible change to a persisted format.
- **MINOR bump** — additive changes: new public items, new features
  flagged off by default, new methods on traits with defaults.
- **PATCH bump** — bug fixes and internal changes with no surface
  effect.

The trigger for the 1.0.0 cut is a separate decision, not specified
here.

## When to bump

The version-bump gate runs on every PR (see
[regression-testing.md](regression-testing.md) for the gate
mechanics). The gate fails unless `workspace.package.version` in the
repo-root `Cargo.toml` has advanced relative to the merge base, **and**
the PR has touched at least one source-bearing file. The rules:

**A bump is required if the PR touches any of:**
- `crates/*/src/**` (any Rust source file under any crate);
- `crates/*/Cargo.toml`, except changes confined to the
  `[dev-dependencies]` section;
- `Cargo.toml` (workspace root) outside `[workspace.dependencies]`
  dev-only entries;
- any file that participates in a published artifact: build scripts
  (`build.rs`), code-generation inputs (`.proto`, `.fbs`), embedded
  assets.

**A bump is not required for:**
- changes confined to `docs/`, `README.md`, `CHANGELOG.md` itself,
  `.github/`, top-level config that does not affect compilation
  (`.editorconfig`, `rustfmt.toml`, `clippy.toml`), or any non-Rust
  subdirectory (`go/`, `java/`, `python/`, `examples/` that are not
  workspace members);
- formatting-only changes verified by `cargo fmt --check`.

**The gate is path-based.** It cannot tell a comment-only edit
from a real code change, so any edit under `crates/X/` — even a
rustdoc tweak in `crates/astraea-core/src/lib.rs` — requires a
version bump. The conservative behavior is intentional: detecting
"comment-only" diffs reliably would need parser-aware diffing,
which the current gate does not do. If you find this annoying in
practice, see the [implementation plan](versioning-implementation-plan.md)
for the future-work item that adds finer detection.

Recommendation: when in doubt, bump PATCH and add a one-line
CHANGELOG entry that says "no API change" if appropriate. Over-
bumping is cheap; under-bumping is what the gate exists to catch.

## CHANGELOG format

History lives in a single repo-root `CHANGELOG.md`. The file follows
[Keep a Changelog](https://keepachangelog.com/) conventions, adapted
for our workspace-uniform scheme.

Layout rules:

- One H2 section per release, header `## [VERSION] - YYYY-MM-DD`.
  Versions are listed newest first.
- An `## [Unreleased]` section sits at the top while a release is
  being prepared.
- Within each release, group entries under H3 subsections in this
  order: `### Added`, `### Changed`, `### Fixed`, `### Removed`,
  `### Deprecated`, `### Security`. Omit any subsection that has no
  entries.
- Each bullet within a subsection is keyed by crate name in bold,
  followed by a short imperative description. The crate prefix is
  mandatory so a reader can grep for changes to a specific crate
  across releases.

Example block:

```markdown
## [0.2.0] - 2026-06-01

### Added
- **astraea-core:** add `Direction::Both` variant for traversals.
- **astraea-graph:** add `Graph::neighbors_typed` returning typed
  neighbor records.

### Changed
- **astraea-storage:** page header now records a 32-bit CRC; existing
  pages are read with the old format and rewritten on next flush.
- **astraea-cli:** rename `--graph-path` flag to `--data-dir`.

### Fixed
- **astraea-flight:** correct schema arrow encoding for nullable
  list children.
- **astraea-mcp:** stop dropping the trailing newline on tool stdout.
```

Rules that follow from the layout:

- A breaking change (MINOR bump pre-1.0, MAJOR bump post-1.0) **must**
  appear under `### Changed` or `### Removed`, never under `### Added`
  alone.
- A bullet that affects more than one crate may either be written as
  one bullet keyed by the leading crate with a parenthetical
  ("**astraea-core:** rename `Node::label` to `Node::kind` (callers
  in astraea-graph, astraea-storage updated)"), or split into one
  bullet per crate. Either is acceptable; consistency within a
  release is preferred.

## Release process

A release is a single commit on `main` that bumps
`workspace.package.version` and updates `CHANGELOG.md`, followed by a
tag on the `official` remote.

Tag scheme: `v<VERSION>`, for example `v0.2.0`. The `v` prefix is
mandatory. Tags are annotated.

Release source-of-truth remote: `official`. The repo also has `origin`
and `public` remotes; releases are not driven from those. The
version-gate workflow runs on PRs targeted at `official`'s `main`
branch.

Step by step:

1. Open a PR against `main` that:
   - sets `workspace.package.version` to the new version in the root
     `Cargo.toml`;
   - moves the contents of `## [Unreleased]` in `CHANGELOG.md` under a
     new `## [<VERSION>] - <YYYY-MM-DD>` heading;
   - leaves an empty `## [Unreleased]` block at the top for future
     work.
2. The version-gate runs the impact analysis, fans out tests, and
   verifies the bump and CHANGELOG entry are present.
3. On approval, merge the PR to `main`.
4. A human (release manager) runs:

   ```
   git fetch official
   git checkout official/main
   git tag -a v<VERSION> -m "Release v<VERSION>"
   git push official v<VERSION>
   ```

5. Tag creation triggers no automation in v1 of this policy. A
   follow-up may add `release-plz` or a hand-rolled tag script that
   reads `CHANGELOG.md` and drafts a GitHub release; that work is
   tracked in
   [versioning-implementation-plan.md](versioning-implementation-plan.md)
   step 7.

Hotfix releases follow the same procedure. There is no separate
release branch; hotfixes land on `main` like any other change.

## Acceptance test

Two worked examples a reviewer can answer from this document alone.

### Example 1 — "I added a new public fn to astraea-core."

You added a new `pub fn` to `crates/astraea-core/src/types.rs`. No
existing signature changed; nothing was removed.

- **Bump:** PATCH. Pre-1.0, additive changes are PATCH. If the current
  workspace version is `0.1.0`, set it to `0.1.1` in the root
  `Cargo.toml`.
- **CHANGELOG:** add a bullet under `### Added` in the
  `## [0.1.1] - <today>` block:
  `- **astraea-core:** add <name of new fn> for <reason>.`
- **Tests:** the gate will fan out tests to every crate that depends
  on `astraea-core`; see
  [regression-testing.md](regression-testing.md). You do not pick
  the matrix; the gate does.

### Example 2 — "I fixed a typo in a doc comment."

You edited a `///` rustdoc comment in `crates/astraea-graph/src/lib.rs`
and changed nothing else.

- **Bump:** PATCH (`0.1.0` → `0.1.1`). The gate is path-based; any
  edit under `crates/X/` requires a bump even when the diff is
  purely a comment. This is conservative on purpose — see the
  [When to bump](#when-to-bump) section.
- **CHANGELOG:** one bullet under `### Changed`:
  `- **astraea-graph:** doc-comment fixes (no API change).`
- **Tests:** the regression-test matrix runs `cargo test -p
  astraea-graph` (and every downstream crate). This is also
  conservative; it does not slow the gate enough to matter today.

If a comment-only edit is genuinely too cheap to bump for (e.g.
in a stacked PR series), the contributor's options are: (a) batch
the comment fix into the next source-bearing PR; or (b) wait for
the parser-aware diff exemption tracked in the implementation plan
as a future improvement.
