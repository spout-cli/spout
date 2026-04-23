# Stage 9 — compose override file support for `spout alloc`

## Goal

Close the faithfulness gap in `spout alloc`'s compose-mode scan.

Today, `spout alloc` (no service arg) auto-detects **one** compose file
from `COMPOSE_FILENAMES` and reads only that. Real-world projects
routinely split port declarations out into an override file — the base
`docker-compose.yml` declares services without `ports:` (because
prod/Dokploy/Traefik handles routing), and `docker-compose.override.yml`
adds `ports:` stanzas for local dev. Running `spout alloc` against such
a repo misses every service that only gets its ports from the override
file — exactly the services the user needs spout for.

Docker Compose itself auto-loads both files and merges them. Spout
diverging from that is a bug, not a missing feature.

Concrete canonical example: `/home/pete/dev/go/prompttamer`. Base
declares `postgres`, `api`, `adminer`, `mailpit` with no `ports:`.
Override adds ports to all four. `spout alloc` today allocates zero of
them (only `frontend` and `glitchtip-web` have base-level `ports:`, and
both are behind profiles the dev never runs).

## Decisions locked

- **Auto-detect override alongside base.** Same four stems as today,
  with `.override` inserted:
  - `docker-compose.override.yml`
  - `docker-compose.override.yaml`
  - `compose.override.yml`
  - `compose.override.yaml`

  Matched independently from the base — e.g. a `docker-compose.yml`
  base pairs with whichever override variant exists, not just the
  matching stem. Keeps the detection logic simple and mirrors what devs
  actually do (nobody cares that their override is `.yaml` when base is
  `.yml`).

- **Merge rule: override wins on `ports:` per service.** Simpler than
  docker-compose's spec (which appends + dedups for `ports:`), but
  produces the same outcome for every realistic case spout cares about:

  | Base has `ports:`? | Override has `ports:`? | Result             |
  |:------------------:|:----------------------:|--------------------|
  | No                 | No                     | Skip service       |
  | No                 | Yes                    | Use override ports |
  | Yes                | No                     | Use base ports     |
  | Yes                | Yes                    | **Use override**   |

  The last row is where we diverge from docker's append semantics.
  Documented in CHANGELOG and the `compose()` doc comment. If someone
  reports a real project broken by this, we revisit.

- **`-f` accepts multiple files, chained in order.** Replaces the
  current single-value `-f` with a `Vec<PathBuf>`. Last-wins across
  the chain using the same merge rule above. If `-f` is given at all,
  auto-detect is disabled entirely — no mixing user-specified files
  with auto-detected override. Explicit beats implicit.

- **Override without base is a usage error.** If auto-detect finds
  only an override file and no base, error out with a clear message
  ("override compose file found but no base — pass the base with
  `-f`"). Unlike docker, which also rejects this, spout's error should
  be friendlier than "failed to open docker-compose.yml".

- **Profile filtering stays out.** Today's parser doesn't honour
  `profiles:`, and we're not adding it here. Spout allocates ports
  whether or not the service is in an active profile — the user is
  declaring intent to run it eventually. Out of scope.

- **`COMPOSE_FILE` env var stays out.** Docker's env-var-driven file
  chain can wait until someone asks for it.

- **v1.0 gate.** This lands before the v1.0 tag. Compose-alloc is a
  flagship feature in the `[Unreleased]` changelog section; shipping
  it without override support means it silently misses the most common
  real-world compose pattern. Cheaper to fix now than to document-and-
  defer.

## Approach

TDD; tests before implementation; fmt/clippy/test green between
commits. Three commits, roughly even in size.

### Commit 1 — `feat(alloc): auto-detect compose override alongside base`

`src/commands/alloc/compose.rs`:
- Rename `ComposeService` → keep the name; the shape is unchanged.
- Extract compose file merge into a pure function:
  `merge_services(base: Vec<ComposeService>, overlay: Vec<ComposeService>) -> Vec<ComposeService>`.
  Override-wins semantics. Pure `Vec` → `Vec` for unit-testability.
- Change `parse` to stay `&str → Result<Vec<ComposeService>>` (no
  change) so it composes cleanly.

`src/commands/alloc/mod.rs`:
- `discover_compose` returns `(PathBuf, Option<PathBuf>)` — base, then
  optional override. Add a parallel `OVERRIDE_COMPOSE_FILENAMES`
  constant in `src/project_markers.rs` (grouped with the existing
  `COMPOSE_FILENAMES` so future renames stay co-located).
- `compose()` reads both files if present, parses each, calls
  `merge_services`, then allocates from the merged list.
- Update the summary header to include both files when both are read:
  `docker-compose.yml + docker-compose.override.yml → 5 services allocated.`

Tests (in `commands/alloc/mod.rs::tests` and `compose.rs::tests`):
- `discover_returns_base_and_override_when_both_exist`
- `discover_returns_base_only_when_no_override`
- `discover_returns_err_on_override_without_base` (with the friendly
  message)
- `merge_override_adds_new_ports_to_portless_base` — the prompttamer
  case, most important
- `merge_override_wins_when_both_declare_ports`
- `merge_preserves_base_only_services`
- `merge_preserves_override_only_services`
- `merge_protocol_follows_winning_file` (UDP in override, TCP in base
  → UDP wins)

### Commit 2 — `feat(cli,alloc): accept multiple -f flags, chained in order`

`src/cli.rs`:
- Change `Alloc::file: Option<PathBuf>` → `Alloc::files: Vec<PathBuf>`
  with `#[arg(short = 'f', long = "file")]` and clap's default array
  accumulation semantics.

`src/commands/alloc/mod.rs`:
- `compose()` takes `files: &[PathBuf]`. If non-empty, parse them in
  order and fold with `merge_services` left-to-right — last wins. No
  auto-detect.
- If empty, fall through to today's auto-detect path (now base +
  override aware from Commit 1).

Tests:
- `explicit_files_chain_in_order_last_wins`
- `explicit_files_disable_autodetect` — i.e. `-f foo.yml` in a
  directory with `docker-compose.override.yml` present ignores the
  override
- `explicit_missing_file_is_compose_not_found` (unchanged contract)
- `explicit_malformed_file_is_compose_invalid` (unchanged contract)

### Commit 3 — `docs: override compose file support for spout alloc`

- `README.md` — the "Compose-file mode" paragraph in the alloc section
  gains a short subsection: "Override files are auto-loaded if present,
  matching `docker compose up` behaviour. For non-standard filenames,
  chain with `-f file1 -f file2`." One-line callout on the `ports:`
  merge rule divergence.
- `CHANGELOG.md` `[Unreleased]` — one new Added bullet.
- `llms.txt` — one-line note in the `spout alloc [-f <PATH>]` entry.
- `docs/spout-prd.md` — §5 `alloc` description gains one sentence on
  override support. No §18 changes needed.
- No `templates/CLAUDE.md` change — the downstream agent primer
  already says "prefer `spout alloc` from the compose file"; override
  handling is transparent.

## Test matrix

Covered across commits:

| Scenario                                | Expected                              |
|-----------------------------------------|---------------------------------------|
| Base only (today)                       | Unchanged — no regression             |
| Override only (no base)                 | Friendly `ComposeNotFound` error      |
| Base + override, override adds ports    | All services allocated ← prompttamer  |
| Base + override, same service in both   | Override's ports win                  |
| Base + override, different protocols    | Winning file's protocol               |
| `-f a.yml` with override in CWD         | Only `a.yml` — no auto-detect         |
| `-f a.yml -f b.yml`                     | b-then-a merge, last wins             |
| `-f missing.yml`                        | `ComposeNotFound` exit 8              |
| `-f malformed.yml`                      | `ComposeInvalid` exit 8               |

## Error model

No new error variants. `ComposeNotFound` (exit 8) and `ComposeInvalid`
(exit 8) cover every failure mode. The "override without base"
friendly message attaches a context string to `ComposeNotFound`.

## Not doing

- `COMPOSE_FILE` env var (defer unless asked)
- `profiles:` filtering (out of scope; today's behaviour preserved)
- Docker's full append-and-dedup `ports:` merge semantics (defer until
  a real project hits the edge case)
- Any change to the single-service `spout alloc <service>` path — this
  stage touches only the compose-mode branch
- `spout alloc --dry-run` (requested once informally, still no user
  demand)

## Critical files

- `src/commands/alloc/compose.rs` — parse stays put, add `merge_services`
- `src/commands/alloc/mod.rs` — `discover_compose` signature change,
  `compose()` reads two files
- `src/project_markers.rs` — add `OVERRIDE_COMPOSE_FILENAMES` constant
- `src/cli.rs` — `-f` becomes `Vec<PathBuf>`
- `README.md`, `CHANGELOG.md`, `llms.txt`, `docs/spout-prd.md` — docs

## Risks

1. **Clap's multi-value `-f` semantics.** Need to verify that `-f a.yml
   -f b.yml` produces `vec![a, b]` not a single merged string. The
   `#[arg(long)]` + `Vec<_>` pattern should do it; check against
   existing uses in the tree.

2. **Merge order clarity in summary output.** When both base and
   override are read, the summary should cite both files so the user
   knows what spout saw. Keep the format short — one line, comma-
   separated paths.

3. **Test isolation.** Existing compose tests use `TempDir`. New
   override tests need two files per temp dir; make sure the existing
   `write_compose` helper composes cleanly (it does — it takes a
   filename arg).
