# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `spout completions <shell>` — emits a completion script for bash, zsh, fish, elvish, or powershell (via `clap_complete`). Annotated `[READ ONLY]` in help.
- Display-only Ratatui TUI for `spout ls`. Activates only when stdout is a TTY and `--no-tui` was not passed; pipes, redirects, and non-TTY contexts fall back to the existing plain-text output unchanged. Press `q`, `Esc`, or `Ctrl-C` to exit.
- `--no-tui` flag on `spout ls` to force plain-text output even in a terminal.
- TUI viewer now ships with a droplet in the title, a cyan port column, dim metadata columns, and a green status-dot on the project separator.
- `SPOUT_ICONS` env var — optional `service=icon,…` map that prefixes service names with a user-chosen glyph in the TUI. Spout ships no built-in mapping. Plain-text output (`--no-tui` and pipes) is unchanged.
- `SPOUT_PROJECT` env var — monorepo escape hatch. When set, overrides the git-remote/git-root/CWD layered project identity. Whitespace is trimmed; empty or unset falls through to the default. Intended for per-subdirectory use via direnv, mise, or shell rc.

[Unreleased]: https://github.com/spout-cli/spout/compare/HEAD...HEAD
