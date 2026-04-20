# Stage 2 — Planning

**Stage:** Shell completions + display-only TUI for `spout ls`
**Written:** Before coding begins
**Covers:** Two ergonomic features flagged as v1 deliverables but deferred from Stage 1 to keep MVP scope tight.

---

## What we are building

Two independent additions on top of the Stage 1 core loop. Neither changes the semantics of any existing command. Both are gated so agents and scripts see no behaviour change.

### Feature A: Shell completions

A new `spout completions <shell>` subcommand that emits a completion script to stdout for the requested shell (bash, zsh, fish, elvish, powershell). Users pipe it into the conventional install path for their shell. Installation is a one-liner per shell in the README.

`clap_complete = "4"` is already declared in `Cargo.toml` — this is pure wiring.

### Feature B: Display-only TUI for `spout ls`

When `stdout` is a TTY and `--no-tui` is not passed, `spout ls [--project]` renders a styled Ratatui table of registered services (columns: `SERVICE` / `PORT` / `ALLOCATED` / `ENV VAR`) instead of plain text. Press `q` or `Esc` to exit cleanly.

When `stdout` is piped or `--no-tui` is passed, the existing plain-text path runs unchanged. Agents, Makefiles, and scripts see no behaviour change.

---

## Dependency decisions

Only one new crate: `ratatui` (default features — pulls the `crossterm` backend automatically). `clap_complete` is already in.

No `arboard` (no clipboard actions in this stage — deliberate scope cap).

---

## Build order

### Step 1: Shell completions (`feat(cli): shell completions`)

Trivial wiring, no tests needed — `clap_complete` is battle-tested. No logic in `src/cli.rs` beyond adding the subcommand.

- Add `Completions { shell: clap_complete::Shell }` variant to `Commands` in `src/cli.rs`.
- Dispatch in `src/main.rs`: `clap_complete::generate(shell, &mut Cli::command(), "spout", &mut std::io::stdout())`.
- Document the per-shell install one-liners in `README.md`.

### Step 2: TUI for `spout ls` (`feat(tui): display-only ls viewer`)

The heavier piece. Must stay under 400 lines per CODING_GUIDELINES, and all Ratatui imports confined to `src/tui.rs`.

- Add `ratatui` to `Cargo.toml`.
- Add `#[arg(long)] pub no_tui: bool` to the `Ls` variant in `src/cli.rs`.
- In `src/commands.rs::ls()`, branch on `std::io::stdout().is_terminal() && !no_tui`:
  - TTY path → call into `tui::render(&registry, scope)`, return nothing.
  - Non-TTY / `--no-tui` path → existing `format_all` / `format_project_block`, unchanged.
- New `src/tui.rs`:
  - Setup/teardown guard (a struct whose `Drop` impl runs `disable_raw_mode` + leaves the alternate screen). Crucial for panic safety.
  - Render loop: Draw a styled `Table` widget. Service column uses `services::env_var_name(service)` — the first production caller of that helper, so the `#![cfg_attr(not(test), allow(dead_code))]` on `services.rs` comes off in this commit.
  - Event loop: poll with a timeout, handle `q` / `Esc` / `Ctrl-C` to exit, handle resize events (ratatui does most of this if widgets are sized via `Layout`).
- `src/main.rs`: `mod tui;`, thread the `no_tui` flag through dispatch.

---

## Module structure

Additions only:

```
src/
  tui.rs           # NEW. All Ratatui code. <400 lines.
```

No changes to the existing module graph. `tui.rs` depends on `registry`, `project`, `services`, `error`. Nothing else depends on `tui.rs` except `commands.rs` (via a direct call from the `ls` handler).

---

## What "done" looks like

- [ ] `spout completions bash | head` emits a bash completion script
- [ ] `source <(spout completions bash)` enables tab-completion in a subshell
- [ ] `spout ls --project` in a real terminal launches the TUI; `q` exits cleanly
- [ ] Terminal state is fully restored on exit (cursor visible, no dangling raw mode, main screen active)
- [ ] Terminal state is restored even if the TUI panics (verify by temporarily inserting a `panic!()` inside the render loop)
- [ ] `spout ls --project | cat` emits plain text (pipe → `is_terminal()` false → fallback path)
- [ ] `spout ls --project --no-tui` emits plain text even in a TTY
- [ ] `spout get <service>` is unchanged (still prints the port to stdout, no TUI)
- [ ] `spout completions <shell>` never touches the TUI
- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo test` — all existing 72 tests still pass, plus any new
- [ ] `wc -l src/tui.rs` under 400
- [ ] `docs/planning/02-learning.md` written on completion

---

## LLM-friendly guarantees

The TUI only ever activates when all three hold:
1. Command is `spout ls` (not any other command)
2. `stdout` is a TTY (via `std::io::IsTerminal`)
3. `--no-tui` was not passed

Any agent that pipes output, redirects to a file, runs in a non-TTY context (CI, subshell, Makefile `$(shell ...)`), or opts out explicitly gets the existing plain-text path. `spout get` never touches the TUI — it's a pipe-capture command by design.

---

## Out of scope (deferred)

- Clipboard-copy actions (needs `arboard` — sugar on top of sugar)
- Keyboard navigation (`j`/`k`/arrows)
- History drilldown inside the TUI
- Search / filter inside the TUI
- TUI on any command other than `ls`
- `clap_mangen` man pages — separate feature if wanted later
- `spout gc` — still out per Stage 1.1

---

## Risks and things to watch

- **Terminal restoration on panic.** The setup/teardown guard must use `Drop`, not scope exit, so a panic in the render loop still restores raw mode + leaves the alternate screen. Ratatui's own examples show the pattern.
- **`tui.rs` size budget.** Display-only fits comfortably under 400 lines. If the module ever grows (navigation, clipboard, drilldown), split into `tui/mod.rs` + submodules rather than blow the cap.
- **Completions installation varies by shell and OS.** Document bash/zsh/fish in the README; Elvish and PowerShell users work it out from `clap_complete` conventions.
