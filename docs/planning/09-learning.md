# Stage 9 — compose override file support (learning)

Retrospective on three commits (plus a simplify-review polish) that
closed the faithfulness gap in `spout alloc` compose-mode: today's real
projects split service declarations from port declarations across
`docker-compose.yml` and `docker-compose.override.yml`, and the existing
scan only read the first.

## What shipped

Tests 199 → 226 (+27, including the merge with PR #10's multi-port
work). All files under 400 lines.

Commits 1 and 2 of the original plan (`257bf1b`, `d4a4b21`) reached
`main` directly. The remaining commits — multi-`-f`, simplify polish,
docs, this retrospective — were pushed to `stage-9-compose-overrides`
and rebased onto post-#10 `main` after parallel work landed there;
the final shipped commit hashes are the post-rebase ones, not the
hashes referenced inside this document.

## What the plan got right

- **Four-name `OVERRIDE_COMPOSE_FILENAMES` mirroring the existing
  `COMPOSE_FILENAMES`.** Extensions match independently (a `.yml` base
  pairs with a `.yaml` override if that's what exists). Keeps the
  discovery logic two `find_existing` calls.
- **Override-wins merge rather than docker's append-and-dedup.**
  spout consumes only the first port + protocol per service, so the
  two rules produce identical observable behaviour for every real
  project. Saved a lot of corner-case tests.
- **Friendly error for "override without base".** `ComposeNotFound`
  now carries a context string so the error can differentiate
  "nothing found at all" from "found override but no base, pass
  `-f`." Kept the exit code at 8.
- **Single shape for auto-detect and explicit chains.** Commit 2's
  simplify pass collapsed `ComposeFiles { base, overlay }` into a
  plain `Vec<PathBuf>`. Eliminated a branch and made `display_files`
  / `format_compose_summary` take a slice naturally.

## What the plan got wrong

- **Predicted keeping `ComposeFiles` through commit 2.** Commit 1
  introduced `ComposeFiles { base, overlay }` because the auto-detect
  shape was "one base plus optional overlay." Commit 2's multi-`-f`
  input blew that assumption up — explicit chains can be any length.
  The plan said "keep `ComposeFiles` and branch the two paths inside
  `compose()`." In practice that would have meant two parallel code
  paths for file collection, two `display_files` implementations, and
  a struct that only covered one of them. Vec<PathBuf> unified them.
  Self-correction happened inside commit 2's implementation rather
  than a separate refactor.
- **Predicted `discover_compose` surviving.** Became
  `resolve_compose_files` when the Vec shape landed — the name
  "discover" implied autodetect-only, but the function now also
  validates an explicit chain.
- **Underweighted the test-duplication cost.** The tests.rs file went
  from 15 tests at the top of stage 9 to 18 tests by the end of
  commit 2. One test (`load_chain_single_file_reads_it`) duplicated
  what the other `load_chain_*` tests already covered. Simplify
  review caught and removed it.

## Unused in the plan

- **Multi-file footgun warnings on chain length.** The plan hedged
  about "what if the chain is 10 files deep?" — in practice no
  real-world project has tried more than 3, and the BTreeMap merge
  cost is negligible for <20 services per file.
- **TOCTOU discussion for `is_file()` validation.** The efficiency
  review flagged it but concluded the check is load-bearing for
  PRD exit-code semantics (surface `ComposeNotFound` rather than
  a later `ComposeInvalid` when the file genuinely doesn't exist).
  Kept as-is.

## What we didn't see coming

A separate Claude session (PR #10, branch
`claude/fix-spout-docker-compose-qe00h`) shipped multi-port
registration to `main` while this branch was mid-flight. Both touched
`ComposeService` and `compose.rs`: ours added override discovery and
the `merge_services` helper, theirs reshaped `ComposeService` to carry
every declared port and dropped the `extra_ports` warning we'd built
around. The merge was mechanical, not semantic — both features compose
cleanly:

- Override-wins still applies per service; the per-service payload is
  now `Vec<ComposePort>` rather than single port + extra-counter.
- `parse` returns `(Vec<ComposeService>, Vec<String>)` for parse-level
  warnings; our `load_chain` adopted the tuple via `try_fold` and
  threads warnings through to `ComposeOutcome`.
- Their multi-port allocation (suffix extras with `-{container_port}`)
  now runs against the merged service set, so override files declaring
  a service's ports correctly produce one allocation per port.

Lesson: there's no in-band signal between concurrent agent sessions on
the same repo. Spout itself exists to stop two Claudes brute-forcing
the same ports — but two Claudes can still brute-force the same
codebase. For a multi-day stage, scan `git log origin/main` before each
session start; for a feature-branch PR, expect the merge base to drift
under you and plan for a rebase pass before merge rather than treating
push as the finish line.

## Learnings for future stages

- **Collapse wider shapes when the second case lands.** Stage 9
  showed that introducing a struct for a 2-case variant (base +
  optional overlay) paints you into a corner when a third case
  arrives (explicit chain). Default to slices/Vec when the case
  count is open-ended from the start. Structs for variant
  discrimination are only worth it when the variants are finite
  and semantically distinct (like `RmTarget` in stage 8).
- **Simplify review catches planning-era assumptions.** The plan
  doc was written with commit-1's shape in mind. Commit 2 revealed
  the shape had to generalize. Letting simplify run as a separate
  polish commit (rather than amending) keeps the narrative clean
  for anyone reading the history — "commit 2 shipped what the plan
  said, simplify pass cleaned it up with hindsight."
- **Dropping unnecessary tests is fine.** The 17-new-tests number
  would have been 18 without the simplify pass. Quantity isn't
  the goal; coverage is. If two tests exercise the same path via
  different inputs, the second adds no information.

## Shape of final commits

Stage 9 landed in 5 commits (plan + 3 feature + 1 refactor + docs).
The refactor commit isolated the simplify-pass cleanups cleanly
rather than folding them into commit 2 — matches the prior pattern
from stage 1's `refactor: simplify-review fix for realloc` commit.
