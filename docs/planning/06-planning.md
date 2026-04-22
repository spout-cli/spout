# Stage 6 — `spout prune`

## Goal

Ship `spout prune` — the first cleanup command for long-lived
registries. Detects stale entries by age (`allocated > --older-than
days`) and by path-existence (absolute-path identities whose directory
no longer exists). Offers three modes: `--dry-run` (surface only),
interactive (`[y/N/q/!]` per entry), and `--yes` (bulk).

Design in full: `docs/proposals/prune-command.md`. This doc is the
execution plan. For the "why" of each decision, read the proposal.

## Decisions (all resolved)

- **Stdin over Ratatui** for the interactive mode. Smaller, works
  over SSH, testable with buffer injection. TUI is a follow-up if
  the UX feels cramped.
- **Split `src/registry.rs` first.** Stage 5 learning doc flagged
  it at 398/400 lines. Prune adds iteration helpers; split makes
  the growth safe.
- **Default cutoff: 90 days**, overridable via `--older-than`.
- **Path heuristic:** `identity.starts_with('/')` → filesystem
  path. Windows is WSL-only.
- **Git-remote resolution deferred** as `--check-remotes`. No
  network I/O in this stage.

## Commit sequence

TDD; tests before implementation; fmt/clippy/test green between
commits.

1. **`refactor(registry):`** split into `src/registry/mod.rs` +
   `src/registry/io.rs`. Zero behaviour change. `pub use io::*`
   preserves every caller's imports.
2. **`feat(date):`** `parse_iso_date`, `days_between`, `days_ago`
   helpers. No external date crate added.
3. **`feat(cli,commands):`** `spout prune --dry-run` scanner —
   collects candidates by age + path-existence, formats the report.
4. **`feat(commands):`** interactive stdin confirmation. Injectable
   `BufRead`/`Write` for testability. Likely extracts to
   `src/commands/prune.rs` when `commands.rs` nears the cap.
5. **`feat(commands):`** `--yes` bulk mode with rich reason strings
   so `whois --history` stays informative.
6. **`docs:`** CHANGELOG, README, llms.txt, drop from PRD §18.

## Critical files

- `src/registry.rs` → `src/registry/mod.rs` + `src/registry/io.rs`
- `src/date.rs` — extend
- `src/cli.rs` — new Prune subcommand
- `src/commands.rs` — dispatch + initial scanner
- `src/commands/prune.rs` — likely extraction target during Commit 4
- `src/main.rs` — one dispatch arm
- Docs: CHANGELOG, README, llms.txt, docs/spout-prd.md §18

## Verification

1. `cargo test` green after every commit; 138 → ~155.
2. Post-Commit 1: no caller edits needed; tests still 138/138.
3. Post-Commit 3: seeded registry (fresh + 100d + 200d +
   absolute-path pointing at deleted tempdir) — dry-run surfaces
   the three stale, `--older-than 365` drops to two.
4. Post-Commit 4: `echo -e "y\nn\nq" | spout prune` removes the
   first, keeps the second, quits on the third.
5. Post-Commit 5: `spout prune --yes` empties candidates;
   `spout whois <port> --history` shows the rich reason strings.
6. Final gates: fmt/clippy/tests green; `wc -l src/**/*.rs` under
   400 everywhere.

## Risks

- `src/commands.rs` at 386 today; scanner + interactive loop likely
  push it past 400 around Commit 4. Plan: extract
  `src/commands/prune.rs` the moment it tips — same pattern as
  `project_markers.rs` carved from `project.rs` in Stage 3b.
- Date-math by hand can miss edge cases (leap years, month
  boundaries). Testing uses known offsets against a fixed reference
  date to avoid flakiness.
- TOCTOU on path-existence: a project directory could appear or
  disappear between `spout prune --dry-run` and the subsequent
  `spout prune`. Same risk as the registry allocator's bind race.
  Mitigation: the user re-runs if surprised; the proposal accepts
  the window.

## Out of scope

- TUI confirmation UI (stdin first; revisit later).
- `--check-remotes` — network-probing identity validity.
- Pruning the `history` array itself (proposal: history stays —
  it's what `whois --history` reads).
- Scheduled/cron pruning.

## Deferred to the learning doc

Plan-vs-reality notes, any commits that split differently than
planned, notable surprises.
