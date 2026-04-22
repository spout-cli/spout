# Rules

## Code

- TDD. Tests before implementation. No exceptions.
- No file > 400 lines. No function > 40 lines. No function > 4 args.
- No `unwrap()` or `expect()` in production code. Permitted only in tests and in `main()` for unrecoverable startup errors.
- All business logic in modules. `main.rs` is dispatch only.
- `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` must pass before commit.

## Behaviour

- stdout is port numbers and list output only. Everything else to stderr — including log output.
- `get`, `ls`, `check` are strictly read-only. Never mutate the registry.
- `alloc`, `set`, `rm` mutate — always through `registry::with_lock`.
- Every error variant maps to exactly one exit code per the PRD.
- Lock file path is derived from registry path, not hardcoded.

## Process

- A **stage** is a coherent multi-commit design effort — monorepo support, UDP, prune. For each stage: read `docs/planning/NN-planning.md` before starting, write `docs/planning/NN-learning.md` after completing.
- Standalone commits — small features, CI/infra, cleanup, docs — don't need the planning bookends. Use conventional commits and land them directly.
- One logical change per commit. Conventional commit messages.

See [CODING_GUIDELINES.md](docs/CODING_GUIDELINES.md) for detail and rationale.
See [docs/planning/](docs/planning/) for stage plans and learnings.
See [spout-prd.md](docs/spout-prd.md) for the product spec.
