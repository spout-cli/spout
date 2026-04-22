# Stage 7 — `spout alloc` from `docker-compose.yml`

## Goal

Ship compose-file inference for `spout alloc`. With no service name,
read `docker-compose.yml` (or one of three sibling names), parse the
`services.*.ports` block to infer TCP/UDP, allocate one port per
service, emit a tabular summary.

Full design: `docs/proposals/compose-alloc.md` (commit `83ad114`).
The three decisions we locked before writing the proposal:

- **File discovery:** auto-detect the four standard names used by
  `project_markers.rs`, with `-f <PATH>` override.
- **Spec scope:** parse the `ports:` block, infer TCP/UDP from
  `/udp` suffix.
- **Output:** tabular summary on stdout, shape mirrors `spout ls`.

Open question from the proposal we're taking the recommended default
on: **multi-port services** get the first port allocated + a stderr
warning. Revisit if real use demands better.

## Approach

TDD; tests before implementation; fmt/clippy/test green between
commits. Five feature commits + docs.

### Commit 1 — `refactor(commands): split commands.rs into commands/ module`

Mirrors the `prune/` split from Stage 6. `src/commands.rs` is at
389/400; adding the compose `alloc` dispatcher pushes it over.
Target shape:

- `src/commands/mod.rs` — thin dispatcher with `get`, `set`, `rm`,
  `ls`, `env`, `check`, `whois`, `validate_port`. Keeps the existing
  `pub use prune::run as prune;` re-export pattern.
- `src/commands/alloc.rs` — new module. Moves the existing
  `pub fn alloc(path, service, protocol)` here. Compose entry point
  (`pub fn compose(...)`) lands in Commit 4.

Zero behaviour change. Tests stay inline in the per-module `tests`
blocks. Same pattern: `git mv` `commands.rs` to `commands/mod.rs` to
preserve rename history, then carve `alloc.rs` out.

### Commit 2 — `feat(deps): add serde_yaml_ng for compose parsing`

One-line `Cargo.toml` edit:

```toml
serde_yaml_ng = "0.10"
```

No new Rust code. CI runs the full gate to catch any transitive
surprise.

### Commit 3 — `feat(alloc): parse compose services and port specs`

New `src/commands/alloc/compose.rs`. Pure-function layer:

```rust
#[derive(Debug, PartialEq)]
pub(super) struct ComposeService {
    pub name: String,
    pub port: Option<(u16, Protocol)>,
    pub extra_port_count: usize,  // for multi-port warning
}

pub(super) fn parse(yaml: &str) -> Result<Vec<ComposeService>, SpoutError>;
```

Port-spec parsing handles the four forms listed in the proposal.
Ranges (`"9000-9005:9000-9005"`) and bind IPs
(`"127.0.0.1:5432:5432"`) parse to the container port; unparseable
entries are dropped with the service's `extra_port_count`
incremented so the caller can warn.

Tests against fixture strings (no filesystem):
- Short form `"5432"` → 5432/tcp.
- Host:container `"5432:5432"` → 5432/tcp.
- Protocol suffix `"53:53/udp"` → 53/udp.
- Long form `{target: 8080, protocol: udp}` → 8080/udp.
- Bind-IP `"127.0.0.1:5432:5432"` → 5432/tcp.
- Multi-port service `ports: ["5432", "9229"]` → first kept,
  `extra_port_count = 1`.
- No `ports` block → `port = None`.
- Malformed YAML → `SpoutError::ComposeInvalid`.

### Commit 4 — `feat(cli,commands): --file flag, auto-detect, batch alloc`

Wiring + the batch-lock allocator:

- `src/cli.rs`: `Alloc { service: Option<String>, #[arg(long)] udp: bool, #[arg(short = 'f', long = "file")] file: Option<PathBuf> }`.
- `src/main.rs`: dispatch matrix per the proposal. `service=None` + `udp=true` is a clap `conflicts_with` or a runtime usage error.
- `src/commands/alloc.rs`: new `pub fn compose(registry_path, file: Option<&Path>)` orchestrates discovery → parse → batch alloc.
- `src/allocator.rs`: new `alloc_many(&mut Registry, project, &[(service, protocol)])` that walks candidates and mutates in-place — designed to be called *inside* a `with_lock` closure. `compose()` acquires the lock once and calls `alloc_many` to avoid N fsyncs. The single-service `alloc()` stays lock-aware as-is for its sole caller.
- `src/error.rs`: `ComposeNotFound` and `ComposeInvalid(String)` variants at exit code 8. Test for each.
- Output: new helper `format::compose_summary(reg, names, new_count)` for the tabular block. Services emit in compose-file-declaration order (preserve `Vec` ordering, don't re-sort).

Tests:
- `compose_discovers_docker_compose_yml` (tempdir + seeded file).
- `compose_auto_detects_each_of_the_four_names`.
- `compose_honours_explicit_f_override`.
- `compose_missing_file_exits_eight`.
- `compose_allocates_all_services_in_one_lock` (assert registry has N entries after one `with_lock`).
- `compose_second_run_is_idempotent` (reruns return same ports).
- `compose_multi_port_service_warns_on_stderr_and_allocs_first`.
- `compose_udp_inferred_from_slash_udp`.
- `compose_bare_alloc_with_udp_flag_is_usage_error`.

### Commit 5 — `docs: CHANGELOG, README, PRD, llms.txt for compose alloc`

- CHANGELOG `[Unreleased]` entry covering the command, the flag, the
  auto-detect, the TCP/UDP inference, the multi-port warning, the
  new exit code.
- README: new section near the other per-feature explainers.
- PRD: §3.2 mutation boundary row for `spout alloc` stays ✅ (same as
  single-service); §6 CLI block shows the new invocations; §Exit
  Codes gains row 8 (`ComposeNotFound` / `ComposeInvalid`); §18
  drops the "Compose inference" bullet (now shipped).
- `llms.txt`: extend the `spout alloc` block.

No code changes.

## Process housekeeping

- Write `docs/planning/07-planning.md` before Commit 1 (this doc).
- Write `docs/planning/07-learning.md` after Commit 5.
- Push at natural beats: after Commit 1 (split lands clean), after
  Commit 3 (parser visible), after Commit 5 (feature done).

## Files

Line budgets under the 400-line cap:

| File | Now | After | Headroom |
|---|---|---|---|
| `src/commands.rs` | 389 | — | splits into `commands/{mod,alloc}` |
| `src/commands/mod.rs` | — | ~275 | |
| `src/commands/alloc.rs` | — | ~120 | Commit 1 scaffolding |
| `src/commands/alloc/compose.rs` | — | ~250 | parser + discovery + summary |
| `src/allocator.rs` | 270 | ~300 | `alloc_many` adds ~25 |
| `src/cli.rs` | 246 | ~260 | `-f` flag + a test |
| `src/error.rs` | 153 | ~170 | two variants + tests |
| `Cargo.toml` | 24 | 25 | serde_yaml_ng |

Test count expected to grow from 162 to ~185.

## Verification

1. `cargo test` green after every commit.
2. Post-Commit 2: `cargo build` on a clean target/ completes with
   the new dep; no transitive surprise.
3. Post-Commit 3: 10+ new parser tests all green; parser is a pure
   function with no filesystem or lock access.
4. Post-Commit 4: end-to-end with a seeded tempdir —
   ```
   SPOUT_REGISTRY=/tmp/s.json cd <tempdir with docker-compose.yml>
   spout alloc         # prints tabular summary, allocates N services
   spout alloc         # prints same summary, no new allocations (idempotent)
   spout ls --project  # shows the same N services
   ```
5. Final gate: fmt, clippy (`--all-targets`), tests green. `wc -l
   src/**/*.rs` — all files under 400.

## Risks

- `serde_yaml_ng` parses YAML via a libyaml binding. Compile-time
  impact on CI is a known ~2–3s — acceptable. Runtime parse is
  single-digit ms for typical compose files.
- Port-spec parsing has a long tail of edge cases (see proposal §
  Non-goals). MVP deliberately handles only the common four forms;
  everything else drops with a stderr warning. Real users with exotic
  compose files will hit the warnings and can split services
  manually.
- `alloc_many` is a new allocator entry point. Risk: subtle
  difference in behaviour between single-service `alloc()` and
  `alloc_many()`. Mitigation: `alloc_many` is implemented as a loop
  that calls the same candidate-walk logic; differences are purely
  about lock scope, not allocation semantics. Unit tests cover both.

## Out of scope

- `--dry-run` for compose alloc (deferred per proposal).
- Multi-port naming beyond "first + warn."
- `extends`/`include`/`${VAR}` interpolation.
- Compose profiles.
- Multi-protocol services (TCP+UDP on same port/name).
