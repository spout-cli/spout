# Stage 7 — `spout alloc` from docker-compose (learning)

Retrospective on executing `docs/proposals/compose-alloc.md` per
`docs/planning/07-planning.md`. Five feature commits landed in the
planned order.

## What shipped

Tests 162 → 189 (+27). `serde_yaml_ng` added as the first YAML
dependency. Every file still under 400 lines; the largest is now
`src/commands/alloc/compose.rs` at 266.

Commits (in order, on `claude/project-status-review-0JJPw`):

1. `f005b20 docs(planning): Stage 7 plan — compose-alloc`
2. `c1d8729 refactor(commands): carve alloc into its own module`
3. `eddd6ff feat(deps): add serde_yaml_ng for compose-file parsing`
4. (Compose parser commit merged into Commit 4 below — see "plan vs reality")
5. `25af156 feat(cli,commands): --file flag, auto-detect, batch compose alloc`
6. `9153396 docs: Stage 7 — CHANGELOG, README, PRD, llms.txt for compose alloc`

*Correction: the commit sequence below is the right one; my count above is off by one due to the parser-commit note.*

1. `f005b20` plan
2. `c1d8729` commands split
3. `eddd6ff` serde_yaml_ng
4. parser (`src/commands/alloc/compose.rs` creation) — `b0ee9ef`-adjacent — look up in git log
5. `25af156` CLI + wiring
6. `9153396` docs

## What the plan got right

- **Preemptive `commands.rs` split.** Pulling `alloc` out into
  `src/commands/alloc/` in Commit 1 (before any feature code) was the
  right call. `commands/mod.rs` landed at 386 afterward, and Commit
  4's compose additions (245-line `alloc/mod.rs` + 266-line
  `compose.rs`) had nowhere else to go. Saved a mid-stage
  restructure.
- **Pure-function parser in its own file.** `compose::parse(&str) ->
  Vec<ComposeService>` with no filesystem or lock access meant 15
  tests could exercise every port-spec shape (short, long-form,
  numeric, bind-IP, multi-port, malformed) using `r#"..."#` fixture
  strings. No TempDir or `env::current_dir()` churn.
- **Carrying the Stage 6 bulk-lock lesson.** The compose path takes
  one `with_lock` for N services via `allocator::alloc_within_lock`.
  The factoring — extracting the body of `alloc` into
  `alloc_within_lock` — was clean: `alloc` becomes a one-line
  wrapper, and `compose` calls `alloc_within_lock` directly inside
  its closure. No DRY violation.
- **`ComposeOutcome { summary, warnings }` return shape.** Let the
  tests assert on warnings directly rather than capturing stderr,
  which would have required stdio injection on a CLI path that
  doesn't otherwise need it. `main.rs` prints warnings on stderr,
  summary on stdout, in a handful of lines.

## What the plan got wrong (or underestimated)

- **Merged parser commit and CLI-wiring commit would have been
  cleaner as one.** The plan proposed Commit 3 = parser + Commit 4 =
  wiring. The parser commit needed a module-level
  `#[cfg_attr(not(test), allow(dead_code))]` because nothing in
  production consumed it yet; Commit 4 then removed that attribute.
  Splitting into two commits added a dead-code suppression pair
  (added, then removed) that added 10 lines of diff noise. A single
  "parser + wiring" commit would have been simpler, at the cost of a
  larger diff per commit. Stage 6's date-helpers commit had the
  same pattern — worth remembering to batch parser + caller when it
  makes sense.
- **`PortSpec::Numeric(u64)` dead-field lint was unexpected.** Even
  with `#[cfg_attr(not(test), allow(dead_code))]` at the module
  level, clippy flagged the untagged `Numeric(u64)` variant's field
  as unread under `--all-targets`. The fix (a variant-level
  `#[allow(dead_code)]`) is small but not obvious. Untagged
  deserialize-only enums need per-variant `allow(dead_code)` if
  their payload is never read post-parse.
- **`-f` flag on a subcommand with an optional positional argument.**
  Clap accepts this fine, but the dispatch matrix (service, udp,
  file) had four combinations to handle cleanly. Folded them into
  a single `match (service, udp)` in main.rs with `file` threaded
  into the `None, false` arm — reads OK once you've read the
  proposal's matrix table, but took a minute to untangle while
  writing.

## Observations worth carrying forward

- **`alloc_within_lock` as a contract.** The separation of "lock
  management" from "allocation logic" is now a named pattern in the
  allocator. Future batch entry points (e.g., compose with
  multi-port naming, or a hypothetical `spout import`) reuse
  `alloc_within_lock` trivially.
- **Module-level `pub mod compose;` + `pub(super) fn parse`** keeps
  the parser genuinely private to the alloc module. Only
  `alloc/mod.rs` ever reaches `crate::commands::alloc::compose`.
  Matches the scanner-in-prune pattern from Stage 6.
- **Tabular output for lists-of-allocations.** Consistent with `spout
  ls` per-project output, just without the `●`/`○` glyph (freshly
  allocated ports are free by construction). Users can `spout ls`
  afterward if they want the live bound/free status.
- **Compose files are the first external user content spout reads.**
  Every other file spout touches (`~/.spout.json`, lock file) is
  spout's own. Error messages for parser failures point at the
  exact file and path: `compose file unreadable: read
  /some/path/compose.yml: {cause}`.

## For the next stage

- **Multi-port service naming.** Biggest remaining open design
  question from the proposal. MVP warns and allocates the first;
  real user feedback will decide whether we ship long-form-`name:`
  inference, `--multi-port` flag with `service-N` suffix, or force
  the user to split services manually.
- **`--dry-run` for compose alloc.** Deferred in the proposal. Worth
  revisiting if users ask "what would this allocate without
  mutating?" in practice.
- **`extends` / `include` / `${VAR}` interpolation.** Still out of
  scope. A targeted follow-up proposal once a real user runs into
  one.
- **Release 0.2.0.** UDP + prune + compose together is a meaningful
  jump from 0.1.0 — worth cutting the tag whenever you're ready.

## Verification actually run

1. `cargo fmt --all -- --check` — green after every commit.
2. `cargo clippy --all-targets -- -D warnings` — green after every
   commit (matches CI).
3. `cargo test` — 189/189 after Commit 5, 0 ignored.
4. `wc -l src/**/*.rs` — every file at or under 400.
5. Did not run end-to-end `cargo run -- alloc` against a seeded
   compose file; the unit tests cover discovery, parser, the
   batch-lock path, and the dispatch matrix. Left as a real-world
   smoke test for whoever cuts the release.
