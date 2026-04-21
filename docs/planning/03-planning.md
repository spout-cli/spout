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

---

# Stage 3b — Nearest-marker auto-detect (addendum)

Added after 3a shipped. 3b removes the "user has to remember to set `SPOUT_PROJECT`" footgun by auto-detecting subproject boundaries inside a git worktree.

## The rule

When git-remote identity is available, walk up from CWD toward the git root. If an ancestor directory (strictly below git root) contains a compose marker, append its path-relative-to-git-root to the remote identity.

- `~/work/my-monorepo/apps/web/` has `docker-compose.yml`, CWD inside it → `github.com/acme/my-monorepo/apps/web`
- `~/work/my-monorepo/apps/api/` has `docker-compose.yml`, CWD inside it → `github.com/acme/my-monorepo/apps/api`
- `~/work/my-monorepo/` has `docker-compose.yml` at the root (no ancestor match below root) → `github.com/acme/my-monorepo` (today's behavior, unchanged)
- `~/work/solo-repo/` with no compose marker anywhere → `github.com/acme/solo-repo` (today's behavior, unchanged)

`SPOUT_PROJECT` wins over the marker walk (Stage 3a's contract holds — it's the user's escape hatch when the walk gets it wrong).

## Marker set — narrow on purpose

Only the four forms Docker Compose itself recognises:

- `docker-compose.yml`
- `docker-compose.yaml`
- `compose.yml`
- `compose.yaml`

Language markers (`package.json`, `Cargo.toml`, `go.mod`, `pyproject.toml`) are tempting but introduce real false positives — nested Cargo workspace members aren't independent projects, nested `package.json` files in a pnpm workspace aren't either. Compose files uniquely say "this directory deploys a set of services independently", which is exactly spout's mental model. Narrow is the right default; if demand for language-marker support appears, it's a future stage.

## Why nearest wins (not root-first)

The rejected precedence in the user's original proposal put `.git` first, which would have defeated the fix — a monorepo has one `.git` at the root, so every subdir would resolve to it. The fix inverts: walk up from CWD, take the first compose marker we find, never past git root. So an `apps/web/docker-compose.yml` wins over a root-level `docker-compose.yml` even though both exist.

## Composition boundaries

- If marker-containing dir equals git root: return no subdir (the marker adds no information beyond what git remote already provides). Keeps today's behavior for single-project repos with a root compose file.
- If marker-containing dir is a strict ancestor of CWD (or CWD itself) below root: return its path-relative-to-root in POSIX form (forward slashes).
- If no marker anywhere between CWD and git root: fall through unchanged.

## Module structure & line budget

`src/project.rs` is 284 lines at HEAD. The marker walk (helper + tests) is ~100 new lines. 284 + 100 ≈ 384 — under the 400-line cap, but tight. Decision: keep in `project.rs` for now, and if a Stage 3c ever needs to grow it further, split into `project/mod.rs` + `project/markers.rs`.

## Testability

`compose_marker_subdir(git_root: &Path, cwd: &Path) -> Option<String>` is a pure function on two paths. Tests build temp directory layouts with `tempfile::TempDir` and call it directly — no env manipulation, no `OnceLock` interaction.

Test matrix (target ~8 tests):

1. No marker anywhere between CWD and root → `None`
2. Marker at git root only, CWD = root → `None` (root marker doesn't add info)
3. Marker at git root only, CWD = subdir without marker → `None`
4. Marker at `<root>/apps/web`, CWD = `<root>/apps/web` → `Some("apps/web")`
5. Marker at `<root>/apps/web`, CWD = `<root>/apps/web/cmd/server` → `Some("apps/web")` (walks up)
6. Marker at both `<root>/docker-compose.yml` and `<root>/apps/web/docker-compose.yml`, CWD = `<root>/apps/web/cmd` → `Some("apps/web")` (nearest wins)
7. Each of the four compose filename variants is detected (parameterised / four small tests)
8. Directory (not file) matching a marker name at `<root>/apps/web/docker-compose.yml/` → does not match (we want files)

Integration test: a new `resolve_with_override_appends_marker_subdir` that synthesises a fake git root + marker path via the pure helper and confirms composition into the final identity string.

## What "done" looks like

- [ ] `~/work/repo/apps/web` with `docker-compose.yml` → identity ends in `/apps/web`
- [ ] `~/work/repo/apps/api` with `docker-compose.yml` → identity ends in `/apps/api` (distinct from web)
- [ ] `~/work/repo/` with root `docker-compose.yml` → identity unchanged from today
- [ ] `~/work/repo/` with no compose file anywhere → identity unchanged from today
- [ ] `SPOUT_PROJECT=explicit` set → marker walk is skipped entirely
- [ ] `cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo test` — all existing tests plus new ones pass
- [ ] `wc -l src/project.rs` under 400
- [ ] README "Monorepos" subsection updated to mention auto-detect
- [ ] CHANGELOG entry under Unreleased / Added
- [ ] `docs/planning/03-learning.md` written (covers both 3a and 3b)

## Out of scope

- Language markers (`package.json`, `Cargo.toml`, etc.)
- Opt-out env var (`SPOUT_PROJECT` is the opt-out — setting it bypasses the walk entirely)
- Walking beyond the git root (the git identity already disambiguates cross-repo; we don't need to)
- Non-git contexts (no git → no remote identity to append to → no-op)

## Risks and things to watch

- **Canonicalisation on macOS `/tmp` symlinks.** Tests using `TempDir` on macOS run under `/var/folders/...` but `env::current_dir()` may return `/private/var/...`. The pure `compose_marker_subdir` canonicalises both inputs before `strip_prefix`, which is required for reliability.
- **Windows path separators.** Spout doesn't support Windows (WSL is the recommendation). `path_to_posix` uses `/` explicitly. Not an issue for v1.
- **Cache interaction.** Identity is still computed once per process via `OnceLock`. The marker walk is ~4 `metadata` calls max per level — microseconds. Absorbed by the cache.
- **Behavioural change for existing users of root-compose repos.** No change — root-only marker returns `None` so the identity is unchanged. Verified by test case 2 + 3.

## Build order (3b)

1. Write tests for `compose_marker_subdir` (TDD).
2. Implement `compose_marker_subdir` + `has_compose_marker` + `COMPOSE_MARKERS` constant.
3. Add orchestration in `resolve_with_override` — between the override check and the plain git-remote fallthrough.
4. Update module doc to list marker walk as layer 0.5.
5. README update — swap the manual-SPOUT_PROJECT-for-monorepos paragraph for "we auto-detect; here's the escape hatch if needed".
6. CHANGELOG entry.
7. Write `03-learning.md` covering both 3a and 3b.

