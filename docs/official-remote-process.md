# Pushing to the Official Remote: Process and Considerations

This is the operational runbook for the `official` remote on the
AstraeaDB repo. It is the companion to
[versioning.md](versioning.md),
[regression-testing.md](regression-testing.md), and
[versioning-implementation-plan.md](versioning-implementation-plan.md):
those describe the *policy* and the *file-by-file landing plan*;
this document describes what actually happens when you push,
what to set up on the GitHub side, and what the steady-state day-to-day
looks like.

## Current state (2026-05-20)

The local checkout at `/Users/jimharris/Documents/astraeadb/` and
its `origin` remote (`jimeharrisjr/AstraeaDB`) both contain:

- `docs/versioning.md`
- `docs/regression-testing.md`
- `docs/versioning-implementation-plan.md`
- `docs/official-remote-process.md` (this file)
- `CHANGELOG.md` (baseline `## [0.1.0] - 2026-05-19`)
- `.github/workflows/version-gate.yml`

None of this is on the `official` remote
(`AstraeaDB-Official`) yet. The version-gate workflow is dormant
until that push happens.

The companion impact-analysis tool lives in the sibling repo at
`AstraeaDB/astraea-development` under `projects/version-control/`
and has been pushed (commit `2a19cda` on its `main`). The CI
workflow clones that repo to obtain the tool source.

## What the first push to `official` does

The moment you run `git push official main`:

1. The `.github/workflows/version-gate.yml` file lands on the
   official repo. GitHub registers it as an active workflow on the
   next event.
2. Subsequent pushes to `official:main` and PRs opened against
   `official:main` trigger the workflow. It will:
   - clone `AstraeaDB/astraea-development` (the impact-tool source);
   - build the impact tool;
   - run it against the PR diff;
   - fan out `cargo test -p $crate`, `cargo clippy -p $crate`, and
     `cargo fmt --check` over the impacted matrix;
   - verify `workspace.package.version` was bumped when crates
     changed;
   - verify `CHANGELOG.md` was updated with a matching `## [X.Y.Z]`
     header when crates changed.
3. Because branch protection on `official:main` does **not** yet
   require the workflow (advisory mode), a red X surfaces on the
   PR UI but does **not** block merge.

## Pre-flight checklist — before the first push to `official`

These items must be configured in the `AstraeaDB-Official` repo on
GitHub (**Settings → Secrets and variables → Actions**), or the
very first workflow run will fail in `build-impact-tool`.

### Required: repository variable `DEV_ENV_REPO`

- **Variables** tab → **New repository variable**
- Name: `DEV_ENV_REPO`
- Value: `AstraeaDB/astraea-development`

Without it, the second `actions/checkout@v4` step in
`build-impact-tool` has no `repository:` argument and fails.

### Conditionally required: secret `DEV_ENV_TOKEN`

Needed **only if `AstraeaDB/astraea-development` is private**. If
it is public, skip this step — the default `GITHUB_TOKEN` is
sufficient.

- **Secrets** tab → **New repository secret**
- Name: `DEV_ENV_TOKEN`
- Value: a fine-grained PAT with `contents:read` scoped to
  `AstraeaDB/astraea-development`. Set its expiration to something
  reasonable (90 days, or never if you're comfortable rotating
  manually).

### Optional: repository variable `DEV_ENV_REF`

- Defaults to `main` if unset.
- Use this to pin the impact tool to a specific ref (e.g. a tag
  like `v0.1.0` or a SHA) once you want the gate's behavior to be
  reproducible across reruns.

### Recommended: confirm Actions permissions

- **Settings → Actions → General → Workflow permissions**
- Confirm "Read repository contents and packages permissions" is
  enabled for `GITHUB_TOKEN`.
- Leave "Allow GitHub Actions to create and approve pull requests"
  off — this workflow does not need it.

## The first push

```bash
git -C /Users/jimharris/Documents/astraeadb push official main
```

That single command lands the commit on `official:main` and arms
the gate. Two pieces of feedback to watch for:

1. **The push itself** must succeed. If `official:main` has drifted
   from your local `main`, you may need to fetch and rebase first.
   Do not force-push the shared `official` branch.
2. **The first workflow run** appears under **Actions** on the
   official repo. Because the push event triggers the
   `on: push: branches: [main]` clause, the workflow runs against
   the new commit. The `compute-impact` step computes diffs against
   `${{ github.event.before }}`, which on a non-bootstrap push is
   the prior `main` SHA. Expect the impact set to include every
   crate impacted by your landing commit (which, if it touched
   only `docs/`, `CHANGELOG.md`, and `.github/`, will be empty —
   so the matrix jobs skip).

## What to do immediately after the first push

1. **Open Actions** on `AstraeaDB-Official`. Watch the `version-gate`
   run kick off. Expect `build-impact-tool` to take ~2-3 minutes
   the first time (no cache yet).
2. **Inspect the `compute-impact` step's logs.** Verify it picked
   up the expected diff and produced a sensible `crates=[...]`
   output. An empty array for a docs-only landing is correct.
3. **Open a no-op test PR.** A typo fix in `README.md` is ideal.
   Expected behavior:
   - `compute-impact` outputs `[]`.
   - `test-matrix`, `lint-matrix`, `version-bump-check`,
     `changelog-check` all skip via `count != '0'`.
   - `fmt` runs and passes.
   - The PR is mergeable (advisory mode).
4. **Open a real test PR.** Bump `workspace.package.version` to
   `0.1.1`; add a `## [0.1.1] - <today>` block to `CHANGELOG.md`
   with one bullet; fix a typo in
   `crates/astraea-core/src/lib.rs`. Expected behavior:
   - `compute-impact` outputs the full 15-crate array.
   - `test-matrix` fans out 15-way and all pass.
   - `version-bump-check` and `changelog-check` pass.
5. **Iterate for ~10 PRs.** Confirm the matrix is predictable and
   the gate doesn't false-positive. Implementation-plan step 5
   suggests this dwell-time before flipping to required.

## Steady-state — every PR after the first

For any contributor, the rules going forward are:

1. **Edit code.** If the change touches anything under
   `crates/X/src/` or `crates/X/Cargo.toml` (excluding the
   `[dev-dependencies]` section), the PR will require a bump.
2. **Bump `workspace.package.version`** in the repo-root
   `Cargo.toml`. Pre-1.0 rules: any breaking change bumps MINOR
   (`0.1.0` → `0.2.0`); compatible additions or fixes bump PATCH
   (`0.1.0` → `0.1.1`). See
   [versioning.md](versioning.md#version-bump-rules).
3. **Add a `CHANGELOG.md` entry** under a new
   `## [X.Y.Z] - YYYY-MM-DD` H2, or under the working
   `## [Unreleased]` block.
4. **Open a PR against `official:main`.** The workflow runs. Read
   the matrix; fix failures.
5. **Merge.** Once branch protection is flipped on (next section),
   a failing gate genuinely blocks merge.

## Flipping to required (implementation-plan step 5)

After the dwell period:

1. **Settings → Branches → Branch protection rules → Edit `main`**
   (or **Add rule** if none exists).
2. Enable **Require status checks to pass before merging**.
3. Mark these checks as required (start typing each name; the UI
   autocompletes from recent runs):
   - `test-matrix` (the matrix-leaf check)
   - `lint-matrix`
   - `fmt`
   - `version-bump-check`
   - `changelog-check`

   Do **not** require `build-impact-tool` or `compute-impact`
   directly — they are upstream jobs whose success is implied by
   the matrix jobs succeeding, and listing them adds spurious
   "missing check" failures when the matrix is empty.
4. Save. The gate is now required.

This is the only non-PR action in the entire rollout.

## Cutting a release

A release is a single commit on `official:main` that:

- bumps `workspace.package.version` to a stable number (e.g.
  `0.2.0`),
- renames the working `## [Unreleased]` block in `CHANGELOG.md` to
  `## [0.2.0] - <today>`, and
- inserts a fresh empty `## [Unreleased]` block above it.

Then tag and push:

```bash
git -C /Users/jimharris/Documents/astraeadb checkout main
git -C /Users/jimharris/Documents/astraeadb pull official main
git -C /Users/jimharris/Documents/astraeadb tag -a v0.2.0 -m "Release 0.2.0"
git -C /Users/jimharris/Documents/astraeadb push official v0.2.0
```

The tag on `official` is the release source of truth. Mirror the
tag to `public` and `origin` at your discretion. See
[versioning.md](versioning.md#release-process) for the full
release procedure.

## Considerations and gotchas

### The absolute-path hack
`projects/version-control/Cargo.toml` in the dev-env repo
hard-codes `/Users/jimharris/Documents/astraeadb` as the path to
the AstraeaDB workspace crates. The CI workflow rewrites this to
`../../../astraeadb` with a `sed` step before building. If anyone
refactors the dev-env layout, the sed pattern breaks silently and
`build-impact-tool` fails with a cryptic cargo error.

The clean fix is to vendor the impact tool into
`astraeadb/tools/version-impact/` and switch to workspace
dependencies. This is tracked as step 7 of the implementation
plan. Until that lands, the sed hack is the working solution.

### The dev-env repo must be reachable
`build-impact-tool` clones `AstraeaDB/astraea-development`. If that
repo moves, is renamed, or becomes inaccessible, CI breaks
immediately and silently (the checkout step fails with a 404).
Mitigations: pin `DEV_ENV_REF` to a known-good SHA; mirror the
dev-env to a stable internal location; eventually vendor.

### Comment-only edits force a version bump
The gate is path-based and cannot distinguish a `///` rustdoc fix
from a real code change. A typo fix in
`crates/astraea-core/src/lib.rs` requires a PATCH bump and a
`CHANGELOG.md` entry. The doc
([versioning.md Example 2](versioning.md#example-2--i-fixed-a-typo-in-a-doc-comment))
acknowledges this and suggests batching comment fixes into the
next source-bearing PR.

### Dev-deps are part of the impact closure
`astraea-rag` lists `astraea-graph` only under
`[dev-dependencies]`, but a change to `astraea-graph` still
triggers `cargo test -p astraea-rag`. This is intentional — a
test breakage in a downstream crate is exactly what the gate
should catch — but it does mean the impact set is wider than the
public-API graph would suggest. Don't be surprised.

### No-op bumps are not caught
If a PR bumps `workspace.package.version` without touching any
crate source file, `version-bump-check` skips entirely (the
impact set is empty, so the job's `if:` short-circuits). The
gate cannot tell that the bump is gratuitous. Catching this is
step 7 future work. In practice, code review catches it.

### The first push triggers the gate against itself
The push of `version-gate.yml` to `official:main` itself fires the
`on: push: branches: [main]` clause, and the workflow runs against
the diff between the previous `main` and the new commit. That
diff includes `.github/workflows/version-gate.yml` (a non-crate
path) and the docs and `CHANGELOG.md`. The impact set will be
empty, the matrix jobs skip, and the run should be green. If
`build-impact-tool` errors on missing `DEV_ENV_REPO`, that's the
pre-flight checklist item you missed.

### If the workflow itself is broken
- **In advisory mode**: merge despite the red X; fix the workflow
  in the next PR.
- **In required mode**: either push a fix to the workflow file
  directly to `main` (requires admin bypass of branch protection),
  or temporarily unrequire the failing checks in branch protection
  settings, ship the fix as a PR, then re-require.

### `public` is independent
The `public` remote (`jimeharrisjr/graph-astraeadb`) does not run
the gate. Pushing to `public` mirrors content without enforcement.
If you ever want the gate on public PRs too, copy
`.github/workflows/version-gate.yml` there and configure the same
repo variables. For now, treat `public` as a read-only mirror.

### Rolling back the gate entirely
If the gate is causing more pain than value:

1. **Unrequire the checks** in branch protection (instant; reversible).
2. **Delete the workflow file** in a PR
   (`rm .github/workflows/version-gate.yml`). The gate stops
   firing the moment that PR merges.
3. **Keep the docs and CHANGELOG.** They are independently
   valuable and impose no enforcement.

The reverse — turning the gate back on — is just landing the
workflow file again and re-marking the checks as required.

## One-page summary

1. Set `DEV_ENV_REPO=AstraeaDB/astraea-development` as a repo
   variable on `AstraeaDB-Official`.
2. (If the dev-env repo is private) set `DEV_ENV_TOKEN` as a
   repo secret.
3. `git -C /Users/jimharris/Documents/astraeadb push official main`.
4. Open a no-op test PR; confirm the workflow runs green and skips
   the matrix.
5. Open a real test PR (bump + CHANGELOG + crate edit); confirm
   the workflow fans out and passes.
6. After ~10 clean PRs, flip branch protection to require the gate.
7. From then on, every crate-touching PR needs a version bump and
   a CHANGELOG entry; the gate enforces both.
