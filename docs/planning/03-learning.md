# Stage 3 — Learning

**Stage:** Monorepo support
**Written:** 21 April 2026 (after Stage 3b landed)
**Planning doc:** [03-planning.md](03-planning.md) — includes the Stage 3b addendum at the bottom

---

## What shipped

Both phases of monorepo support:

- **Stage 3a** — `SPOUT_PROJECT` env-var override as the top layer of `project::resolve()`. Set non-empty → wins over everything else. Unset/empty/whitespace-only → falls through.
- **Stage 3b** — auto-detect via compose-marker walk. When spout finds a `docker-compose.yml` / `.yaml` / `compose.yml` / `.yaml` in an ancestor directory (strictly below git root), it appends that directory's path to the base identity. Nearest marker wins. A root-level marker adds nothing (preserves single-project-repo behaviour).

Test count: 94 → 102 (five parser/override tests in 3a, eight marker-walk tests in 3b). `src/project.rs` split into `src/project.rs` (296 lines) + `src/project_markers.rs` (160 lines) when 3b pushed the combined file over the 400-line cap. `cargo fmt --check` + `cargo clippy -D warnings` clean at every commit. End-to-end smoke-tested against the built binary in a temp monorepo layout.

---

## What the plan got wrong

### 1. The user's first-draft marker precedence would have silently defeated the fix

Original proposal put `.git/` at the top of the precedence list, which in a monorepo means every subdir resolves to the same root and the "fix" does nothing. Caught before any code was written — the user had the right instinct (marker-based disambiguation) but the wrong order of operations.

The synthesis that actually works: keep git-remote as the base identity, walk up from CWD within the worktree, and append the *nearest* marker directory's relative path. This preserves cross-clone stability (git remote still anchors) while adding within-repo disambiguation.

**Fix:** Wrote the synthesis into the planning doc as the "rejected alternatives" section before starting Stage 3b, so the reasoning is captured alongside the decision.

### 2. The marker walk initially only triggered when a git remote existed

My first wiring put the marker walk inside the `git_remote_identity()` branch:

```rust
if let Some(remote) = git_remote_identity() {
    if let Some(subdir) = current_marker_subdir() { ... }
    return Ok(remote);
}
if let Some(path) = git_root_path() { return Ok(path); }
```

The smoke test caught it immediately — a fresh `git init` with no remote configured fell through to `git_root_path()` without ever checking for markers. Both subdirs collided on the bare git-root path.

**Fix:** Coalesce the base into `git_remote_identity().or_else(git_root_path)`, then do the marker walk once against whichever base succeeded:

```rust
if let Some(base) = git_remote_identity().or_else(git_root_path) {
    if let Some(subdir) = current_marker_subdir() {
        return Ok(format!("{base}/{subdir}"));
    }
    return Ok(base);
}
```

This preserves the "remote preferred over path" ordering while applying marker composition uniformly. Unit tests didn't catch this because they use `resolve_with_override` directly and the spout repo itself has a remote — the smoke test was essential.

### 3. Line-budget math was wrong

Planning doc projected ~100 new lines for the marker walk, bringing `project.rs` from 284 to ~384 — "under the 400-line cap, but tight". Actual: 441 lines, over the cap. The eight-case test matrix was chunkier than estimated (~120 lines for tests alone).

**Fix:** Split into `project.rs` + `project_markers.rs` as the planning doc flagged was a risk. Cleaner than expected — `compose_marker_subdir` and its helpers are pure functions on `&Path`, so the impure wrapper `current_marker_subdir` is the only thing that stays in `project.rs`. Dependency direction: `project.rs` imports from `project_markers.rs`, not the other way, which matches the existing pattern of pure helpers living alongside the orchestration.

Split paid off twice: `project_markers.rs` tests are also free of the `OnceLock` + `std::env::set_var` interaction problem that plagues `project.rs`, since they operate purely on `tempfile::TempDir` paths.

---

## What went right

- **TDD caught the bugs that mattered.** Writing the eight marker-walk tests first made the core algorithm trivially correct by the time I wrote the implementation. The one real bug (wiring into the wrong branch of `resolve_with_override`) was caught by the smoke test, which is exactly where unit tests can't reach.
- **Pure-function design for the marker walk.** `compose_marker_subdir(git_root, cwd)` takes both paths as arguments, so its tests never touch env vars or shell out to git. They build temp-dir fixtures and call the function directly. Zero flakiness potential.
- **Canonicalising both sides of the comparison.** Flagged in the planning doc's risks section: macOS temp dirs live under `/var/folders/...` but `env::current_dir()` returns `/private/var/...` — different string, same inode. `canonicalize()` on both paths before `strip_prefix` resolved it. Two test runs confirmed this wasn't a theoretical concern (the tests failed without canonicalize).
- **Narrow marker set.** Compose files only. No Cargo.toml / package.json / go.mod. Keeps the false-positive surface zero — in a Cargo workspace, every member has a `Cargo.toml`, and naming every crate its own spout project would be wrong. Same shape as the Stage 1 decision to drop the default-ports table: the maintenance tax of a broader match is never worth it.
- **`resolve_with_override(Option<String>)` split from 3a paid off in 3b.** The testable-core-plus-thin-wrapper pattern landed in 3a for env-var testability. In 3b, the pattern absorbed the marker-walk composition without any restructuring — one more `or_else` in the existing chain. If I'd inlined the env-var read into `resolve()` in 3a, 3b would have needed to re-extract it.
- **Per-logical-change commits held up.** Stage 3 shipped across six commits (plan + 3a feat + 3a docs + simplify-review + 3b feat + 3b docs-and-learning). Each stood on its own; `git log --oneline` reads as a coherent narrative. One re-split (the simplify-review fixes after 3a) was cleaner as a single commit than three micro-commits.

---

## Surprises

- **Clippy caught a `let...else` → `?` rewrite.** Mid-implementation, `let Some(parent) = cursor.parent() else { return None; };` tripped `clippy::question_mark`. The `?` operator version is idiomatic and shorter, but I'd written the verbose form out of habit from code that does more than just propagate. Small reminder that Rust clippy lints continue to teach even after years.
- **The "no remote" case was the one that broke.** I wrote the happy-path test (remote + marker = composed identity) first and it worked. Forgot that a fresh `git init` without `git remote add` is exactly the state most users start in locally before they push anywhere. Smoke test in a real fixture was the thing that caught it — unit tests against `resolve_with_override` use the spout repo's own environment (which has a remote), so the bug hid.
- **Splitting modules was easier than expected.** I resisted the split in the planning doc ("keep in `project.rs` for now") but when the file hit 441 lines there was no decision to make. Once I started extracting, it took ten minutes. The pure-function API shape made it almost mechanical. Next time I see a file approaching the cap, split preemptively rather than squeezing.
- **End-to-end smoke tests catch things unit tests can't.** Three unit-test regressions this stage would have each made it past unit tests and broken real usage:
  1. Marker walk in the wrong branch (Stage 3b)
  2. `std::env::set_var` + `OnceLock` interaction that made Stage 3a untestable via env (required the `resolve_with_override` split)
  3. Canonicalisation of temp-dir paths on macOS

  Each was caught by fixture-based testing, not mocked unit tests. Worth carrying forward as a rule: new identity-resolution logic needs a temp-dir smoke test.

---

## Process observations

- **Planning-doc addendums are cheap to extend.** Stage 3a's planning doc had a "Stage 3b deferred" section from day one. When 3b came, I appended the 3b design to the same file rather than spinning up a 04-planning.md. Kept the full Stage 3 story in one place and made the learning doc easier to write — one doc, both phases.
- **Exploratory-question → AskUserQuestion → synthesis was the right shape for the marker-precedence discussion.** User proposed a fix with a subtle bug; I flagged it with a 2-3 sentence take, proposed a synthesis, user agreed. No code written until both understood the design. The alternative (starting to code and negotiating mid-implementation) would have wasted effort on the wrong precedence.
- **Simplify-review between Stage 3a and 3b was well-timed.** Three small fixes (env-var const consistency, `&str` over `String`, tighter fallthrough tests) shipped before 3b built on top of the same code. Would have been harder to disentangle if 3b had piled more changes on unreviewed 3a code.

---

## For next stage

Stage 3 is the last "core identity" piece. Plausible next directions:

- **Compose inference for allocation.** `spout alloc` with no service argument could parse `docker-compose.yml` and alloc every declared service in one call. Noted in Stage 1's next-stage list as the top ergonomic win remaining. Scope risk: docker-compose YAML has a long tail of edge cases (anchors, `extends`, `include`). Needs a scoping pass on which subset we support.
- **`spout gc`.** Deferred three times now. Detect registrations where the project directory no longer exists on disk; offer to purge. Real value for long-lived registries.
- **CI in GitHub Actions.** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `wc -l src/*.rs` against the 400-line cap. All four bars are enforced locally; putting them in CI makes them durable against future contributors.
- **Encapsulate `Registry` public fields.** Mentioned in Stage 1 and Stage 2 next-stage notes. `commands.rs`, `tui.rs`, and now `project.rs`-adjacent modules all reach into `reg.projects` directly. An accessor-based interface would let storage changes propagate cleanly.
- **Fix the flaky `check_returns_true_for_free_port` test.** Hit the flake on both Stage 3a and 3b. Stage 2 learning doc flagged it, Stage 3a learning doc flagged it again. Either `#[ignore]` with a pinned high port, or a different technique entirely. Not free, but not expensive either — worth slotting into a quiet stage.

---

## Commit trail

```
cc0a59b  docs: document monorepo auto-detect in README + CHANGELOG
6d1e2dc  feat(project): compose-marker auto-detect for monorepo subprojects
7953541  docs(planning): Stage 3b addendum — compose-marker auto-detect
93a2e77  refactor: simplify-review fixes — env-const, &str, tighter tests
9c5bc39  docs: document SPOUT_PROJECT monorepo override
aca4d23  feat(project): SPOUT_PROJECT env-var override for monorepo escape hatch
a69ae98  docs(planning): Stage 3 plan — monorepo support
```

Plus this learning doc as the final Stage 3 commit.
