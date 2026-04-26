# Stage 10 — surface recently-removed in not-found errors (learning)

Retrospective on a two-implementation-commit stage that closed the
last loop in agent-safety for `spout get` failures: when a service
was just removed in this project, the failure message now surfaces
that fact so an agent can pause before re-allocating.

## What shipped

Tests 230 → 237 (+7). All files under 400 lines.

- `247eeed` `feat(registry,error): surface recently-removed in not-found errors`
- `f3ffba9` `docs: surface recently-removed in not-found error messages`
- this learning doc + index update

Two impl/docs commits rather than the planned three — the docs
commit absorbed both the user-facing surfaces and the agent-facing
ones, no need to split.

## What the plan got right

- **`Option<RemovedRecord>` as the new field shape.** Carrying just
  the most recent record (rather than a `Vec`) kept the format
  branching tractable: four cases (available × removed) instead of
  N. The agent reads the date and judges; older records are still
  reachable via `whois --history` if needed.
- **`RemovedRecord` lives in `error.rs`, not as a re-export.** The
  primitive is two `String`s. Keeping it independent of
  `registry::HistoryEntry` means `error.rs` still doesn't depend on
  registry types — the mapping happens at the call site in
  `commands/mod.rs`. Worth the eight-line struct definition.
- **Format helper branches over `(available.is_empty(),
  recently_removed.is_some())`.** The `match` cleanly enumerates the
  four cases. Empty + removed is the only case that promotes the
  hint ("register fresh"); everything else uses the existing hints.
  Easy to read, easy to extend.
- **`history_for_service` mirrored `history_for_port`.** Same shape,
  same sort, same comment style. Consistency at the registry layer.

## What the plan got wrong

- **Predicted three implementation commits.** The plan budgeted one
  commit for the registry method + variant + tests, one for docs,
  one for the learning doc. In practice the registry method, variant
  extension, and tests all naturally landed together — splitting
  them would have produced no-op-against-tests intermediate states.
  Two commits, not three, was honest about the shape.
- **Underestimated test wording fragility.** The new format helper
  changed "no services registered" to "no services currently
  registered" (clearer with the new layout). One existing test was
  asserting on the old substring; minor breakage caught immediately
  by `cargo test`. Plan should have flagged that wording shifts in
  format helpers tend to break loose `contains()` assertions.

## Unused in the plan

- **`HistoryEntry::released` ISO-8601 sort risk.** The plan flagged
  it as risk #2 — would future writers preserve the format? The
  registry already routes every `released` write through `today_iso()`
  (one writer, one format), and the comment on `history_for_service`
  documents the assumption. No further work needed.
- **Format readability across all four combos.** Risk #1. End-to-end
  smoke test of all four cases (empty/populated × no-removal/has-
  removal) read cleanly on the first try. The hint adaptation ("try
  `spout alloc X` to register fresh" vs. plain alloc vs. env)
  carried the weight of the layout difference.

## What we didn't see coming

- **The wording shift broke a test on a slightly different
  assertion than the plan anticipated.** Plan worried about format
  helper risks ("verify visually with all combos"); the actual
  failure was a test using `contains("no services registered")`
  before the helper added the word "currently." Lesson: when the
  format helper is the source of truth for user-facing wording,
  loose substring assertions in tests should target the most
  specific stable token (the verb, the variable name) rather than
  the surrounding phrase.
- **The Display attribute syntax for nested fields tripped slightly
  on the first attempt.** thiserror's `.field` shorthand worked, but
  needed `.recently_removed.as_ref()` to convert `&Option<T>` to
  `Option<&T>` for the helper signature. Not a bug, just a syntax
  detail worth noting for future variant extensions.

## Learnings for future stages

- **When extending an error variant, design the helper signature
  first.** Decide which fields the format helper takes by reference
  vs. value, and shape the variant fields to match. Saves a round
  of borrow-checker arguments after the fact.
- **Battle-tested polish deserves the stage shape.** This work
  could have shipped as a single conventional commit ("close the
  loop on user-just-removed in get failures"). The user explicitly
  elevated it to stage status because the design was non-trivial
  and the rationale was worth recording. The decisions-locked
  section earned its keep — the "no time cutoff" call could easily
  have flipped the other way mid-implementation without it.
- **The "facts only, no scolding" pattern keeps scaling.** Stage 9.5
  established it; stage 10 extended it to history surfacing. The
  message reports `recently removed: api (date, "reason")` and
  trusts the agent to decide. Resist the temptation to add advisory
  language ("consider asking the user before re-allocating") in the
  message itself — the date is the signal; agents read context.

## Shape of final commits

Stage 10 landed in 4 commits (plan + 1 feature + 1 docs +
learning). Cleaner than stage 9's five-commit shape because there
was no parallel-Claude collision to rebase around and no simplify
pass needed — the variant extension was small enough that the
review-after-implementation step folded into the same commit.
