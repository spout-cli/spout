# Stage 6 — `spout prune` (learning)

Retrospective on executing `docs/proposals/prune-command.md` per
`docs/planning/06-planning.md`. Six feature commits landed in order;
one small post-stage cleanup followed.

## What shipped

Tests 138 → 161 (+23). Every file still under the 400-line cap, with
`src/commands/prune/mod.rs` at 309 and `src/commands/prune/scanner.rs`
at 181 — the second-file extraction from Commit 5 gave comfortable
headroom. CI's `cargo clippy --all-targets -- -D warnings` matches
what I run locally now.

Commits (in order, on `claude/project-status-review-0JJPw`):

1. `f5b3ec4 docs(planning): Stage 6 plan — spout prune`
2. `da5ef77 refactor(registry): split into registry/mod.rs + registry/io.rs`
3. `2c19423 feat(date): parse_iso_date, days_between, days_ago helpers`
4. `342ab8e feat(cli,commands): spout prune --dry-run scanner`
5. `74f1f4c feat(commands): prune interactive confirmation via stdin`
6. `43c3edb feat(commands): prune --yes bulk removal, split scanner into its own module`
7. `746b34d docs: Stage 6 — CHANGELOG, README, PRD, llms.txt for spout prune`
8. `d942845 chore(date): drop dead-code suppression now that prune uses the helpers`

## What the plan got right

- **Registry split up front.** Stage 5 ended with `registry.rs` at
  400/400. Commit 1 broke it into `registry/mod.rs` (schema + methods,
  195) and `registry/io.rs` (file I/O, 222). `pub use io::*` kept
  every caller's imports untouched — 138 tests passed on the first
  build post-rename. No downstream edits required.
- **`#[serde(rename_all = "lowercase")]` is well-tested.** The Stage
  5 pattern (serde-default migration) was transparent here; Stage 6
  added no new schema.
- **Stdin-first interactive mode.** `&mut impl BufRead` +
  `&mut impl Write` makes the loop trivial to test with `&[u8]` and
  `Vec<u8>`. The four responses (`y/N/q/!`) shook out cleanly with
  four focused tests. First interactive command spout has shipped.
- **Rich reason strings.** `apply_remove` computes
  `"pruned: stale (older than Nd)"` or `"pruned: project path missing"`
  from `StaleReason` + cutoff, and `registry::Registry::remove` took
  it as a `&str` with no signature change. `spout whois <port>
  --history` keeps the pruning narrative intact for anyone
  retrieving old ports later.

## What the plan got wrong (or underestimated)

- **`commands/prune.rs` would blow the cap earlier than expected.**
  The plan said "split during Commit 4 if needed." Reality: Commit 4
  landed at 396/400, dead on the cap. Commit 5 (bulk + tests) needed
  the second split one commit earlier than expected. Outcome was the
  same `commands/prune/{mod,scanner}.rs` layout, just reached via
  explicit split mid-stage rather than drift. Next time, plan the
  scanner extraction from Commit 3.
- **`civil_from_days` needed to become `pub(crate)`.** Not
  catastrophic — one attribute change in `date.rs` — but the plan
  treated the new date helpers as self-contained. The test helper in
  `prune::scanner::tests::iso_days_ago` ended up wanting the reverse
  direction, and inlining Hinnant's algorithm a second time felt
  worse than widening visibility. Worth remembering: if a pure
  function would be convenient to call from sibling tests, the
  friction savings usually outweigh the API-surface concern.
- **Commit 2 shipped dead code for one commit.** The helpers landed
  ahead of their first production caller, tripping
  `-D warnings -D dead_code`. Solved with a module-level
  `#[cfg_attr(not(test), allow(dead_code))]`, then removed in the
  post-stage `d942845` cleanup. The alternative — merging Commits 2
  and 3 — would have made the diff harder to review. Net: worth the
  small process wart.

## Observations worth carrying forward

- **Prune's `apply_remove` takes `older_than` so it can format the
  reason.** The scanner already knows the actual age, but the
  history message uses the cutoff (what the user asked for), which
  is more stable information. "The entry was 141d old" is noisier
  than "the entry was older than the 90d threshold." Design choice
  I didn't capture in the proposal; worth noting.
- **`[y/N/q/!]` converges to the docker prune UX.** The "yes to all"
  bang is borrowed from `debconf` / `apt` prompts. Users who have
  pruned anything before pattern-match on it immediately.
- **TOCTOU on path-existence is acceptable.** A project directory
  could be created or deleted between the scan and the `apply_remove`
  confirmation. Spout doesn't re-check; if the user confirms `y`
  they mean it. Same philosophy as the allocator's bind-race.
- **The `DEFAULT_OLDER_THAN` const is in `scanner.rs` for the
  dry-run format; the `--older-than` CLI default lives in `cli.rs`
  via `default_value_t = 90`.** These stay in sync by convention. If
  the default ever changes, both need updating. A `pub(crate) const
  DEFAULT_OLDER_THAN` shared between them would be cleaner, but
  adds cross-module coupling for one number.

## For the next stage

- **Stage 4's planning-doc gap is still unresolved.** Stages 5 and 6
  both followed the planning + learning discipline. Backfilling
  Stage 4 (or formally retiring the stage concept) is overdue.
- **`spout realloc <service>`** is the smallest PRD §18 item left —
  would be a half-day stage. `spout alloc --from-compose` is still
  deferred per the user.
- **`--check-remotes` for prune** is the only flag the proposal
  deferred. It adds a network-probe code path to prune; non-trivial
  because `git ls-remote` can fail for reasons other than "remote
  gone" (auth, rate limit, flaky net). Worth a short proposal addendum
  before picking up.
- **Release 0.2.0 is sensible now.** UDP (Stage 5) and prune (Stage 6)
  together are a meaningful jump from 0.1.0. Tagging needs a push
  from an authenticated terminal — same blocker as 0.1.0 from the
  deep-dive pass.

## Verification actually run

1. `cargo fmt --all -- --check` — green after every commit.
2. `cargo clippy --all-targets -- -D warnings` — green after every
   commit (matches the CI invocation, which `-- -D warnings` alone
   does not).
3. `cargo test` — 161/161 after the final commit, 0 ignored.
4. `wc -l src/**/*.rs src/*.rs` — every file at or under 400.
5. Did not run `echo -e "y\nn\nq" | spout prune` end-to-end against
   a real seeded registry; the unit test with `&[u8]` stdin covers
   the same code paths deterministically.
