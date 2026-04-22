# `spout prune` — clean up stale port registrations

_Status: proposal, 2026-04-22. Not committed to a stage yet._

## Context

Spout leases are permanent by design — "your port stays yours until you
explicitly release it." Over time a developer's registry accumulates
registrations from projects they've deleted, prototypes they've abandoned,
and experiments they've moved past. The cleanup path today is manual:
`spout rm <service>` for each one, which requires remembering what's there.

PRD §18 reserves `spout prune` for "stale-entry audit with optional
removal." This doc works through how to build it without violating
spout's design commitments.

## Constraints

- **No daemon.** Permanent design commitment.
- **`get` stays read-only.** Rules out "bump a last-accessed timestamp
  on every lookup" — that would mutate the registry on a read path and
  serialise every `make dev` invocation on a file lock.
- **Offline by default.** `git ls-remote` is slow, flaky over bad links,
  rate-limited, and fails on private repos without auth. Not viable as
  a default signal.
- **Human-in-the-loop.** Staleness is fuzzy without real usage tracking;
  the tool surfaces candidates, the human decides.

## Signals available without a daemon

| Signal                  | Source                                           | Strength for "stale" |
|-------------------------|--------------------------------------------------|----------------------|
| Age                     | `Entry.allocated` (ISO date already in registry) | Moderate — old ≠ unused |
| Currently bound         | `probe_bound_ports` (already used by `ls`)       | Weak — services cycle |
| Project path missing    | `Path::exists()` on absolute-path identities     | **Strong** — truly orphaned |
| Git remote resolves     | `git ls-remote <url>` over the network           | Strong but slow/flaky |

**`allocated` age** and **path-existence** are both cheap and
deterministic. Proposal uses them as the default. Remote resolution
would be a future opt-in flag.

Path-existence only applies to identities that look like absolute
filesystem paths (leading `/` on Unix). Git-remote-style identities
like `github.com/acme/foo` cannot be reverse-mapped — no way to find
the clone on disk without a heuristic scan of `$HOME`.

## Proposed CLI shape

```
spout prune                           # interactive per-entry prompts (safe default)
spout prune --dry-run                 # surface candidates only; no changes
spout prune --yes                     # bulk remove without prompts
spout prune --older-than <DAYS>       # tune age cutoff (default: 90)
```

`--dry-run` and `--yes` are mutually exclusive — `--dry-run --yes`
exits with a usage error.

**Candidate definition (default):** a registration is a candidate if
`allocated` > 90 days ago, **or** its identity is an absolute path
that no longer exists on disk.

**With `--dry-run`:**

```
$ spout prune --dry-run
Stale candidates:

  github.com/acme/old-project
    ○ postgres        21000  allocated 2025-03-15  (403d)
    ○ redis           21001  allocated 2025-03-15  (403d)

  /Users/pete/tmp/spike   [path missing]
    ○ clickhouse      21040  allocated 2025-09-20  (214d)

3 candidates across 2 projects.
Rerun `spout prune` to remove interactively, or `spout prune --yes` to skip prompts.
```

Nothing stale:

```
$ spout prune --dry-run
Nothing to prune (all registrations < 90d, all project paths present).
```

**Default (interactive stdin):**

```
$ spout prune
Remove github.com/acme/old-project/postgres?
  allocated 2025-03-15 (403d ago, free ○)
  [y/N/q/!] y
  removed.
Remove github.com/acme/old-project/redis?
  allocated 2025-03-15 (403d ago, free ○)
  [y/N/q/!] !
  removed.
  removed /Users/pete/tmp/spike/clickhouse.

Done: 3 removed, 0 kept.
```

`y`=yes, `N`=no (default on bare Enter), `q`=quit immediately, `!`=yes
to all remaining.

If there are no candidates, `spout prune` prints the same "nothing to
prune" message as `--dry-run` and exits 0 without prompting.

**With `--yes`:** prints each removal and proceeds without stopping.

## History preservation

Existing `rm` writes `reason: "user requested"` into `history`.
`spout prune` uses richer reason strings so `spout whois <port> --history`
stays useful:

- `"pruned: stale (older than 90d)"` — age-triggered
- `"pruned: project path missing"` — path-triggered

No schema change — just different strings in the existing `reason`
field.

## Design decisions still open

1. **Stdin prompts vs navigable TUI.** PRD §18 and CODING_GUIDELINES
   §UI both imply Ratatui. Stdin is ~150 lines, works over SSH and in
   CI. Selectable TUI is ~200–300 lines and needs `tui.rs` split into
   a submodule per the guidelines' overflow clause. Default recommendation:
   **stdin first**, revisit TUI if the UX feels cramped. Matches the
   "one logical change per commit" instinct — ship the core, iterate.

2. **Default age cutoff.** 90 days feels right for this user's rhythm;
   only real use will tell. Always overridable via `--older-than`.

3. **Git-remote resolution as opt-in flag.** `spout prune --check-remotes`
   would attempt `git ls-remote` for git-remote identities. Explicit
   flag so default stays offline. Deferred.

4. **Path detection heuristic.** "Is this identity a local path?"
   Propose: starts with `/` (Unix). Windows is WSL-only today so the
   second case is moot. Simpler than trying to parse every identity.

## Out of scope

- Continuous background tracking of "last used." Daemon territory.
- Network probes by default. See constraints.
- Bumping a last-accessed timestamp on `spout get`. Breaks the
  read-only guarantee.
- Automatic/cron-driven pruning. Explicit invocation only — spout
  doesn't surprise you.

## Verification path (for the implementation stage, when we get there)

1. Seed a test registry with mixed ages: fresh, 95d, 200d, plus an
   absolute-path identity pointing at a deleted directory.
2. `spout prune --dry-run` lists only the 95d, 200d, and missing-path
   entries.
3. `spout prune --dry-run --older-than 365` lists only the 200d +
   missing-path.
4. `spout prune --yes` removes all candidates; re-running says
   "nothing to prune." History contains entries with the right
   `reason` strings.
5. `echo -e "y\nn\nq" | spout prune` removes the first, keeps
   the second, quits on the third without touching remaining
   candidates.

## Next step

Decide on the open questions above — especially (1), which drives
scope more than the others. Then move to an implementation plan.
