# Stage 8 — `--project` on `rm` and `get` (learning)

Retrospective on three commits implementing the project-level `rm` gap
the user identified mid-conversation: prune handles "files are gone"
but there was no clean path for "decommission this still-extant
project."

## What shipped

Tests 189 → 198 (+9). All files under 400 lines. Branch
`claude/project-status-review-0JJPw`.

Commits:
1. `e24f172 docs(planning): Stage 8 plan`
2. `079c326 feat(cli,commands): rm --project [NAME] and get --project <NAME>`
3. `7795ad4 docs: CHANGELOG, README, PRD, llms.txt`

## What the plan got right

- **`RmTarget` enum encodes the matrix at the type level.** The
  3-args entry point (`path`, `target`, `opts`) stays under the
  4-arg cap. Main.rs's `build_rm_target` is the only place the
  matrix lives, and the `(None, None)` case is forced to be an
  explicit error rather than something `run` has to handle.
- **`Registry::remove_project(name, reason)` as a single method.**
  Cleaner than looping `remove()` per service: avoids the
  redundant per-service "is the project entry empty now?" check
  that `remove()` does, and writes a single `today_iso()` value
  shared across all the history entries.
- **Single-block `[y/N]` confirm, not per-entry.** Prune's per-entry
  loop fits stale-audit; "decommission this project" is one
  decision. The prompt prints all the services that would go, then
  one prompt. Easy mental model.
- **Distinct history reason `"user requested (project rm)"`.**
  Differentiates from the existing `"user requested"` string so
  `whois --history` reads informatively after the fact. Same
  pattern as the prune reasons.

## What the plan got wrong

- **`commands/mod.rs` blew the 400 cap by 11 lines** when I added
  the `get_with_explicit_project_reads_from_that_project` test plus
  the `rm_current` test helper. Resolved by inlining `rm_current`
  more compactly and trimming two of the older docstrings on `ls`
  and `env`. Both docstrings were three short paragraphs that
  paraphrased themselves; one tighter paragraph each kept the
  semantic info and shaved enough to land at 396. Carries forward:
  the file is now at 396/400 — the next addition needs an
  extraction (perhaps `set` to its own module, mirroring the prune
  and alloc splits).
- **CLI `rm` ergonomics took longer to settle than expected.** The
  matrix `(service: Option, project: Option<Option>, yes, dry_run)`
  has eight combinations even after pruning the obviously-invalid
  ones, and main.rs's `build_rm_target` had to encode the lot. The
  shape works, but it's the most CLI logic that lives outside
  clap's declarative attributes in the codebase. Worth revisiting
  if more `--project` adds land — clap's `ArgGroup` might help.

## Observations worth carrying forward

- **Reusing prune's stdin-injection pattern.** `confirm()` takes
  `&mut impl BufRead, &mut impl Write` so tests drive it with
  `&[u8]` and `Vec<u8>` — same shape as prune's interactive loop.
  Spout now has two interactive commands and the pattern is
  proven. Future interactive prompts (e.g. `spout realloc` if it
  ever needs confirmation) reuse this directly.
- **`commands::rm` re-export.** `commands/mod.rs` declares
  `pub use rm::{run as rm, RmOptions, RmTarget};` so `commands::rm`
  remains the call site (matching `commands::alloc`,
  `commands::prune`). Keeps the `main.rs` call shape uniform.
- **Skipped `alloc --project` and `set --project`.** The user asked
  if other commands should follow the pattern; my recommendation
  was no for write paths. Writing registrations for a project
  you're not in feels off — both commands derive context from CWD,
  and overriding that for create-side semantics would be a footgun.
  Documented the decision in the planning doc and held the line.

## For the next stage

- **Exit code 7 (`Io`) is being used for usage errors.** main.rs's
  `(None, None) -> Err(SpoutError::Io("specify a service or
  --project"))` reuses the I/O variant for a usage-validation
  failure. The variant fits better than `ComposeInvalid` (which
  the `--udp + no service` path uses), but both are cosmetic
  abuses. A future cleanup might add `SpoutError::Usage(String)`
  at a fresh exit code (9) and route both paths there. PRD §Exit
  Codes table would gain one row.
- **`commands/mod.rs` is at 396/400.** Next addition extracts
  `set` to its own module. Probably trivial; matches the prune
  and alloc splits.
- **`spout realloc <svc>`** is the smallest PRD §18 item still
  parked; would compose cleanly with the new rm pattern.

## Verification actually run

1. `cargo fmt --all -- --check` — green after every commit.
2. `cargo clippy --all-targets -- -D warnings` — green after every
   commit.
3. `cargo test` — 198/198 after the last commit.
4. `wc -l src/**/*.rs` — every file at or under 400. `commands/mod.rs`
   at 396, `commands/rm.rs` at 275.
5. Did not exercise the `[y/N]` prompt against a real terminal; the
   stdin-injection unit tests cover the y / N / EOF paths
   deterministically.
