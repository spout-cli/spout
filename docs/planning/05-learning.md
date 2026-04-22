# Stage 5 — UDP support (learning)

Retrospective on executing `docs/proposals/udp-support.md` per
`docs/planning/05-planning.md`. Six commits landed as planned; no
stage splits, no reorderings.

## What shipped

Tests 117 → 138 (+21). Every file still under the 400-line cap.
Release still tagged `[Unreleased]` in the CHANGELOG pending the next
version bump. The v0.1.0 tag from the prior session remains local-only
on my machine, awaiting an authenticated push.

Commits (in order, on `claude/project-status-review-0JJPw`):

1. `8cd8f95 feat(registry): Protocol enum, bump schema to v2`
2. `939d08d feat(allocator): UDP OS probe, protocol-dispatched is_port_free_on_os`
3. `743ceb1 feat(registry): protocol-aware is_port_claimed`
4. `76c893d feat(commands,cli): --udp on alloc/set/check, multi-protocol whois`
5. `520412d feat(format,tui): protocol visible on ls rows in both renderers`
6. `604e9b0 docs: Stage 5 — CHANGELOG, README, PRD, llms.txt for UDP`

## What the plan got right

- **Schema v2 with serde-default migration worked exactly as
  proposed.** v1 files read transparently into v2 structs; missing
  `protocol` defaults to Tcp; next mutating write persists v2; old
  binaries error loud on v2 per the existing exact-match version
  check. Zero data-migration code.
- **The service-name-unique rule stayed right.** `spout get
  <service>` is still a one-line HashMap lookup, `spout env` still
  derives env-var names from the service name alone. Users who want
  both TCP and UDP on the same logical name register two services.
  It's the simplest thing that could work.
- **Test-first was the right discipline for the migration.** The
  `v1_file_upgrades_to_v2_on_first_mutating_write` test caught a bug
  on first run — `write()` serialises whatever `Registry.version`
  holds, not `CURRENT_VERSION`, so the upgrade had to be forced in
  `with_lock` right after read. Without the test, that would have
  been a silent landmine.

## What the plan got wrong

- **`registry.rs` line budget was too tight.** Plan said "+10 lines,
  tight at 395". Actual growth was ~15 lines even after extracting
  Protocol to `src/protocol.rs`. Landed at exactly 400 by pruning
  two docstrings. If the next commit in this file adds anything at
  all it needs to displace something else — worth flagging as
  technical debt. The schema module is structurally dense and
  probably ready for a real split (`registry/schema.rs` +
  `registry/io.rs`) before Stage 6.
- **The CHANGELOG entry I wrote for 0.1.0 last session was wrong.**
  Claimed the TUI ENV VAR column was "replaced with PROJECT." Code
  reality: ENV VAR is still a column, PROJECT is a section header
  above each table, not a column. Didn't catch this until Stage 5
  Commit 5 when I went to add the PROTO column and discovered the
  shape didn't match the doc. Left the 0.1.0 CHANGELOG untouched
  (too late to rewrite a cut release) but made sure Stage 5's
  additions describe the actual columns. Worth a docs cleanup pass.
- **Estimated +40 lines on allocator; actual +~30.** Close enough,
  but consistently under-estimated. No real consequence.

## Observations worth carrying forward

- **Protocol in its own module paid off immediately.** `src/protocol.rs`
  hosts the enum, its Display/Ord impls, and the schema-integration
  tests for how Protocol rides inside Registry. The v1-reads-as-tcp
  tests live there rather than in `registry.rs` specifically because
  `registry.rs` was over-cap. That co-location also reads naturally
  — "tests about how Protocol behaves in the schema" fit the protocol
  module.
- **`r.version = CURRENT_VERSION` inside `with_lock`** is a single-
  point-of-migration pattern I want to remember. Every future schema
  bump can reuse it: widen `read()`'s accepted versions, add
  `#[serde(default)]` to new fields, and the next mutating write
  upgrades the file automatically. No explicit migration step, no
  version-jump tables.
- **`whois` as always-multi-protocol is clearly right.** I noticed
  while writing the commands.rs test that "what's on port 5432?" is
  the most common whois use case, and filtering by protocol would
  defeat the point. A dedicated `whois 5432/udp` syntax was briefly
  tempting but the proposal's choice (just list everything) reads
  cleaner and matches how `grep` works.
- **Asymmetry between plain-text and TUI is acceptable when the
  audiences differ.** Plain-text `ls` suffixes the port as
  `20000/tcp` inline (grep-friendly); TUI has a dedicated PROTO
  column (eyeballs-friendly). Both convey the same info; both are
  idiomatic for their medium. Didn't need to unify them.

## For the next stage

- `src/registry.rs` at exactly 400 is a flashing light. Next stage
  that touches it should start by splitting.
- The CHANGELOG ENV-VAR-vs-PROJECT inaccuracy from 0.1.0 deserves a
  correction pass. Not urgent — the release is tagged, and the
  active documentation (§9 of the PRD, the PROTO column now alongside
  ENV VAR) is accurate.
- Stage 6 candidates, in approximate order of leverage:
  1. `spout prune` per `docs/proposals/prune-command.md`. Design
     is done; one open question (stdin vs Ratatui) to resolve.
  2. `spout alloc` compose-file parsing. User asked to defer.
  3. `spout realloc <service>` — PRD §18 future work, small.
- The "Stage 4 has no planning doc" gap from the prior conversation
  is still unresolved. Stage 5 treated it as not-my-problem and
  numbered from 04 forward. A backfill for Stage 4 (or an explicit
  decision that the stage concept is evolving) is overdue.

## Verification actually run

1. `cargo fmt --all -- --check` → exit 0 after every commit.
2. `cargo clippy -- -D warnings` → exit 0 after every commit.
3. `cargo test` → 138/138 after the final commit, 0 ignored.
4. `wc -l src/*.rs` → largest is `registry.rs` at 400, at the cap.
5. Did not run the end-to-end `nc -u -l <port>` probe manually.
   The unit-level `is_port_free_on_os_returns_false_for_bound_udp_port`
   test covers the same code path with a `UdpSocket` bound in the
   test process, so the coverage is equivalent.
