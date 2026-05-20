# Versioning Implementation Plan

This document is the file-by-file landing order for the versioning
and regression-testing policies. It is paired with
[versioning.md](versioning.md) (the policy itself) and
[regression-testing.md](regression-testing.md) (the test-selection
algorithm). A fresh contributor should be able to read this document
top to bottom and land every item without further questions.

## Goal & scope

Land the workspace-uniform versioning policy and the impact-driven
regression-testing gate inside the AstraeaDB repository. The work
consists of three new docs (already drafted), one new top-level
`CHANGELOG.md`, one new GitHub Actions workflow at
`.github/workflows/version-gate.yml`, and a one-time branch-protection
flip on the `official` remote. No AstraeaDB source code (the contents
of `crates/`) changes as part of this plan. The impact-analysis tool
that the gate shells out to is a separate deliverable in
`/Users/jimharris/Documents/astraea-development/projects/version-control/`
and is referenced here as a precondition, not built here.

## Pre-flight

Three preconditions must be true before the gate is enforceable.

1. **Impact tool builds and runs.** In the sibling repo at
   `/Users/jimharris/Documents/astraea-development/projects/version-control/`,
   `cargo build --release` must succeed and the resulting binary
   must print a JSON array when handed a stdin list of changed file
   paths. The tool's modules are described in
   [regression-testing.md](regression-testing.md). If the tool does
   not yet exist on disk, that is a blocking dependency for step 3
   below — but steps 1, 2, 6, and 7 can land first.
2. **Docs are merged.** The three documents `docs/versioning.md`,
   `docs/regression-testing.md`, and this file land before any CI
   wiring, so contributors and reviewers have something to point at
   when the first gate failure shows up.
3. **CHANGELOG baseline exists.** `CHANGELOG.md` at the repo root is
   seeded with a `## [0.1.0]` entry summarizing the current state of
   the workspace. Without this baseline, the changelog-check step in
   the workflow has no anchor against which to diff.

## Landing order

The work lands in seven numbered steps. Steps 1, 2, and 6 are pure
docs/content and have no dependencies. Step 3 depends on the impact
tool. Step 4 depends on step 3. Step 5 is a manual GitHub action.
Step 7 is optional and may be deferred indefinitely.

### Step 1 — Land `CHANGELOG.md` baseline

Create `CHANGELOG.md` at the repo root. Use the layout described in
[versioning.md](versioning.md) ("CHANGELOG format" section). The
baseline records every existing crate as the initial release. The
full block to commit is in the "File diffs" section below.

PR scope: one new file. No code changes. Self-merging.

### Step 2 — Land `docs/versioning.md` and `docs/regression-testing.md`

Both files are already drafted alongside this plan. They cross-link
to each other and to this file. Commit them in a single PR so the
cross-links resolve immediately.

PR scope: two new files. No code changes. Self-merging.

### Step 3 — Land `.github/workflows/version-gate.yml` in advisory mode

Create the workflow at `.github/workflows/version-gate.yml`. Job
shape (jobs are described in detail in
[regression-testing.md](regression-testing.md)):

- `build-impact-tool` — clones the sibling dev-env repo, rewrites
  the absolute `path = "/Users/.../astraeadb"` deps in
  `projects/version-control/Cargo.toml` to relative form (sed step),
  runs `cargo build --release` of the `astraea-embedded-template`
  package (the impact tool's actual package name — see
  `projects/version-control/Cargo.toml` line 2), and uploads the
  resulting binary as a workflow artifact.
- `compute-impact` — downloads the binary, runs `git diff --name-only
  origin/main...HEAD`, pipes to the binary, exposes the JSON array as
  a job output named `crates`.
- `test-matrix` — `strategy.matrix.crate: fromJSON(...)` running
  `cargo test -p $crate`.
- `lint-matrix` — same matrix, running
  `cargo clippy -p $crate --all-targets -- -D warnings`.
- `fmt` — single job running `cargo fmt --check`.

Advisory mode is achieved by **not** adding the workflow to
required status checks on the `official` remote. The YAML itself
does not need `continue-on-error: true` — branch protection (a
GitHub UI setting, not a YAML setting) is the binary switch
between advisory and required. The workflow runs on every PR
either way; in advisory mode a failed job surfaces as a red X
that the reviewer can read, but it does not block merge.

The goal of this landing is to observe the gate firing on real PRs
over one or two weeks, confirm the matrix is what was predicted,
and fix any edge cases the impact tool surfaces (path mapping bugs,
generated files, submodules) before any contributor is blocked.

Exit criterion for advisory mode: ten consecutive PRs whose matrix
matches the reviewer's hand prediction.

PR scope: one new file under `.github/workflows/`. No code changes.

### Step 4 — Add version-bump and CHANGELOG enforcement steps

Add two jobs to `.github/workflows/version-gate.yml`:

- `version-bump-check` — fails the PR if the impact set is non-empty
  and `workspace.package.version` in the root `Cargo.toml` has not
  advanced relative to the merge base. (Detecting the reverse "no-op
  bump" — version advanced without any source change — is a desirable
  follow-up but is **not** in the first cut; the path-based impact
  tool would already classify a version-bump-only PR as "no impact",
  so the version-bump-check job is gated on a non-empty impact set
  and simply does not run on the no-op case. Catching it cleanly is
  a step-7 improvement.)
- `changelog-check` — fails the PR if the impact set is non-empty and
  either (a) `CHANGELOG.md` was not edited in the PR, or (b) the
  file has no `## [<new-version>]` header. The job's regex is
  `^## \[$head_ver\]` — see `.github/workflows/version-gate.yml`
  job `changelog-check`.

These jobs are still advisory until step 5 (still not in branch
protection on `official`).

PR scope: edits to the workflow file from step 3. No code changes.

### Step 5 — Flip the workflow to required (manual, GitHub UI)

This is the only non-PR action in the plan. On the `official` remote
(the release source-of-truth, per
[versioning.md](versioning.md)), a repo admin opens
`Settings -> Branches -> Branch protection rules -> main` and adds
the following status checks to "Require status checks to pass before
merging":

- `version-gate / test-matrix`
- `version-gate / lint-matrix`
- `version-gate / fmt`
- `version-gate / version-bump-check`
- `version-gate / changelog-check`

`build-impact-tool` and `compute-impact` are upstream jobs; they
become required transitively.

After flipping the toggle, edit `.github/workflows/version-gate.yml`
in a follow-up PR to remove `continue-on-error: true` from every
job. This makes the failure mode explicit in the workflow file
itself, not just enforced at the branch-protection layer.

No PR is created for the toggle; the PR is the `continue-on-error`
removal.

### Step 6 — Document the release tag procedure

Confirm the release procedure section in
[versioning.md](versioning.md) reflects current reality, and add a
short `docs/release-checklist.md` (not in scope for this plan to
write; mentioned here so a future contributor knows where it belongs).
This step is bookkeeping; it does not unblock anything.

PR scope: optional doc change.

### Step 7 — (Optional, follow-up) Release tag automation

Either add `release-plz` (`https://release-plz.dev`) configured for
workspace-uniform mode, or write a hand-rolled shell script at
`scripts/tag-release.sh` that:

- reads the new version from `workspace.package.version`;
- reads the matching CHANGELOG section;
- creates an annotated tag on the current commit;
- pushes the tag to `official`.

This is explicitly out of scope for v1. The release-tag step in
[versioning.md](versioning.md) is human-driven until step 7 lands.

## File diffs

Concrete artifacts each step creates.

### `CHANGELOG.md` (step 1)

Place at the repo root. The baseline block:

```markdown
# Changelog

All notable changes to AstraeaDB are documented in this file.
See [docs/versioning.md](docs/versioning.md) for the policy and
[docs/regression-testing.md](docs/regression-testing.md) for how
the CI gate enforces it.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
adapted for AstraeaDB's workspace-uniform semver. Every release
applies to every workspace crate simultaneously.

## [Unreleased]

## [0.1.0] - 2026-05-19

Initial baseline. All 15 workspace crates ship at version 0.1.0.

### Added
- **astraea-core:** initial release — IDs, label enums, and the
  `GraphOps` trait surface shared by every other crate.
- **astraea-storage:** initial release — page-based storage engine
  and the `StorageEngine` trait implementations.
- **astraea-graph:** initial release — `Graph` API over
  `StorageEngine`, including `neighbors_filtered` and `bfs`.
- **astraea-query:** initial release — query planning and execution
  on top of `astraea-graph`.
- **astraea-vector:** initial release — vector index and similarity
  search primitives.
- **astraea-server:** initial release — gRPC server exposing the
  graph and query surfaces.
- **astraea-flight:** initial release — Arrow Flight transport and
  Flight SQL endpoints.
- **astraea-cli:** initial release — `astraea` command-line client.
- **astraea-rag:** initial release — retrieval-augmented-generation
  helpers on top of the query and vector surfaces.
- **astraea-gnn:** initial release — graph neural network primitives.
- **astraea-cluster:** initial release — clustering and partitioning
  utilities.
- **astraea-gpu:** initial release — GPU acceleration hooks.
- **astraea-algorithms:** initial release — graph algorithms
  (centrality, paths, community detection).
- **astraea-crypto:** initial release — cryptographic primitives
  (TLS material, signing).
- **astraea-mcp:** initial release — Model Context Protocol server
  exposing AstraeaDB to LLM tools.

[Unreleased]: ../../compare/v0.1.0...HEAD
[0.1.0]: ../../releases/tag/v0.1.0
```

The date `2026-05-19` is the date this plan was authored; if the
baseline lands later, update the date to the actual landing date.
The relative-URL compare links assume a GitHub remote; adjust if the
canonical `official` remote is elsewhere.

### `docs/versioning.md` and `docs/regression-testing.md` (step 2)

Already drafted and committed alongside this file. No stubs needed.

### `.github/workflows/version-gate.yml` (steps 3, 4, 5)

Lives at `.github/workflows/version-gate.yml`. The workflow has six
jobs (after step 4), described in detail in
[regression-testing.md](regression-testing.md):

1. `build-impact-tool` — checkout, `cargo build --release` of the
   sibling project, upload binary as artifact.
2. `compute-impact` — checkout, download artifact, run
   `git diff --name-only origin/main...HEAD`, pipe to binary, expose
   JSON as `crates` job output.
3. `test-matrix` — needs `compute-impact`, fans out
   `cargo test -p $crate` over `fromJSON(needs.compute-impact.outputs.crates)`.
4. `lint-matrix` — same matrix shape, runs
   `cargo clippy -p $crate --all-targets -- -D warnings`.
5. `fmt` — single runner, `cargo fmt --check`.
6. `version-bump-check` — needs `compute-impact`, fails if
   `workspace.package.version` did not advance and impact set is
   non-empty, or if version advanced and impact set is empty.
7. `changelog-check` — needs `compute-impact`, greps `CHANGELOG.md`
   for an H2 matching the new version.

The actual YAML is not duplicated here; the ops-writer who lands the
file should follow the structure above and lint with `actionlint`
before opening the PR.

### Branch protection (step 5)

No file diff. Manual UI action on the `official` remote.
Reversible — the toggle can be undone in the same UI if the gate
mis-fires after rollout.

### `scripts/tag-release.sh` or `release-plz` config (step 7, optional)

Not specified here. Deferred.

## Acceptance

The plan is complete when a fresh contributor can:

- Read [versioning.md](versioning.md), this file, and
  [regression-testing.md](regression-testing.md) in order, and
  produce a PR that lands step 1 without further questions.
- Repeat for steps 2, 3, 4 — landing each as a separate PR — without
  needing clarification on file paths, job names, or matrix shape.
- Recognize step 5 as a manual UI flip on the `official` remote and
  perform it (or hand it to the repo admin) without ambiguity about
  which checks to mark required.
- Recognize step 7 as out-of-scope and not block on it.

Each step in "Landing order" lists its own PR scope and exit
criterion; a contributor who satisfies those exit criteria has
delivered that step. The plan as a whole is delivered when steps 1
through 5 have landed and a contributor's first source-bearing PR is
correctly blocked by the gate.
