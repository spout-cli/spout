# spout — Coding Guidelines

These rules exist to make the codebase maintainable by humans and AI agents alike. They are not suggestions.

---

## Core Principles

**Simple is clever.** If you are proud of how clever a solution is, it is probably wrong. The best code reads like prose. The next person to touch this file might be an LLM at 3am — write accordingly.

**Maintainability is king.** Optimise for readability and changeability, not performance. Performance problems can be fixed. Incomprehensible code cannot.

**TDD first.** Tests are written before implementation. If you cannot write a test for it, you do not understand the requirement well enough to implement it.

---

## File and Function Rules

### Maximum file length: 400 lines

No file may exceed 400 lines. Hard limit. No exceptions.

When a file approaches 400 lines, split it before it hits the limit. Do not wait until you are over. The correct response to "this file is getting long" is to split it now, not to finish the feature first.

**How to split:** Extract cohesive groups of functions into new modules. If you cannot find a cohesive group to extract, the file's responsibilities are already too mixed — fix the design first.

### Maximum function length: 40 lines

Functions longer than 40 lines are doing too much. Extract sub-functions. A named sub-function is always preferable to a comment explaining what a block does.

### Maximum function arguments: 4

If a function needs more than 4 arguments, introduce a config struct. This makes call sites self-documenting and future changes localised.

```rust
// ❌ Too many arguments
fn allocate_port(project: &str, service: &str, start: u16, end: u16, check_os: bool) -> Result<u16>

// ✅ Use a struct
struct AllocOptions {
    start_port: u16,
    end_port: u16,
    check_os: bool,
}
fn allocate_port(project: &str, service: &str, opts: AllocOptions) -> Result<u16>
```

---

## Test-Driven Development

### The cycle

1. Write a failing test that describes the behaviour you want
2. Write the minimum code to make it pass
3. Refactor — clean up without changing behaviour
4. Repeat

Tests are not written after the fact. They are not written to hit a coverage number. They are written first, as a design tool.

### Test file location

Unit tests live in the same file as the code they test, in a `#[cfg(test)]` module at the bottom:

```rust
// src/registry.rs
pub fn get_port(...) { ... }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_returns_none_when_service_not_registered() { ... }
}
```

Integration tests that span multiple modules live in `tests/`.

### Test naming

Test names are sentences describing behaviour:

```rust
// ❌ Bad — describes implementation
fn test_get_port_hashmap_lookup()

// ✅ Good — describes behaviour
fn get_returns_none_when_service_not_registered()
fn alloc_is_idempotent_for_existing_service()
fn get_never_mutates_registry()
```

### What to test

Every public function needs tests for:
- The happy path
- All documented error cases (every exit code must have a test)
- Boundary values and edge cases
- Concurrent access for anything touching the registry

---

## Error Handling

### No `unwrap()` or `expect()` in production code

Permitted only in tests and in `main()` for truly unrecoverable startup failures. Everywhere else, propagate errors explicitly.

```rust
// ❌ Never in production paths
let content = fs::read_to_string(path).unwrap();

// ✅ Propagate
let content = fs::read_to_string(path)
    .map_err(|e| SpoutError::RegistryUnreadable(e))?;
```

### Error types

Use a single top-level `SpoutError` enum. Every variant maps to exactly one exit code. No error is silently swallowed.

```rust
pub enum SpoutError {
    ServiceNotRegistered,        // exit 1
    NoFreePortFound,             // exit 2
    RegistryCorrupt(io::Error),  // exit 3
    RegistryVersionUnknown(u32), // exit 4
    PortAlreadyClaimed(String),  // exit 5
    PortInUse(u16),              // exit 6
}
```

### stdout/stderr contract

All user-facing errors go to stderr. Port numbers and list output go to stdout. This is enforced in tests — stdout must be capturable and clean.

---

## UI: Ratatui

`spout ls` and `spout prune` use [Ratatui](https://ratatui.rs) for their human-facing interactive display. All other commands remain plain stdout/stderr — agents call those commands programmatically and must not receive TUI output.

**Rule:** Ratatui is only invoked when stdout is a TTY. If stdout is piped or redirected, fall back to plain text. Use `std::io::IsTerminal` to detect this.

```rust
use std::io::IsTerminal;

if std::io::stdout().is_terminal() {
    // render with Ratatui
} else {
    // plain text output
}
```

**Stack:**
- `ratatui` — UI widgets and layout
- `crossterm` — terminal backend (default, cross-platform)

**MSRV:** Ratatui requires Rust **1.88.0** minimum. All CI pipelines must pin to this version or higher.

**Module:** All Ratatui code lives in `src/tui.rs`. It must not exceed 400 lines. Split into `src/tui/` sub-modules if needed. Zero Ratatui imports anywhere outside `src/tui.rs`.

---

## Module Structure

```
src/
  main.rs          # CLI parsing and dispatch only — target ≤ 100 lines
  cli.rs           # Argument definitions (clap)
  registry.rs      # Registry read/write, file locking, atomic writes
  allocator.rs     # Port allocation logic, OS port checking
  error.rs         # SpoutError enum and exit code mapping
  project.rs       # CWD-based project name inference
  services.rs      # Well-known service → default port mapping
  tui.rs           # Ratatui UI — only loaded when stdout is a TTY
docs/
  planning/
    README.md        # Numbering convention and stage index
    01-planning.md   # Architecture decisions before coding starts
    01-learning.md   # Learning doc after stage 1 complete
    ...              # One planning + one learning doc per stage
tests/
  registry_integration.rs
  allocator_integration.rs
  cli_integration.rs
```

`main.rs` contains only CLI parsing and dispatch. All logic lives in modules. Business logic in `main.rs` is a bug.

---

## Documentation

### Docs directory

Every **stage** — a coherent multi-commit design effort — produces two documents in `docs/planning/`:

- **`NN-planning.md`** — written *before* coding begins. What are we building? What are the design options? What did we decide and why?
- **`NN-learning.md`** — written *after* the stage is complete. What did we learn? What surprised us? What would we do differently?

Standalone commits — single-feature additions, CI/infra work, cleanup, docs — don't need the bookends. See [`docs/planning/README.md`](planning/README.md) for the numbering convention.

These docs are mandatory, not optional. They are for future contributors and future agents. They capture the *why*, not just the *what*.

### Inline comments

Comments explain *why*, not *what*. The code explains what.

```rust
// ❌ Explains what (the code already says this)
// Increment the port by 1
port += 1;

// ✅ Explains why
// Walk forward one port at a time rather than jumping — the registry
// is sparse and staying close to the well-known default reduces
// surprise for developers reading the registry file directly.
port += 1;
```

Public functions and types get doc comments (`///`). Private functions get inline comments only where the reasoning is non-obvious.

---

## Clippy and Formatting

Run both before every commit. All warnings are errors.

```bash
cargo fmt --all -- --check
cargo clippy -- -D warnings
```

Enforce in `Cargo.toml`:

```toml
[lints.rust]
unsafe_code = "deny"

[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
```

`#[allow(...)]` requires an inline comment explaining why. No silent suppressions.

---

## CI Configuration

Rust CI compatibility is confirmed across all major platforms. The binding constraint is **Ratatui's MSRV of 1.88.0** — all CI pipelines pin to this version.

### CI environment variables

Set these in all pipelines:

| Variable | Value | Reason |
|----------|-------|--------|
| `CARGO_TERM_COLOR` | `always` | Coloured compiler output in logs |
| `CARGO_INCREMENTAL` | `0` | Incremental compilation is wasteful in CI |
| `RUSTFLAGS` | `-D warnings` | Promotes all warnings to errors |
| `SPOUT_REGISTRY` | job-specific path | Prevents concurrent job collisions |

### GitHub Actions

Rust is **pre-installed** on all GitHub Actions runners (`ubuntu-latest`, `macos-latest`, `windows-latest`). Use `dtolnay/rust-toolchain` only to pin the version.

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  CARGO_INCREMENTAL: "0"
  RUSTFLAGS: "-D warnings"
  SPOUT_REGISTRY: /tmp/spout-${{ github.run_id }}.json

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: "1.88.0"
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy -- -D warnings

  test:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: "1.88.0"
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --verbose

  release:
    runs-on: ubuntu-latest
    if: startsWith(github.ref, 'refs/tags/v')
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: "1.88.0"
      - run: cargo build --release --verbose
```

### GitLab CI

GitLab CI is Docker-based. Rust is not pre-installed — use the official `rust` image.

```yaml
# .gitlab-ci.yml
image: rust:1.88.0

variables:
  CARGO_TERM_COLOR: always
  CARGO_INCREMENTAL: "0"
  RUSTFLAGS: "-D warnings"
  SPOUT_REGISTRY: /tmp/spout-${CI_JOB_ID}.json

stages:
  - lint
  - test

fmt:
  stage: lint
  script:
    - rustup component add rustfmt
    - cargo fmt --all -- --check

clippy:
  stage: lint
  script:
    - rustup component add clippy
    - cargo clippy -- -D warnings

test:
  stage: test
  script:
    - cargo test --verbose
```

### CircleCI

CircleCI uses the `cimg/rust` convenience image.

```yaml
# .circleci/config.yml
version: 2.1

jobs:
  test:
    docker:
      - image: cimg/rust:1.88.0
    environment:
      CARGO_TERM_COLOR: always
      CARGO_INCREMENTAL: "0"
      RUSTFLAGS: "-D warnings"
      SPOUT_REGISTRY: /tmp/spout-registry.json
    steps:
      - checkout
      - run:
          name: Format check
          command: cargo fmt --all -- --check
      - run:
          name: Clippy
          command: cargo clippy -- -D warnings
      - run:
          name: Test
          command: cargo test --verbose

workflows:
  ci:
    jobs:
      - test
```

---

## Git

Commit messages follow conventional commits:

```
feat: add spout alloc command
fix: handle IPv6 in port availability check
test: add concurrent allocation regression test
docs: add registry design learning doc
refactor: extract port walking into allocator module
chore: pin toolchain to 1.88.0 for ratatui MSRV
```

One logical change per commit. Do not bundle refactors with features.

---

## What "Done" Means

A piece of work is done when:

1. Tests written first, all passing
2. `cargo clippy -- -D warnings` passes
3. `cargo fmt --all -- --check` passes
4. No file exceeds 400 lines
5. No function exceeds 40 lines
6. No function has more than 4 arguments
7. No `unwrap()` or `expect()` outside tests
8. stdout/stderr contract respected
9. For stage work: `docs/planning/NN-learning.md` is written
10. CI passes on push
