# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- UDP support via `--udp` flag on `spout alloc`, `spout set`, and `spout check`. TCP remains the default, so every existing invocation is byte-identical. TCP and UDP registrations at the same port number coexist — a TCP claim on 5432 does not block UDP 5432, matching kernel semantics.
- `spout whois <port>` now surfaces every registration for that port across both protocols, sorted TCP first. One line per match: `5432/tcp: project/service (active, allocated DATE)`.
- `spout ls` gains a `PROTO` column in the TUI; plain-text output suffixes the port as `port/proto` on every row. Service rows sort by protocol then service name, so TCP groups above UDP at the same port.
- Registry schema bumped to v2 to record protocol per entry. v1 files read transparently — the missing field defaults to `tcp` — and the next mutating command persists v2.
- `spout prune` — surface and optionally remove stale registrations. Candidates are entries whose `allocated` is older than `--older-than <DAYS>` (default 90), or whose project identity is an absolute filesystem path that no longer exists. Three modes: `--dry-run` surfaces candidates without changes; `--yes` bulk-removes without prompting; the default prompts `[y/N/q/!]` per entry via stdin (`y` yes, `N` keep, `q` quit, `!` yes-to-all). Pruned entries land in `history` with reasons like `pruned: stale (older than 90d)` or `pruned: project path missing`, so `spout whois <port> --history` stays informative.

### Changed
- `SpoutError::PortAlreadyClaimed` and `SpoutError::PortInUse` carry the protocol and render it in the user-facing message (e.g. `port 5432/udp is already in use by the operating system`).

## [0.1.0] - 2026-04-22

### Added
- `spout completions <shell>` — emits a completion script for bash, zsh, fish, elvish, or powershell (via `clap_complete`). Annotated `[READ ONLY]` in help.
- Display-only Ratatui TUI for `spout ls`. Activates only when stdout is a TTY and `--no-tui` was not passed; pipes, redirects, and non-TTY contexts fall back to the existing plain-text output unchanged. Press `q`, `Esc`, or `Ctrl-C` to exit.
- `--no-tui` flag on `spout ls` to force plain-text output even in a terminal.
- TUI viewer ships with a droplet in the title, a cyan port column, and dim metadata columns.
- `SPOUT_ICONS` env var — optional `service=icon,…` map that prefixes service names with a user-chosen glyph in the TUI. Spout ships no built-in mapping. Plain-text output (`--no-tui` and pipes) is unchanged.
- `SPOUT_PROJECT` env var — monorepo escape hatch. When set, overrides the git-remote/git-root/CWD layered project identity. Whitespace is trimmed; empty or unset falls through to the default. Intended for per-subdirectory use via direnv, mise, or shell rc.
- Monorepo auto-detect: `spout` now walks up from CWD toward the git root, and if it finds a `docker-compose.yml` / `docker-compose.yaml` / `compose.yml` / `compose.yaml` in an ancestor directory, appends that directory's path-relative-to-git-root to the project identity. Nearest marker wins. Repos without a compose file — or with one only at the git root — behave identically to before.
- `spout env [--project <NAME>]` — prints one `KEY=VALUE` line per registered service for the current (or named) project, suitable for `eval $(spout env)`. Read-only; shares `--project` semantics with `spout ls`.
- Live bound/free status indicator on `spout ls`: every registered port is probed at render time, and the row shows whether it is currently bound on the OS. Works in both plain-text and TUI output.
- `spout ls` TUI now groups rows by project with titled section headers. In `--project`-filtered views the redundant section title is omitted.
- GitHub Actions CI — runs `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, and enforces the 400-line-per-file cap on every push.
- `templates/CLAUDE.md` — a primer that downstream projects can drop into their own repo to teach coding agents how to call spout correctly.

### Changed
- TUI `ENV VAR` column replaced with `PROJECT`. Project identity now shows on every row, both in the all-projects view and the `--project`-filtered view. The per-project separator row is gone — redundant when project is a per-row column.

### Removed
- `services::env_var_name` helper and its tests. Was only consumed by the (now-replaced) TUI ENV VAR column; callers who need the env-var name can derive it trivially (uppercase, hyphens → underscores, append `_PORT`).

[Unreleased]: https://github.com/spout-cli/spout/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/spout-cli/spout/releases/tag/v0.1.0
