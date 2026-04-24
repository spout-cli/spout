# `spout alloc` — compose-file inference

_Status: proposal, 2026-04-22. Not committed to a stage yet._

## Context

Every new multi-service project today means running `spout alloc
<service>` once per service, typing six or eight service names by
hand. The compose file already enumerates them. PRD §18 reserves
the one-line behaviour: "`spout alloc` with no arguments parses
`docker-compose.yml` in the current directory and auto-allocates for
every service declared." This doc works through what that actually
means.

## Goals

1. `spout alloc` with no service name reads the compose file in the
   current directory and allocates a port per declared service.
2. `-f, --file <PATH>` overrides the filename. Default behaviour
   auto-detects `docker-compose.yml`, `docker-compose.yaml`,
   `compose.yml`, `compose.yaml` in that order — the same four names
   `src/project_markers.rs` already recognises for monorepo detection.
3. Each allocation is idempotent: re-running against the same file
   returns the same ports, identical to today's
   `allocator::alloc()` semantics per `(project, service)` pair.
4. Protocol is inferred from the compose port spec (`/udp` suffix →
   UDP, everything else → TCP). TCP is the default for services
   whose port spec lacks a protocol marker.
5. Output is a tabular summary to stdout (service, port, protocol),
   matching the list-output shape of `spout ls`. Exit code 0 on
   success, 2 on no-free-port.

## Non-goals

- Writing back to the compose file (spout doesn't mutate user files —
  ever).
- Resolving `extends`, `include`, `${VAR}` interpolation, or
  YAML merge-key chains that span files. Spec-compliant handling
  would swallow a whole stage and isn't the 80% case.
- Parsing `depends_on`, `networks`, `volumes`, or any compose field
  other than `services.*` and `services.*.ports`.
- `docker-compose up` parity. Spout stays a port registry.
- Running against a running compose project (use `spout whois` for
  that direction).
- Windows: WSL-only per the existing PRD scope.

## Design

### CLI surface

```
spout alloc                    # read ./docker-compose.yml (or siblings); alloc all services
spout alloc -f compose.prod.yml   # explicit filename
spout alloc <service>          # existing single-service mode; unchanged
spout alloc <service> --udp    # existing UDP mode; unchanged
```

Clap shape:

```rust
Alloc {
    service: Option<String>,
    #[arg(long)] udp: bool,
    /// Path to a compose file. Default: auto-detect the standard names
    /// in the current directory. Ignored when <service> is given.
    #[arg(short = 'f', long = "file", value_name = "PATH")]
    file: Option<PathBuf>,
}
```

Dispatch matrix:

| `service` | `--udp` | `-f` | Behaviour |
|---|---|---|---|
| `Some(s)` | any | ignored | Today's single-service alloc (`commands::alloc_one`). |
| `None` | `false` | `None` | Compose alloc, auto-detect filename. |
| `None` | `false` | `Some(p)` | Compose alloc, read `p`. |
| `None` | `true` | any | Usage error: `--udp is per-service; pass a service name or declare UDP in the compose port spec`. |

### File discovery

```rust
fn discover_compose_file(explicit: Option<&Path>) -> Result<PathBuf, SpoutError> {
    if let Some(p) = explicit {
        return if p.exists() { Ok(p.to_owned()) } else { Err(...) };
    }
    for name in ["docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"] {
        let candidate = Path::new(name);
        if candidate.exists() {
            return Ok(candidate.to_owned());
        }
    }
    Err(SpoutError::ComposeNotFound)
}
```

A new `SpoutError::ComposeNotFound` variant is needed. Exit code 8 —
the next free code after the existing 1–7. PRD exit-code table will
grow by one row.

### Port-spec parsing

Compose supports several port-spec forms. We handle the three the
vast majority of real files use:

| Form | Example | Parses to |
|---|---|---|
| Numeric shorthand | `"5432"` | container 5432, protocol tcp |
| Host:container | `"5432:5432"` | container 5432, protocol tcp |
| With protocol | `"53:53/udp"` | container 53, protocol udp |
| Long form | `{target: 8080, protocol: tcp}` | container 8080, protocol tcp |

Everything else (port ranges `"9000-9005:9000-9005"`, bind IPs
`"127.0.0.1:5432:5432"`, `published`/`host_ip`/`mode` fields in long
form) parses as "present but spout ignores the extras" — the port
and protocol are extracted, other fields dropped.

Services with no `ports` block: skipped silently. Services with
non-parseable port specs: skipped with a stderr warning; spout
doesn't fail the whole allocation because one entry is weird.

### Service → registration mapping

- **One port per service (MVP).** The most common case. Service name
  from the compose `services.<name>:` key, port allocated from
  spout's 20000–32767 range, protocol from the port spec.
- **Multi-port services.** If a service declares 2+ ports, MVP
  allocates only the first and emits a stderr warning:
  `spout: 'api' declares 2 ports; allocating only the first (split
  multi-port services into separate compose services, or see
  --multi-port in a future release)`. An open question below.
- **Existing registrations stay.** Same idempotency as today —
  `allocator::alloc()` returns an existing port for the (project,
  service) pair without probing.
- **Single-lock batch.** Stage 6's `prune --yes` learned that calling
  `registry::with_lock` N times for N registrations means N fsync +
  read-parse-write cycles and no transactional semantics on partial
  failure. Compose alloc should mirror the fix: build a
  `Vec<(service, protocol)>` from the compose parse, then acquire
  `with_lock` once and loop `alloc_within_lock(&mut Registry, ...)`
  inside the closure. `allocator::alloc()`'s current shape
  (lock-per-call) suits single-service invocations; compose-batch
  needs either a new `alloc_within_lock` helper or a bulk entry
  point on the allocator.

### Output format

After all allocations, write a tabular block to stdout, shaped like
`spout ls` per-project output and reusing `format::port_status_glyph`:

```
$ spout alloc
docker-compose.yml → 4 services allocated.

  ● postgres     20000  tcp
  ● redis        20001  tcp
  ● coredns      20002  udp
  ● api          20003  tcp
```

Stdout is still list output, preserving the contract. Scripts that
need machine-readable output use `spout env` after.

Already-allocated services show the existing port but flag it:

```
docker-compose.yml → 4 services (2 new, 2 existing).
```

Order is compose-file-declaration order, to match what the user sees
when they open the file.

### Dependency

Adding `serde_yaml_ng = "0.10"` to `Cargo.toml`. This is the first
YAML parser spout has needed; the `_ng` fork is the maintained
successor to the archived `serde_yaml` (dtolnay archived it 2024).
Compose files are the only YAML spout reads or writes, and `_ng`'s
API is a drop-in `serde::Deserialize`-based loader.

Weight: one libyaml-based transitive parser. Compile time adds ~2–3 s
on a clean build per the crate's README.

### Error model

- `SpoutError::ComposeNotFound` (exit 8) — auto-detect turned up
  nothing.
- `SpoutError::ComposeInvalid(String)` (exit 8) — YAML parse failed,
  or `services` is not a map. Shares exit code with ComposeNotFound;
  reusable as a category.
- `NoFreePortFound` (exit 2, existing) — range is full; propagate as
  today.
- Service-name collisions with existing registrations: not an error.
  `alloc()` returns the existing port. Compose-alloc treats this as
  "already allocated, no work to do" and reports it in the tabular
  output.

### Code layout

A new `src/commands/alloc/` submodule (splitting `commands.rs`
before it tips the cap), mirroring the `prune/` pattern:

- `src/commands/alloc/mod.rs` — dispatch: single-service vs
  compose. Takes the CLI args, picks the path.
- `src/commands/alloc/compose.rs` — file discovery, YAML parse,
  iterate services, allocate, format the tabular summary.
- Tests colocated in `compose.rs` — parse fixtures, allocation,
  multi-port warning, idempotency.

The existing `commands::alloc` wrapper stays as the single-service
entry point; the new module adds `commands::alloc::compose()`.

## Open questions

1. **Multi-port service naming.** _Resolved — every port is
   registered. The first keeps the bare service name; extras are
   suffixed with their container port (e.g. `mailpit` +
   `mailpit-1025`). Collisions beyond the first port (same container
   port declared twice) skip with a stderr warning rather than
   invent a hidden `-2` discriminator. The long-form `name:` path is
   still the cleanest refinement if we ever want user-chosen names._

2. **`--dry-run`.** `spout prune --dry-run` is useful. Compose
   allocation is mostly idempotent, so a dry run matters less — but
   "show me which services in this compose file would get ports"
   without mutating is a real debugging question. Defer; add `-n,
   --dry-run` as a follow-up if asked.

3. **Services without `ports`.** Current proposal: skip silently.
   Alternative: allocate anyway (for services that will later get a
   port assignment). Skip silently is more conservative — the user
   can `spout alloc <service>` explicitly if they want one without
   a port declaration. Revisit after real use.

4. **`extends` / `include` / interpolation.** All deferred. The
   user's compose file needs to be self-contained for MVP. If
   `services:` itself uses `extends: { service: base, file: ... }`,
   spout sees the raw reference and treats it as a service entry
   without ports (skip + warning). An `extends`-aware follow-up is a
   separate proposal.

5. **Protocol for multi-protocol services.** A compose service that
   binds both TCP 53 and UDP 53 (like a DNS resolver) declares
   `ports: ["53:53/tcp", "53:53/udp"]`. MVP's "first port wins"
   rule gives it one protocol and drops the other. Same multi-port
   naming problem; same deferral.

## Out of scope

- Any change to `docker-compose.yml` content. Spout reads; it never
  writes to compose files.
- Network probing of container services at their host-mapped ports.
- Reading compose profiles (`profiles:` / `COMPOSE_PROFILES`).
  Allocates for every declared service regardless.

## Stage shape

Candidate next-stage structure. Six commits roughly:

1. `docs(proposals): compose-alloc design doc` (this document).
2. `refactor(commands): split src/commands.rs into commands/{mod,alloc,...}` — preemptive, following the prune pattern. Only if `commands.rs` (389/400) is at risk.
3. `feat(deps): add serde_yaml_ng for compose parsing` — isolated, so any CI surprise is a one-commit revert.
4. `feat(alloc): parse compose services and port specs` — pure function (YAML → `Vec<ComposeService>`), tests against fixture strings.
5. `feat(cli,commands): spout alloc --file and auto-detect, multi-port warning` — wiring, dispatch matrix, tabular output.
6. `docs: CHANGELOG, README, PRD §3.2 + §6 + §18, llms.txt` — stop listing compose inference as future work.

## Verification

1. `cargo test` green after every commit; tests grow by ~15
   (YAML parse cases, discovery, multi-port warning, idempotency,
   TCP/UDP inference).
2. Fixture-file test: ship a `tests/fixtures/compose-basic.yml`
   with five services covering short-form, long-form, `/udp`, and
   a no-ports service. Parse + assertEq against expected
   allocations.
3. End-to-end from a tempdir with a seeded `docker-compose.yml`:
   `SPOUT_REGISTRY=/tmp/s.json spout alloc` returns a tabular
   summary; rerun is idempotent; `spout ls --project` lists the
   same entries.
4. Negative path: missing compose file → exit 8 with
   `ComposeNotFound`; malformed YAML → exit 8 with `ComposeInvalid`.
5. `cargo clippy --all-targets -- -D warnings` and
   `cargo fmt --all -- --check` clean. All src files under 400
   lines (watch `commands.rs` and the new `alloc/compose.rs`).

## Next step

Decide the open questions (especially multi-port naming), approve
the stage shape, and open `docs/planning/07-planning.md` to start
implementation.
