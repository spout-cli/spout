# Stage 1 — Learning

**Stage:** MVP core  
**Written:** 20 April 2026 (after `a0cad53`)  
**Planning doc:** [01-planning.md](01-planning.md) — including the Stage 1.1 addendum at its top

---

## What shipped

All seven commands working end-to-end: `get`, `alloc`, `set`, `rm`, `ls`, `check`, `whois`. ~1,500 lines of Rust across nine modules, no file over 400 lines. 72 tests, `cargo fmt --check` clean, `cargo clippy -D warnings` clean. End-to-end smoke tested against the built binary.

Data model: a per-project map of `{service: {port, allocated}}` entries plus a top-level history array of released registrations. `fd-lock` RwLock for concurrency, atomic writes via tempfile + rename. Project identity layered: `git remote origin` → `git rev-parse --show-toplevel` → absolute CWD, cached per-process via `OnceLock`.

---

## What the plan got wrong

Two foundational design choices in the original `01-planning.md` were load-bearing and turned out to be wrong. Catching them mid-implementation — rather than after shipping — was the difference between a working product and one that fails on day-one adoption.

### 1. Walking from conventional ports

The plan had `services.rs` map well-known names to canonical ports (`postgres → 5432`, `redis → 6379`, etc.) and `alloc` would walk forward from there. This was framed as a DX win: the first postgres project gets 5432, subsequent projects walk to 5433, 5434.

The failure mode only surfaced during a collaborative scenario review: *what happens when a docker container using 5432 is temporarily stopped?* At alloc time, the OS bind-test passes (container isn't holding the port), spout hands out 5432 to a new project, and the stopped container collides on its next startup. That's the exact "two projects fight over 5432" failure spout was built to prevent — happening from inside spout itself.

**Fix:** allocate entirely in `20000–32767`, a range where real software almost never binds. The DX cost ("5436 looks postgres-ish at a glance") got repaid by a new `spout whois <port>` command that turns any mystery port in the wild into one command's worth of answer.

Dropping the default-ports table also simplified `services.rs` from a planned ~50 lines to ~20, and eliminated an ongoing maintenance tax (every new service in the ecosystem would have been a PR against spout).

### 2. Basename project identity

The plan used `basename $PWD` for project name, matching Docker Compose's convention. Silent collision mode: `/work/tyfi` and `/home/personal/tyfi` both register as "tyfi" and one overwrites the other.

**Fix:** layered identity — git remote URL → git root path → CWD path. Git-remote identity survives filesystem moves, clones across machines, and doesn't silently collide. The `git` shell-out is cached via `OnceLock` so the 60–100ms cold cost is paid once per process, not per command.

### 3. Bind-testing in read commands

An intermediate design had `get` bind-test the registered port and fail loud on stale; `alloc` was going to self-heal by detecting stale and reallocating transparently. Both turned out to be wrong: a bound port could mean "our container has it (fine)" OR "something else stole it (broken)", and a bind-test can't distinguish the two. If spout reallocates when its own container is up, the user's workflow gets silently broken.

**Fix:** the registry is the source of truth for ownership. `alloc` is purely idempotent (registered → return existing, no re-check). `get` is a pure registry lookup. Bind-testing only happens when searching for a port nobody has yet (fresh allocation) and in the explicit `spout check` diagnostic. When a port genuinely goes stale, docker-compose fails loud with the OS's "address already in use" and the user runs `spout rm && spout alloc`. Rare, recoverable, doesn't compromise the common case.

---

## What went right

- **TDD caught a date-arithmetic off-by-one in under a minute.** `civil_from_days(20_564)` expected `(2026, 4, 20)`, actually returned `(2026, 4, 21)`. The fix was a test-input tweak (should have been 20563), but without the test the bug would have silently corrupted every `allocated` / `released` date by one day.
- **The 400-line file limit forced a clean split.** `registry.rs` came out at 436 lines with date helpers bundled in. Extracting `date.rs` brought registry to 385 and gave the date helpers their own focused module with its own tests — a better shape than the bundled version would have been.
- **`#![cfg_attr(not(test), allow(dead_code))]` as scaffolding.** Newly-built modules trip `dead_code` warnings because main.rs hasn't wired them up yet. The conditional suppression (apply only in non-test builds — tests already construct everything) let each module pass clippy cleanly during the period between "module exists" and "module has a caller". All suppressions except the one on `services.rs` were lifted during the simplify pass.
- **Hand-rolling over adding deps, where it was a close call.** Hinnant's civil-from-days is ~15 lines of proven integer math; the git remote URL parser is ~30 lines of clear string manipulation. Both avoided pulling in `chrono`/`time` and `url`/`git2`. Both have enough unit tests to trust.
- **Per-module commits at TDD granularity.** One module per commit with its own test suite. Made the simplify passes much easier to review.

---

## Surprises

- **The PRD had more design bugs than the code did.** Implementation pressure surfaces hidden assumptions. Writing tests for concrete scenarios ("what if the container is stopped when alloc runs?") made the problems obvious in minutes, where reviewing the PRD abstractly hadn't in hours.
- **"Keep debugging context" beats "simplify schema today."** I proposed dropping `allocated` from `HistoryEntry` because it meant nesting a struct in the live schema. User pushed back: "an allocated date is useful context if we need to marry it up against logs later." Saved as a project-level preference — worth remembering for future data-modelling decisions.
- **Universal CLI convention is stronger than I credited.** Initially framed "`get` must be read-only" as "mutation bad" reflex. When the user pushed back — "how do other tools handle this?" — a survey of git, docker, kubectl, aws, helm, terraform, brew, and systemctl revealed zero major tools where a `get`-style command mutates under any flag. Convention is load-bearing across the whole ecosystem, not a preference.
- **The session almost derailed mid-stage.** When we surfaced the stopped-container collision risk, it briefly looked like spout was unshippable. The recovery came from reframing — the core value (preventing inter-project collisions via a shared registry) still held; what needed to change was the allocation strategy, not the premise.

---

## Process observations

- **Plans are drafts, not contracts.** Three of the "what the plan got wrong" entries above were caught mid-session by the user, not by me. My early plan review focused on coding-rule compliance (no `expect()`, 400-line limit) rather than product-design assumptions. Next stage: front-load product-design scrutiny *before* module-level execution.
- **The Stage 1.1 addendum at the top of `01-planning.md` did the heavy lift.** The rest of the original plan is still accurate for ~70% of decisions; adding a "supersedes where noted" section at the top was cheaper than rewriting the whole doc. Design-reversal notes at the top of the planning doc is a reusable pattern.
- **Force-pushing after amending the initial commit** left origin diverged from local for most of the session. Noted for next time: either live with the divergence for MVP work, or push early. Not both.
- **Committing per-module was the right granularity.** When the user flagged we should squash `feat(error)` + `refactor: simplify review`, that was a clear signal — one logical change had got split, fix it. Everything else stood.

---

## For next stage (Stage 2)

- **Product-design pass first.** Spend 30 minutes stress-testing the plan's assumptions with concrete scenarios before diving into modules. The failure modes caught in Stage 1 would have been obvious from "what happens during onboarding, when the user has legacy stopped containers?"
- **Compose inference is the top ergonomic win left on the table.** `spout alloc` with no args parsing `docker-compose.yml` to auto-allocate for every declared service. Reduces typing for the common multi-service case without touching the correctness model.
- **Deferred cleanups worth revisiting early.**
  - Extract the triplicated `temp_registry()` test helper (needs a shared test-support module in a binary crate).
  - Encapsulate `Registry` public fields behind accessor methods — `commands.rs` couples to internal structure today.
  - `reason: &str` on `Registry::remove` should be an enum (`RemovalReason::{UserRequested, Reallocated, ...}`) for discoverability and type safety.
- **Carry the collaboration style forward.** User pushback was load-bearing this stage. Keep defaulting to "stop, flag, ask" when a design choice feels off, even if the plan already signed off on it.

---

## Commit trail

```
a0cad53 docs(prd): reconcile spec with Stage 1 implementation
3c5cd18 refactor: simplify pass — cache project identity, O(1) port checks
52690ca feat(cli): wire up all 7 commands end-to-end
b1b141d feat(allocator): idempotent port allocation over 20000-32767
ec77733 refactor(project): layered identity — git remote, git root, CWD
2f0bc4f refactor(registry): extract date helpers to their own module
1d00ac2 feat(registry): JSON registry with locking, atomic writes, history
ce10797 feat(services): env-var naming helper
8b22caa docs(planning): Stage 1.1 design revision
fdce3c9 feat(project): infer project name from current working directory
6d89e1e refactor: trim scaffolding per simplify review
8e1278f feat(error): SpoutError enum with exit code mapping
c575005 chore: scaffold cargo project
93dbc1d initial commit: project scaffolding (amended)
```

The `8e1278f` + `6d89e1e` squash is queued as the last cleanup before push.
