# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-05-12

### Added
- `spout get` / `spout rm` failures now include a `recently removed: <name> (<date>, "<reason>")` line when the requested service has a removal record in this project's history. Closes the loop on the "user just removed it" failure mode: an agent that runs `spout get api` shortly after the user removed `api` sees the date and reason, and can pause before re-allocating rather than silently resurrecting what was just deleted. Most-recent removal only — older entries stay reachable via `spout whois <port> --history`. Cross-project history is invisible (service names are project-scoped). New `Registry::history_for_service` method mirrors the existing `history_for_port`. Exit code 1 unchanged.
- `spout get <service>` and `spout rm <service>` now list the project's actual service names on failure instead of a bare "service not registered". When an agent guesses a wrong name (e.g. `acme-postgres` in a project whose service is registered as just `postgres`), the stderr message surfaces the real name plus a `spout env` pointer — so it self-corrects rather than allocating a duplicate. Exit code 1 unchanged. Empty-project failures suggest `spout alloc <service>`. New `SpoutError::ServiceNotRegisteredInProject` variant carries the context.
- `list` and `free` are now hidden clap aliases for `ls` and `rm`. Common verb guesses (`spout list`, `spout free postgres`) no longer hit "unrecognized subcommand" errors. Help still shows the canonical names.
- `spout alloc` compose-mode now auto-loads `docker-compose.override.yml` (or `.override.yaml` / `compose.override.yml` / `compose.override.yaml`) on top of the base file — same behaviour as `docker compose up`. Projects that split service definitions from port declarations (base declares services, override adds `ports:` for local dev) finally scan correctly. Merge is override-wins per service, which matches docker compose's observable outcome for spout's single `services.<name>.ports` lookup. If only an override is present with no base, spout exits 8 with a friendly message pointing to `-f`.
- `spout alloc -f` is now repeatable: `spout alloc -f a.yml -f b.yml -f c.yml` loads all three in order and folds merge left-to-right — later files win on per-service conflicts. Passing `-f` at all disables the auto-detect sweep (no silent override pickup). The summary header cites every file read, joined with ` + `.
- `spout rm --project [NAME]` removes every service for that project in one shot. Without `NAME` it targets the current project (matches the `--project` flag on `ls` and `env`). Defaults to a `[y/N]` confirmation showing the services about to go; `--yes` skips the prompt; `--dry-run` previews the list without changes. Closes the gap where decommissioning a still-extant project meant looping `spout rm <svc>` by hand. History records each removal with `"user requested (project rm)"` so `spout whois <port> --history` shows the bulk-removal context.
- `spout rm <service> --project <NAME>` removes a single service from a named project (cross-project removal). Useful from outside a project's working directory.
- `spout get <service> --project <NAME>` reads a registered port from a named project rather than the current one.
- `spout alloc` (with no service name) now reads a compose file from the current directory and allocates a port per declared service in a single registry transaction. Auto-detects `docker-compose.yml`, `docker-compose.yaml`, `compose.yml`, `compose.yaml` (same four names used for monorepo detection). `-f, --file <PATH>` overrides. Protocol is inferred from the port spec (`/udp` suffix or long-form `protocol: udp` → UDP; everything else → TCP). Multi-port services allocate every port: the first keeps the bare service name, extras are suffixed with their container port (e.g. mailpit with `["8025:8025", "1025:1025"]` registers `mailpit` and `mailpit-1025`). Idempotent: re-running returns the same ports. Exit code 8 when the file is missing or YAML is malformed (new `ComposeNotFound` / `ComposeInvalid` error variants). Output is a tabular summary: `postgres 20000 tcp` one row per allocated port.
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

[Unreleased]: https://github.com/spout-cli/spout/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/spout-cli/spout/releases/tag/v0.2.0
[0.1.0]: https://github.com/spout-cli/spout/releases/tag/v0.1.0
