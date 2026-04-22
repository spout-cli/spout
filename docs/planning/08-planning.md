# Stage 8 — `--project` on `rm` and `get`

## Goal

Close the project-level `rm` gap: today, removing every service for a
decommissioned-but-still-extant project means looping `spout rm <svc>`
by hand, since `spout prune` only fires on path-missing or stale-by-age
projects. Add `--project [NAME]` to `rm` so a single command can wipe
or re-target. Same flag also lands on `get` for consistency with the
existing `--project` flag on `ls` and `env`.

## Decisions locked

- **`rm --project [NAME]`** — wholesale removal of every service in
  that project. With no `NAME`, uses the current project (matches
  `ls`/`env`/`prune` shape).
- **`rm <service> --project <NAME>`** — single-service removal in a
  named project (cross-project rm).
- **Single confirmation, not per-entry.** Prune's `[y/N/q/!]` per-entry
  loop fits stale-audit; "decommission this project" is one decision.
  `[y/N]` on a block listing all the entries about to go. `--yes` skips.
  `--dry-run` lists without prompting.
- **`get --project <NAME>`** — read a registered port from a different
  project. Required `<NAME>` (no implicit current — single-service
  reads from the current project are already `spout get <service>`
  with no flag).
- **Skip `alloc --project` / `set --project`.** Writing registrations
  for a project you're not currently in is weirder than reading or
  removing; defer until a real use case lands.

## Approach

TDD; tests before implementation; fmt/clippy/test green between
commits.

### Commit 1 — `feat(cli,commands): rm --project [NAME] for whole-project removal`

- `src/cli.rs`: `Rm` gains optional service + `--project [NAME]`
  + `--yes` + `--dry-run`. Service is `Option<String>`; project
  flag mirrors `Ls`/`Env`'s `Option<Option<String>>` shape (`None`
  = no flag, `Some(None)` = `--project` bare = current project,
  `Some(Some(name))` = explicit project).
- `src/commands/mod.rs` or a new `src/commands/rm.rs` — depending on
  size. Existing `rm` is 7 lines; the new dispatch matrix + interactive
  confirm pushes it past 30. Likely extract `src/commands/rm.rs`.
- Dispatch matrix:

  | `service` | `--project` | `--yes` | Behaviour |
  |---|---|---|---|
  | Some(s) | None | any | Today's single-service rm in current project. |
  | Some(s) | Some(name) | any | Single-service rm in `name`. |
  | None | Some(_) | any | Whole-project rm; `--yes` skips confirm. |
  | None | None | any | Usage error: "specify a service or --project". |
  | None | Some(_) | `--dry-run` | List candidates, no changes. |

- Single confirmation block:
  ```
  $ spout rm --project myapp
  Remove all 4 services for 'myapp'?
    postgres  20000  tcp
    redis     20001  tcp
    api       20002  tcp
    dns       20003  udp
  [y/N]
  ```

- History reason for whole-project rm: `"user requested (project rm)"`.
  Distinguishes from single-service `"user requested"` so
  `whois --history` shows the bulk-removal context.

- One `with_lock` for the whole batch — Stage 6/7's lesson. New
  `registry::Registry::remove_project(name, reason)` helper that
  iterates the project's services, pushing each to history before
  removing the project entry.

Tests (in `src/commands/rm.rs::tests`):
- `rm_single_service_unchanged` — existing behaviour preserved.
- `rm_single_in_named_project` — cross-project removal.
- `rm_project_with_yes_removes_all_services_in_one_lock`.
- `rm_project_dry_run_lists_without_removing`.
- `rm_project_records_history_with_distinct_reason`.
- `rm_project_unknown_project_errors_one`.
- `rm_no_args_is_usage_error`.

### Commit 2 — `feat(cli,commands): get --project <NAME> reads from a named project`

- `src/cli.rs`: `Get { service: String, #[arg(long, value_name = "NAME")] project: Option<String> }`. Required name when present (unlike `--project [NAME]` on rm/ls/env, which has the bare-flag-means-current shape; for `get`, "current" is already the no-flag default, so the optional-value second form would be redundant).
- `src/commands/mod.rs::get` takes `Option<&str>` project override; falls back to `project::current_project()` when None. ~5 lines changed.

Tests:
- `get_with_explicit_project_reads_from_that_project` — seed two projects, `get --project p1 svc` returns p1's port, not p2's.
- `get_with_unknown_project_errors_service_not_registered` — same exit code as today's missing-service path.

### Commit 3 — `docs: CHANGELOG, README, PRD, llms.txt`

- CHANGELOG `[Unreleased]`: new entry for the `--project` additions on
  `rm` and `get`, plus `--yes` and `--dry-run` on `rm` for whole-project
  mode.
- README: extend the Core commands block; add a short "Decommissioning a
  project" subsection near "Cleaning up stale registrations."
- PRD §6 CLI block: show the new flags. §3.2 mutation-boundary table:
  the `rm` row's annotation becomes "Always" (already was) — no change.
- `llms.txt`: extend the `spout rm` and `spout get` blocks.

## Critical files

- `src/cli.rs` — Rm and Get arg structs grow.
- `src/commands/mod.rs` or `src/commands/rm.rs` (split if needed).
- `src/registry/mod.rs` — `Registry::remove_project` helper.
- `src/main.rs` — dispatch updates for both commands.
- Docs: CHANGELOG, README, llms.txt, docs/spout-prd.md.

## Verification

1. `cargo test` after every commit; tests grow from 189 to ~200.
2. End-to-end:
   - `spout alloc postgres && spout alloc redis && spout rm --project --yes` — empty registry afterward.
   - `spout get postgres --project other-app` returns the other-app port if registered, errors with exit 1 otherwise.
   - `spout rm` (no args) errors with usage message.
3. History integrity: `spout rm --project --yes` then `spout whois <port> --history` shows the rich reason `"user requested (project rm)"` for each.
4. Final gate: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. All `src/**/*.rs` under 400.

## Risks

- `Rm` argument parsing now has a 2D matrix (service ± project).
  clap's parsing is forgiving but the runtime usage error needs to be
  clear when both args are absent.
- The interactive `[y/N]` confirm for whole-project rm uses stdin —
  same pattern as prune. Reuse `src/commands/prune/mod.rs`'s injectable
  `BufRead`/`Write` shape for tests, OR keep the whole-project confirm
  inline since it's a single read_line. Probably inline.
- `Registry::remove_project` adds API surface. Keep it small —
  iterate keys, call existing `remove()` per service. Tests for the
  helper itself live in `src/registry/mod.rs`.

## Out of scope

- `alloc --project` and `set --project` — see Goals.
- Per-entry interactive confirm for whole-project rm (overkill;
  prune already covers the audit-then-remove flow).
- A new exit code — reuse `ServiceNotRegistered` (exit 1) when the
  named project doesn't exist or is empty.
