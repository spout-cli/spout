## What

<!-- What does this PR do? One sentence. -->

## Why

<!-- Why is this change needed? Link to the issue if there is one. -->

Closes #

## How

<!-- Brief notes on the implementation approach. Non-obvious decisions only. -->

## Checklist

- [ ] Tests written before implementation (TDD)
- [ ] All tests pass: `cargo test`
- [ ] Clippy clean: `cargo clippy -- -D warnings`
- [ ] Formatted: `cargo fmt --all -- --check`
- [ ] No file exceeds 400 lines
- [ ] No function exceeds 40 lines
- [ ] No function has more than 4 arguments
- [ ] No `unwrap()` or `expect()` outside tests
- [ ] stdout/stderr contract respected
- [ ] CHANGELOG updated under `[Unreleased]`
- [ ] If this completes a development stage, `docs/planning/NN-learning.md` is written
