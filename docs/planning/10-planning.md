# Stage 10 — surface recently-removed services in not-found errors

## Goal

Close the last loop in agent-safety for `spout get` / `spout rm` failures.

Stage 9.5 (commit `e14a08a`) made not-found errors enumerate the
project's currently-registered services, which prevents agents from
allocating duplicates after guessing wrong names (`tyfi-postgres` when
the real name is `postgres`). It does **not** help in a different but
adjacent failure mode: the user has just removed `api` and `web`; an
agent in the same project asks `spout get api` a moment later, sees
"no service 'api' in project X (try `spout alloc api`)", and
obediently re-creates the registration the user just deleted.

The fix is to surface the removal as a fact in the failure message:

```
spout: no service 'api' in project 'github.com/petewaters/tyfi'
  available: postgres, redis
  recently removed: api (2026-04-26, "user requested")
  (try `spout env` for KEY=VALUE)
```

The agent reads the date next to the removal record and judges:
seconds-old → ask the user; six-months-old → proceed. Spout reports;
agents decide. No new flag, no new exit code, no new policy.

Battle-tested origin: another Claude session reported sailing through
a `spout get`-then-`spout alloc` sequence after the user had explicitly
removed those services minutes earlier. They self-corrected because
they had conversational context. A fresh agent without that context
would have re-allocated. This stage gives every agent the signal
without relying on session memory.

## Decisions locked

- **Show only the most recent removal record per service.** If `api`
  was alloc/rm cycled three times, listing all three is noise — the
  date of the most recent is the only signal that matters ("was it
  2 minutes ago or 6 months ago?"). Older records stay in `history`,
  reachable via `spout whois <port> --history` if needed.

- **No time cutoff.** Show whatever exists with its date. Defining
  "recent" in code would either drop genuinely useful signal (a
  removal from yesterday filtered as not-recent) or surface noise
  (an entry from last year labelled "recent"). The date is the
  discriminator; the agent reads it.

- **Label the line `recently removed`.** Honest enough — the
  registry only keeps removal history, so every entry shown was at
  some point recent. The date does the work of disambiguating.

- **Facts only, no scolding.** No "or check with the user first"
  wording in the hint. The fact of the removal record is the signal.
  Spout reports; doesn't moralise. (Pattern continues from earlier
  pushback against alloc-time fuzzy "did you mean" warnings.)

- **Cross-project history is invisible.** A removal of `api` in
  project `foo` is not surfaced when `spout get api` fails in
  project `bar`. Service names are project-scoped; history reads
  follow the same scoping.

- **Surfaces in both `get` and `rm` failures.** They share
  `not_registered_in_project`; both get the new behaviour for free.

- **Footer hint stays adaptive.** Empty-project + recent removal
  changes the hint to suggest `spout alloc <service>` to register
  fresh (the natural recovery). Populated project keeps `spout env`
  as the broad-survey hint. No advice that overlaps the available
  list — those are facts and live above the hint.

## Approach

Single-stage, two implementation commits + one docs commit.
TDD throughout; fmt/clippy/test green between commits.

### Commit 1 — `feat(registry,error): add history_for_service + extend not-found error`

`src/registry/mod.rs`:
- New method on `Registry`:
  `history_for_service(&self, project: &str, service: &str) -> Vec<&HistoryEntry>`.
  Filters `self.history` by project + service; sorts most-recent
  first by `released` (string sort works for ISO-8601). Mirrors the
  existing `history_for_port` shape and ordering.

`src/error.rs`:
- New primitive type `RemovedRecord { released: String, reason: String }`
  (independent of registry types so error.rs stays free of
  cross-module deps).
- Extend `SpoutError::ServiceNotRegisteredInProject`:
  ```
  ServiceNotRegisteredInProject {
      project: String,
      service: String,
      available: Vec<String>,
      recently_removed: Option<RemovedRecord>,
  }
  ```
- Update `format_not_registered_help` signature + branching:
  - Lead line unchanged.
  - Empty available → "no services currently registered" line.
  - Populated available → "available: …" line (unchanged).
  - If `recently_removed.is_some()` → emit
    `recently removed: <service> (<released>, "<reason>")`.
  - Footer hint:
    | Available | Removed | Hint |
    |:---------:|:-------:|------|
    | empty     | none    | `(try spout alloc <service>)` (today) |
    | empty     | some    | `(try spout alloc <service> to register fresh)` |
    | populated | none    | `(try spout env for KEY=VALUE)` (today) |
    | populated | some    | `(try spout env for KEY=VALUE)` (unchanged) |

`src/commands/mod.rs`:
- `not_registered_in_project` calls `reg.history_for_service(project, service)`,
  picks `.first()` (most recent), maps to `RemovedRecord`. Populates
  the new field.

Tests (in `commands/mod.rs::tests` + `registry/mod.rs::tests` +
`error.rs::tests`):
- `history_for_service_returns_empty_when_never_removed`
- `history_for_service_filters_by_project_and_service`
- `history_for_service_sorts_most_recent_first`
- `get_failure_includes_recently_removed_when_history_exists`
- `get_failure_picks_most_recent_when_multiple_removals`
- `get_failure_ignores_history_from_other_projects`
- `get_failure_in_empty_project_with_history_uses_alloc_fresh_hint`
- Display test: error message contains the `recently removed:` line
  with date and reason verbatim

### Commit 2 — `docs: surface recently-removed in not-found error messages`

- `CHANGELOG.md` `[Unreleased]` Added — one bullet covering the new
  failure-message line and the rationale (close the loop on
  user-just-removed scenarios).
- `README.md` "For AI agents" — extend the existing "failed lookups
  list actual service names" bullet with a clause about removal
  records.
- `docs/spout-prd.md` §10 Error Handling — extend the
  service-not-registered bullet with one sentence on history
  surfacing.
- `templates/CLAUDE.md` — extend the "If `spout get` exits 1, **read
  the stderr message before doing anything**" paragraph with a third
  outcome: "the error shows a `recently removed:` line — the user
  may have just removed this; ask before re-allocating."
- `llms.txt` — extend the `spout get` block with one short sentence
  on history surfacing.

### Commit 3 — `docs(planning): Stage 10 learning doc`

Standard retrospective. What landed, what surprised us, what we
deferred. Update `docs/planning/README.md` index.

## Test matrix

| Scenario                                      | Expected                              |
|-----------------------------------------------|---------------------------------------|
| Service never existed                         | No `recently removed:` line           |
| Service was removed once                      | One `recently removed:` line, date    |
| Service was removed multiple times            | Most recent only                      |
| Service was removed in another project        | No `recently removed:` line           |
| Service was removed and re-allocated (active) | No error fired — get returns the port |
| Empty project + service has removal history   | Line present + `alloc fresh` hint     |
| Populated project + service has history       | Available list + removed line + env hint |

## Error model

No new error variants. The existing `ServiceNotRegisteredInProject`
variant grows one field. Exit code stays 1. `Display` takes the new
field into account for the body and the footer hint.

The `RemovedRecord` primitive lives in `error.rs` so the variant
doesn't depend on `registry::HistoryEntry`. The mapping happens in
`commands/mod.rs::not_registered_in_project`.

## Not doing

- **Configurable cutoff** (`--recent-window=24h` etc). No use case;
  the date in the message lets the agent judge.
- **Multiple removals listed.** Most-recent only. If a real session
  needs the full picture, `spout whois <port> --history` is the
  documented escape hatch.
- **History surfaced on `spout alloc` success.** The signal belongs
  on the *get* failure, where the decision lives. Surfacing it on
  alloc would be too late and would noise up the success path.
- **History surfaced on `spout set`.** Different decision shape
  (user is being explicit about the port). Out of scope.
- **A new `spout history <service>` subcommand.** The information is
  reachable via `whois`. Adding a new command for one edge case is
  scope creep.
- **Cross-project removal surfacing.** Service names are project-
  scoped; history reads stay project-scoped.

## Critical files

- `src/registry/mod.rs` — new `history_for_service` method
- `src/error.rs` — `RemovedRecord` type + variant extension + format
  helper branches
- `src/commands/mod.rs` — `not_registered_in_project` populates the
  new field
- `CHANGELOG.md`, `README.md`, `docs/spout-prd.md`,
  `templates/CLAUDE.md`, `llms.txt` — docs
- `docs/planning/README.md` — index entry
- `docs/planning/10-learning.md` — retrospective (after merge)

## Risks

1. **Format readability.** Three optional sub-lines under one error
   start risking a wall of text. Verify visually with all combos
   (empty/populated × no-removal/has-removal). If any combo reads
   badly, tighten before merge.

2. **`HistoryEntry::released` sort assumes ISO-8601 dates.** Today's
   `today_iso()` produces them; nothing else writes to the field.
   Add a comment on `history_for_service` noting the assumption so
   future entry-writers don't break the order silently.

3. **`RemovedRecord` is reasonable as a primitive but invites
   creep.** Resist the temptation to add `port` or `protocol` to it
   — the failure message is about service identity, not port
   forensics. If those become useful, `whois --history` is the right
   surface.

4. **Test isolation around `today_iso()`.** Tests that allocate then
   remove run within the same calendar day, so `released` strings
   collide. Use the `Registry::set` + `Registry::remove` pair with
   pre-baked `released` strings via direct `history` push when test
   cases need cross-day differentiation.
