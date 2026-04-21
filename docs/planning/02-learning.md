# Stage 2 — Learning

**Stage:** Shell completions + display-only TUI for `spout ls`
**Written:** 21 April 2026 (after `a95939b`)
**Planning doc:** [02-planning.md](02-planning.md)

---

## What shipped

Two additions on top of the Stage 1 core, both strictly additive — no existing command's semantics changed:

1. **`spout completions <shell>`** — emits a completion script for bash, zsh, fish, elvish, or powershell. Pure `clap_complete` wiring, no logic.
2. **Display-only TUI for `spout ls`** — a styled Ratatui table (columns: `SERVICE` / `PORT` / `ALLOCATED` / `ENV VAR`). Activates only when all three hold: command is `ls`, `stdout` is a TTY, and `--no-tui` was not passed. Pipe, redirect, CI, or explicit opt-out all fall back to the existing plain-text path unchanged.

Test count: 72 → 80. One new module (`src/tui.rs`, 226 lines), one new dep (`ratatui`). `cargo fmt --check` clean, `cargo clippy -D warnings` clean. `src/tui.rs` under the 400-line cap with headroom.

---

## What went right

- **The planning doc aged well.** Every assumption in `02-planning.md` held up during implementation — the TUI gating conditions, the module boundary, the 400-line budget, the RAII guard pattern. The product-design pass at the top of Stage 1's "next stage" notes paid off here: Stage 2's plan was scoped tight enough that there was nothing to discover mid-build.
- **RAII guard for terminal restoration.** `TerminalGuard`'s `Drop` impl runs `disable_raw_mode` and `LeaveAlternateScreen`, so a panic in the render loop still restores terminal state on unwind. This was the single biggest correctness risk for the TUI and it was cheap to get right because the pattern was already well-trodden in Ratatui's examples. The planning-doc checklist item for actually triggering a panic and confirming restoration is still open — worth doing before cutting a release.
- **The TTY-gate is two cheap conditions at one call site.** `stdout().is_terminal() && !no_tui` in `commands::ls` is the entire decision. The third "condition" — that the TUI only exists for `ls` — is structural (no other command calls into `tui::render`), not a runtime check. Agents get a zero-ambiguity rule: if you pipe or pass `--no-tui`, you get plain text. No heuristic, no magic.
- **`services.rs`'s dead-code suppression came off.** The `#![cfg_attr(not(test), allow(dead_code))]` on `services::env_var_name` had been flagged in Stage 1 as scaffolding; `tui.rs` is its first production caller, so the suppression went with the TUI commit. Nice loose end to tidy.
- **Ls now returns `Option<String>`.** `ls()` returns `Ok(None)` when the TUI has already rendered and exited, `Ok(Some(text))` for the plain-text path. `main.rs` just pattern-matches on the `Option` — no TUI concerns leak out of `tui.rs`, and the CLI dispatch stays dumb.

---

## What needed a second pass

- **Completions subcommand was initially un-annotated.** The first completions commit (`3d3afb4`) landed without the `[READ ONLY]` tag in its help text. Noticed on review that every other read-only command was annotated — fixed in a follow-up (`914a1fe`). Worth remembering: the read-only/mutates convention is load-bearing for agent reasoning, so every new subcommand needs the annotation, not just the business-logic ones.
- **Flaky test: `check_returns_true_for_free_port`.** Inherited from Stage 1 but hit on a re-run in this stage. The pattern is "bind an ephemeral port, drop the listener, assert the port is free" — inherently racy because the OS can hand the port to another process in the window between `drop` and the check. Left alone for now (passes in isolation, rare in practice); the honest fix is probably to mark it `#[ignore]` as a smoke test and assert `check()` on a definitely-unused high port instead.

---

## Surprises

- **Ratatui needed less ceremony than expected.** The whole TUI path — setup guard, draw loop, event poll, row collection, exit keys — fit in 226 lines with room for unit tests of the pure functions (`collect_rows`, `is_exit_key`). The widgets API is pleasantly declarative; the main discipline is keeping Ratatui types out of the rest of the crate, which the single-module boundary enforces.
- **Unit-testing TUI logic is tractable if you isolate the pure parts.** `collect_rows` and `is_exit_key` are both testable without a terminal, and they're where the interesting logic lives. The render loop itself is thin glue — the kind of code you smoke-test with `cargo run`, not with unit tests. Four of the six new TUI tests cover `collect_rows` edge cases (empty, project filter, multi-project separator, single-project no-separator); the other two cover `is_exit_key`.
- **The "display-only" cap held firm.** Temptation to sneak in `j`/`k` navigation or a clipboard shortcut was real — they're each <20 lines. Held the line because the planning doc put them out of scope, and the value of a scope-respecting stage is the credibility it builds for the next stage's scope. Deferred items stay deferred.

---

## Process observations

- **Small stages are fast and boring.** Stage 1 was a 14-commit product-design crucible; Stage 2 was 3 implementation commits plus a doc. The cost of a separate stage with its own planning+learning docs is real, but the benefit — clean commit history, scoped risk, a plan that actually matches the code — compounds. Worth doing even when the feature feels small enough to bundle.
- **Read-only/mutates annotation drift is a real risk.** The missed tag on `completions` took a follow-up commit to fix. Adding a test that scrapes `--help` output for the annotation on every read-only subcommand would catch this at CI time rather than review time. Low-value for a 7-command CLI today, worth considering if the command surface grows.
- **Conventional commits landed cleanly.** Every commit in Stage 2 parses as conventional (`feat(cli):`, `feat(tui):`, `refactor(cli):`, `docs(planning):`). One logical change per commit held without any squashing needed.

---

## For next stage (Stage 3)

Options, not commitments — user picks.

- **Compose inference** (`spout alloc` with no args parses `docker-compose.yml` and allocates for every declared service). Flagged in Stage 1's next-stage notes as the top ergonomic win still on the table. Scope risk: docker-compose YAML has a long tail of edge cases (`extends`, `include`, anchors) — needs a product-design pass before coding.
- **`spout gc`** — surface stale registrations (e.g. project directories that no longer exist on disk). Deferred twice now. Worth tackling once the command surface isn't changing as fast.
- **Encapsulate `Registry` public fields.** `commands.rs` and `tui.rs` both couple to `reg.projects` directly. Accessor methods would let the storage shape change (e.g. sorted iteration by default) without touching every caller. Small refactor, one commit.
- **Remove `reason: &str` on `Registry::remove`** in favour of an enum (`RemovalReason::UserRequested` etc.). Carried over from Stage 1 notes — still valid, still small.
- **CI for the existing bars.** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and a `wc -l` guard for the 400-line cap. All run locally today; putting them in a GitHub Action would make the rules enforceable against future contributors, not just against me.

---

## Commit trail

```
a95939b feat(tui): display-only Ratatui viewer for spout ls
914a1fe refactor(cli): annotate completions subcommand as [READ ONLY]
3d3afb4 feat(cli): shell completions via clap_complete
110a23d docs(planning): Stage 2 plan — completions + ls TUI
```

Plus this learning doc and the accompanying CHANGELOG / README touch-up.
