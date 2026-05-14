# Contributing to spout

Thanks for your interest in spout. This project values simplicity and maintainability over cleverness or feature count.

## Before opening a PR

1. Read [`CLAUDE.md`](CLAUDE.md). All rules are enforced, not suggestions.
2. Check the "What's in scope" section below to make sure your idea fits.
3. Check existing issues for the thing you want to work on.

## Rules of engagement

- **TDD.** Tests before implementation. PRs without tests are closed.
- **File size limits.** No file over 400 lines. No function over 40 lines. No function with more than 4 arguments.
- **No `unwrap()` or `expect()` in production code.** Propagate errors explicitly.
- **Formatting and linting.** `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` must pass.
- **Conventional commits.** One logical change per commit.

## What's in scope

spout is a local development port registry. Things that are in scope:

- Port allocation and registration
- Integration with tools developers already use (Docker Compose, Makefiles, varlock)
- Agent-friendliness

Things that are explicitly out of scope:

- Daemons, services, or anything long-running
- Remote port management or cloud sync
- HTTP reverse proxying
- Container orchestration

If in doubt, open an issue first.

## Development setup

```bash
git clone https://github.com/spout-cli/spout
cd spout
cargo test
```

Rust 1.88.0 or higher is required.

## Questions

Open a discussion on GitHub or email the maintainer. Bug reports belong in issues.
