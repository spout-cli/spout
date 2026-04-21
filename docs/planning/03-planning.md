# Stage 3 — Planning (monorepo support)

**Stage:** Monorepo support for project identity
**Written:** Before coding begins
**Covers:** Two additions to `project::resolve()` — the `SPOUT_PROJECT` env-var override (this commit) and nearest-marker auto-detect within the git worktree (deferred to Stage 3b).

---

## The problem

Today's project identity is layered: git-remote-origin → git-root-path → CWD (see `src/project.rs:30-38`). In a monorepo every subdirectory shares the same git remote, so:

```
~/work/my-monorepo/apps/web   → spout alloc postgres → github.com/acme/my-monorepo::postgres
~/work/my-monorepo/apps/api   → spout alloc postgres → github.com/acme/my-monorepo::postgres   (SAME SLOT)
```

The second call is idempotent against the first, so `apps/api` silently reuses `apps/web`'s port. That's exactly the "two projects fight over 5432" failure spout exists to prevent, happening from inside spout.

The Stage 1 plan deferred this explicitly (`docs/planning/01-planning.md:215` — "the monorepo edge case is future work"). Stage 3 picks it up.

## Why two steps, not one

We rejected two single-step alternatives during design:

- **"Walk up to the nearest marker file and use that directory's basename."** Reopens the Stage 1 basename-collision trap documented in `01-learning.md:32` — two unrelated monorepos with `apps/api` subdirs would still collide.
- **"Walk up to `.git` first and use the repo name."** Defeats the monorepo case entirely — there's only one `.git` at the root, so every subdir resolves to the same identity.

The correct synthesis (covered in detail in Stage 3b) is: keep the git-remote identity as the root, then append the marker subdirectory's path-relative-to-git-root when a marker is found below CWD. But that's bigger — marker precedence, test matrix for nested markers, edge cases for CWD-at-git-root. Stage 3a delivers the escape hatch first so monorepo users aren't blocked while we build the auto-detect.

## Stage 3a — what we're building now

A single env-var override at the top of the identity layering. When `SPOUT_PROJECT` is set and non-empty, it wins; everything else falls through unchanged.

- `SPOUT_PROJECT="acme-monorepo/web"` — wins over git remote
- Unset or empty/whitespace — today's behavior, byte-identical

Monorepo users drop it in a per-subdirectory `.envrc` (direnv), a shell rc alias, or mise/asdf hooks. Non-monorepo users see zero change.

## Module structure

Additions to `src/project.rs` only:

- `const SPOUT_PROJECT_ENV: &str = "SPOUT_PROJECT"` at the top, matching the `SPOUT_ICONS_ENV` pattern landed in `src/services.rs` in Stage 3a.
- `fn env_override() -> Option<String>` — reads the env, trims whitespace, returns `None` for empty/whitespace-only.
- Restructure `resolve()` into `resolve()` (thin wrapper that reads env) + `resolve_with_override(Option<String>)` (the testable core). Tests call the core directly with synthetic override values, sidestepping `std::env::set_var`'s interaction with the `current_project` `OnceLock` cache (once primed, the cache is frozen for the rest of the test process).
- Update the module doc to list the env var as layer 0.

## Testability

Env-var mutation in tests is fragile: `current_project()` uses a `OnceLock` cached-per-process (line 21), and `std::env::set_var` is unsafe across threads post-Rust-1.63. We avoid both by testing `resolve_with_override` directly with `Option<String>` values.

Three new tests:
- `resolve_with_override_honours_explicit_name` — `Some("custom")` → `Ok("custom")`, skips git entirely.
- `resolve_with_override_trims_whitespace` — `Some("  name  ")` → `Ok("name")` (actually the trim happens in `env_override`, so this one tests `env_override` via a small helper, or we inline the trim logic into the override path). Decision: inline the trim so `resolve_with_override` handles the full validation in one place.
- `resolve_with_override_falls_through_on_empty` — `Some("")` and `Some("   ")` → falls through to git logic (test that result is non-empty).
- `resolve_with_override_falls_through_on_none` — `None` → falls through to git logic.

## What "done" looks like

- [ ] `SPOUT_PROJECT=foo spout alloc postgres` registers under `foo` instead of the git remote
- [ ] `SPOUT_PROJECT= spout alloc postgres` (empty) behaves identically to unset
- [ ] `SPOUT_PROJECT='  foo  ' spout alloc postgres` registers under `foo` (trimmed)
- [ ] Unset `SPOUT_PROJECT` → behavior unchanged vs. today (git-remote → git-root → CWD)
- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo test` — all existing tests plus new ones pass
- [ ] `README.md` — env-var documented under "Project name"
- [ ] `CHANGELOG.md` — one bullet under Unreleased / Added

## Stage 3b — deferred

Auto-detect via marker walk within the git worktree, identity composed as `{git-remote}/{marker-subdir-path}`. Scope needs its own planning pass — marker precedence (compose.yaml only? language files? polyglot tie-breaking?), behaviour when CWD is the git root, caching implications. Not doing it here.

## Risks and things to watch

- **Silent-collision-by-forgetfulness.** The env-var path is opt-in, so a monorepo user who doesn't know about it still gets today's broken behavior. Mitigated by documenting it visibly in the README's "Project name" section, and by the planned Stage 3b auto-detect that will remove the need to remember.
- **`.envrc` sharing.** If two developers set different `SPOUT_PROJECT` values for the same subdir, their registries diverge. Acceptable — the registry is per-machine (`~/.spout.json`), so this is already the model.
- **Interaction with the `OnceLock` cache.** The env is read once per process, same as `SPOUT_ICONS` and the git-remote lookup. Users who change `SPOUT_PROJECT` mid-session need a fresh process. Consistent with the rest of the CLI's env-var handling.

---

## Build order

1. Write tests for `resolve_with_override` (TDD).
2. Refactor `resolve()` into thin-wrapper + `resolve_with_override(Option<String>)`.
3. Add `env_override()` with const, trim, empty-to-None.
4. Update module doc.
5. README section under "Project name".
6. CHANGELOG entry under Unreleased / Added.

One logical change → one commit (`feat(project): SPOUT_PROJECT env-var override for monorepo escape hatch`). Docs in a separate commit.
