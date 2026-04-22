# spout — Product Requirements Document

**Status:** Draft  
**Version:** 0.2  
**Last Updated:** 20 April 2026

---

## 1. Problem

When running multiple Docker Compose projects locally, services fight over ports. Postgres defaults to 5432, Redis to 6379, APIs to 8080. If another project already has it, you get "port already in use." The current workflow is:

1. Try the default port
2. Fail
3. Try the next one
4. Fail
5. Eventually find a free one
6. Forget to update the `.env` file
7. Everything breaks again next session

AI coding agents make this significantly worse — they brute-force through ports, kill processes they shouldn't, and waste time and money on what should be a solved problem.

**No existing tool solves this cleanly.** PortHub (the closest competitor) requires a background daemon, a web UI, TTL-based leases with heartbeats, and a REST API. It's trying to be DHCP for developers. port-whisperer is a read-only inspector with no registry. The gap — permanent named leases, no daemon, file-based, agent-friendly — is real and unoccupied.

---

## 2. Solution

A Rust CLI called `spout` that maintains a JSON registry of which projects own which ports. Any tool — human, script, or AI agent — can query it to get a conflict-free port instantly.

```bash
# Get a registered port (READ ONLY — never mutates)
$ spout get postgres
20000

# Allocate a new port (MUTATES REGISTRY — explicit, intentional)
$ spout alloc postgres
20000

# Reverse lookup — what owns a port? (READ ONLY)
$ spout whois 20000
20000: github.com/spout-cli/spout/postgres  (active, allocated 2026-04-20)
```

The registry is a single JSON file. No daemon. No service. No background process. Readable by anything.

---

## 3. Design Decisions

### 3.1 Project Identity

**spout derives project identity in layered fallbacks, first match wins.** The goal is a stable identifier that survives filesystem moves and doesn't silently collide with same-basename repos in different directories.

1. **`git config --get remote.origin.url`** — parsed to `host/owner/repo`. Stable across clones, moves, and machines. Primary identity for the ~95% case of a user developing against a remote-tracked git repo.
2. **`git rev-parse --show-toplevel`** — the git root's absolute path. Used when the repo has no remote configured (fresh `git init`, local-only experiments).
3. **Absolute CWD** — fallback when there's no git repo at all, or when `git` isn't installed.

```bash
# In any subdir of a git-tracked project
spout get postgres   # looks up "github.com/spout-cli/spout" + "postgres"
spout alloc postgres # same identity regardless of cwd within the repo
```

Identity is cached per-process via a `OnceLock`, so the `git` shell-outs run at most once per invocation even when multiple command handlers would touch `current_project()`.

**Rationale vs basename:** The original design used `basename $PWD`, matching Docker Compose's convention. That silently collides when two repos share a basename across different parent directories (`/work/myapp` and `/home/personal/myapp` both registered as "myapp"). Git-remote identity is stable across filesystem moves and unique across the whole host. Basename is still effectively the identity for non-git directories, via the CWD-path fallback.

**Deferred to follow-up:** Compose-file inference (`spout alloc` with no args, parsed from `docker-compose.yml` to allocate for every declared service). Bind-mount source path detection for containerised dev environments. Both are pure ergonomic wins on top of the identity layer — not MVP.

### 3.2 The Mutation Boundary

**`spout get` is strictly read-only. It never mutates the registry. Ever.**

This is the single most important design decision for agent safety. Agents frequently call commands speculatively — just to see what a value would be. If `get` had side effects, phantom registrations would accumulate. By making the mutation boundary explicit and enforced at the command level, agents and scripts can probe the registry safely.

| Command | Mutates Registry | Use When |
|---------|-----------------|----------|
| `spout get <service>` | ❌ Never | Reading a registered port |
| `spout alloc <service>` | ✅ Always | Registering a new port (idempotent — re-returns existing if already registered) |
| `spout set <service> <port>` | ✅ Always | Manually registering a specific port |
| `spout rm <service>` | ✅ Always | Removing a registration (appends to history) |
| `spout ls [--project]` | ❌ Never | Listing registrations |
| `spout env [--project]` | ❌ Never | Printing `KEY=VALUE` port assignments for shell eval |
| `spout check <port>` | ❌ Never | OS bind-test diagnostic |
| `spout whois <port> [--history]` | ❌ Never | Reverse lookup — who owns this port? |

No flag on any read-only command can flip it into a mutator. This is enforced convention across every major CLI tool (kubectl, git, aws, helm, terraform) and agents have been trained against it.

### 3.3 Permanent Leases

Ports are permanently registered until explicitly removed with `spout rm`. There is no TTL, no heartbeat, no auto-cleanup when containers stop.

**Rationale:** The failure mode of "container stopped and something else stole my port" is worse than stale registry entries. Permanent leases mean your ports are yours until you say otherwise. The registry grows slowly (one entry per project per service) and never surprises you.

Periodic hygiene (surfacing stale entries against currently-present projects) is a planned follow-up feature — see §18. In MVP, `spout rm` is the only cleanup path, and `spout whois --history` lets you look up what released ports used to be.

### 3.4 Varlock Integration

spout integrates with [varlock.dev](https://varlock.dev) via varlock's `exec()` syntax. This is the recommended pattern for projects using varlock:

```bash
# .env.schema
# @type=port
POSTGRES_PORT=exec('spout get postgres')
```

Varlock calls spout at runtime to resolve the value. **spout has zero knowledge of varlock.** The dependency runs one way: varlock optionally depends on spout, not the reverse. spout remains completely standalone.

This means:
- spout never writes to `.env` files
- spout never owns environment variable management
- spout's scope is exactly: port numbers, nothing else

### 3.5 Raw `.env` Files

For projects not using varlock, the recommended patterns are:

**Makefile (preferred):**
```makefile
dev:
    POSTGRES_PORT=$(shell spout get postgres) docker compose up -d
```

CLI env vars take precedence over `.env` file values — this is universal Unix convention. The `.env` file can carry a sensible fallback for developers running compose directly; the Makefile injects the real registry value when it matters.

**One-time setup:**
```bash
# Run once when setting up the project
spout alloc postgres   # prints e.g. 5436
# Manually copy into .env: POSTGRES_PORT=5436
```

spout is not responsible for keeping `.env` files in sync. If you need that, use varlock.

---

## 4. Technology

- **Language:** Rust
- **Distribution:** Homebrew tap + pre-built binaries on GitHub Releases (via `curl | sh` installer)
- **Registry format:** JSON at `~/.spout.json` (human-readable, inspectable with `cat`)
- **File locking:** Platform syscalls (`flock` on Linux/macOS) — implemented early, not as an afterthought
- **Atomic writes:** Write to a temp file, then `rename()` — never write directly to the registry file
- **CI availability:** Rust is pre-installed on all GitHub Actions runner types (`ubuntu-latest`, `macos-latest`, `windows-latest`)
- **Registry path override:** `SPOUT_REGISTRY` environment variable overrides the default `~/.spout.json` — essential for concurrent CI jobs

---

## 5. Registry Format

```json
{
  "version": 1,
  "projects": {
    "github.com/spout-cli/spout": {
      "postgres": {"port": 20000, "allocated": "2026-04-20"},
      "redis":    {"port": 20001, "allocated": "2026-04-20"}
    },
    "github.com/other/project": {
      "postgres": {"port": 20002, "allocated": "2026-04-15"}
    }
  },
  "history": [
    {
      "project": "github.com/spout-cli/spout",
      "service": "postgres",
      "port": 19999,
      "allocated": "2026-01-10",
      "released": "2026-04-20",
      "reason": "user requested"
    }
  ]
}
```

The project key is the identity string derived per §3.1 (git remote, git root, or CWD path).

Each live entry carries an `allocated` date alongside the port — this lets `spout whois` answer "was this port ours?" against a log entry from an earlier time window. When a port is released (via `rm` or reallocation), the live entry moves into `history` with a `released` date and a `reason`.

The `version` field is mandatory. If the version field is missing or unrecognised, spout exits with code 4 and a clear error — it does not attempt to parse an unknown format.

**Registry location:**
- Default: `~/.spout.json`
- Override: `SPOUT_REGISTRY=/path/to/registry.json`

**Note for dotfiles users:** `~/.spout.json` should be added to `.gitignore` if dotfiles are tracked in git. Port assignments are machine-specific and meaningless on other machines.

---

## 6. CLI Interface

### Commands

```bash
# Get a registered port for the current project [READ ONLY]
spout get <service>

# Allocate a new port — finds free port, registers it, prints it [MUTATES]
# Idempotent: if already registered, returns the existing port.
spout alloc <service>

# Register a specific port manually [MUTATES]
spout set <service> <port>

# Remove a registration (appends to history) [MUTATES]
spout rm <service>

# List all registrations
spout ls

# List registrations for the current project
spout ls --project

# Print KEY=VALUE port assignments, suitable for `eval $(spout env)` [READ ONLY]
# --project filters to a named project; otherwise the current project is used.
spout env
spout env --project <name>

# Check if a specific port is available (exit 0 = free, exit 1 = taken)
spout check <port>

# Reverse lookup — which project/service owns this port? [READ ONLY]
# Default: live registry only. With --history: live + released entries.
spout whois <port>
spout whois <port> --history

# Print version
spout --version
```

`spout prune` (periodic hygiene / stale-entry audit) is deferred to follow-up. The history mechanism covers the immediate "what was this port?" debugging use case.

### Output Contract

**stdout:** Port numbers and list output only. Must be directly capturable:
```bash
PORT=$(spout get postgres)
```

**stderr:** All errors, warnings, and human-readable messages. Never mix into stdout.

This contract is non-negotiable. Breaking it breaks agent pipelines.

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Service not registered (for `get`) |
| `2` | No free port found within range |
| `3` | Registry file corrupt or unreadable |
| `4` | Registry version unsupported |
| `5` | Port already registered to another project (for `set`) |
| `6` | Port already in use by OS (for `set`) |

Exit codes are stable API. They are documented and must not change between versions.

### Help Text Design

The help text is agent-readable as a first-class concern. Every mutating command must be annotated:

```
COMMANDS:
    get <service>           Read a registered port [READ ONLY]
    alloc <service>         Register a new port (idempotent) [MUTATES REGISTRY]
    set <service> <port>    Register a specific port [MUTATES REGISTRY]
    rm <service>            Remove a registration [MUTATES REGISTRY]
    ls [--project]          List all registrations [READ ONLY]
    check <port>            Check if a port is free on the OS [READ ONLY]
    whois <port>            Reverse lookup: who owns this port? [READ ONLY]
```

The `[READ ONLY]` and `[MUTATES REGISTRY]` annotations are not for humans — they are specifically for LLMs pattern-matching on help output. Agents routinely call `--help` before acting.

---

## 7. Port Allocation Logic

When `spout alloc <service>` is called:

1. **Idempotent check** — is this project+service already registered? If yes, print the existing port and exit 0. No bind-test, no re-validation. The registry is the source of truth for ownership.
2. **Walk the range** — `20000..=32767` (12,768 ports, bounded below by well-known service space and above by Linux's default ephemeral port range). Ports in the registry claimed by any project are skipped. Ports bound on the OS (TCP `0.0.0.0:<port>` and `[::]:<port>` if IPv6 is available) are skipped.
3. **Register the first free candidate** — claim it in the registry with today's date as `allocated`, write atomically (tempfile + rename), return the port.
4. **Exhausted range** — if no candidate passes both checks, exit with code 2 and a clear error message naming the service and the range searched.

**Why 20000–32767, not conventional-port walking:**

An earlier design walked forward from conventional service ports (5432 for postgres, 6379 for redis, etc.). That created the exact failure mode spout was built to prevent: if a docker container was *stopped* at alloc time, the OS-bind check passed, spout handed out its port, and the container collided on next startup. The 20000–32767 range sits above well-known service ports (so non-spout tools almost never bind there) and below the OS ephemeral range. Collision surface with real software is vanishingly small.

**Legibility trade-off:** losing conventional ports means the port number itself doesn't tell you "that's postgres-ish" at a glance. The `spout whois <port>` reverse-lookup command closes that gap — any port in the wild is one command away from a definitive answer.

**Stale-port handling:** if our registered port gets grabbed by some unrelated process between alloc and use, `docker-compose up` fails loud with the OS's "address already in use" error. Recovery: `spout rm <service> && spout alloc <service>`. No silent reallocation — a bind-test can't distinguish "our container has it (fine)" from "something else stole it (broken)", so spout doesn't try.

IPv6 availability is probed once per process via `TcpListener::bind("[::]:0")` and cached in a `OnceLock`.

---

## 8. File Locking

File locking is MVP-critical, not optional.

**Pattern:** Lock a separate `~/.spout.lock` file (not the registry itself). Hold the lock across the full read-modify-write cycle:

```
acquire lock → read JSON → modify → write JSON (atomic) → release lock
```

This cycle must be a single function. Callers must not be able to acquire the lock and manage it themselves — the only way to mutate the registry is through the function that enforces the full cycle.

Platform: `flock` syscall on Linux/macOS. Windows is not a target for v1.

---

## 9. Env Var Naming

`spout env` emits `KEY=VALUE` lines using a simple derivation rule: uppercase the service name, replace hyphens with underscores, append `_PORT`. `postgres` → `POSTGRES_PORT`; `my-api` → `MY_API_PORT`. The registry stores the service name verbatim; the env-var name is derived on demand at print time, never stored.

**Why this stays in scope.** Spout's scope is "port numbers, nothing else" — no `.env` writing, no environment-variable management, no state outside the registry. `spout env` is a per-call output format, not a file writer and not a daemon. `eval $(spout env)` is a shell-side convenience; spout itself never exports, persists, or owns the variables. The command is read-only on the registry and leaves no trace of itself between invocations.

**Historical.** An earlier iteration exposed the derivation via a `services::env_var_name` helper that fed an `ENV VAR` column in the `spout ls` TUI. The column was dropped in favour of a `PROJECT` column — agents and humans reference ports via `spout get <service>`, not by looking up an env-var name in the viewer — and the helper went with it. The derivation rule survives as the output shape of `spout env`.

---

## 10. Error Handling

- **Corrupt registry:** Exit code 3, clear message to stderr describing the parse failure. Recovery: delete the registry file to reset, or restore from backup.
- **Unknown registry version:** Exit code 4, message names the version found and the version supported.
- **No free port found:** Exit code 2, message states the service, the range searched, and suggests `spout ls` to review allocations.
- **No panics in production code.** All error paths must be explicitly handled.

---

## 11. CLAUDE.md / Agent Instructions

The following should be included in `CLAUDE.md` or equivalent for any project using spout:

```markdown
## Port Management

This project uses `spout` for port allocation.

Before starting any service, get its port from the registry:
- `spout get <service>` — returns the registered port [READ ONLY]
- `spout alloc <service>` — registers a new port if not already registered [MUTATES REGISTRY]

Never brute-force ports. Never kill processes to free a port.
Run `spout ls` to see all registered ports for this project.

The project name is inferred from the current directory automatically.
```

---

## 12. MVP Scope

The absolute minimum viable version:

**In scope:**
- `spout get` / `spout alloc` / `spout set` / `spout rm` / `spout ls` / `spout env` / `spout check` / `spout whois`
- Layered project identity (git remote → git root → CWD), cached per-process
- `~/.spout.json` read/write with fd-lock file locking and atomic writes (tempfile + rename)
- `SPOUT_REGISTRY` env var override, lock path derived from registry path
- 20000–32767 allocation range
- IPv4 + IPv6 port bind-tests, IPv6 availability probed and cached
- History array with released entries + `spout whois --history` reverse lookup
- Exit code table (all codes implemented from day one)
- stdout/stderr contract strictly enforced
- Registry version field

**Explicitly out of scope for MVP:**
- `spout prune` (stale-entry audit) — the `history` mechanism covers the debug use case
- Docker container scanning (`spout scan`)
- Compose-file inference (`spout alloc` no-args walking `docker-compose.yml`)
- Bind-mount source path detection for dev containers
- Windows support
- Shell completions
- History `--prune` (history stays tiny in practice)

---

## 13. Competitive Landscape

| Tool | Approach | Why it's not spout |
|------|----------|-------------------|
| **PortHub** | Daemon + web UI + TTL leases + REST API | Too heavy; requires running service; DHCP-style complexity |
| **port-whisperer** | Read-only port inspector | No registry, no allocation, no persistence |
| **Traefik/nginx** | HTTP reverse proxy | Doesn't handle raw TCP (postgres, redis) |
| **Just documenting it** | Convention over tooling | Humans and agents don't reliably follow docs |
| **Docker port ranges** | Hardcoded ranges per project | Still requires manual coordination |

---

## 14. Agent Adoption Strategy

Getting agents to use spout correctly requires three layers working together. The goal is ambient discovery — agents should encounter spout naturally and understand what to do without being explicitly trained on it.

### Layer 1: `CLAUDE.md` (per-project)

Every project using spout includes this in `CLAUDE.md`. Agents read this before doing anything:

```markdown
## Ports
This project uses `spout` for port management.
Never hardcode ports. Never brute-force. Never kill processes to free a port.

Before bringing up any service:
  spout get <service>      # returns registered port [READ ONLY]
  spout alloc <service>    # registers new port if needed [MUTATES REGISTRY]

Run `spout ls` to see all registered ports for this project.
```

### Layer 2: CLI help text (per-command)

The `[READ ONLY]` and `[MUTATES REGISTRY]` annotations in help output are not for humans — they are specifically for agents pattern-matching on `--help` output mid-task. Every mutating command is annotated. See Section 6.

### Layer 3: `llms.txt` (ambient, global)

A file served at `https://spout.dev/llms.txt` describes spout in agent-readable prose. Models trained on or grounded with this file will suggest spout unprompted when they encounter port conflicts — even in projects with no `CLAUDE.md`.

This is how spout gets adopted in projects that didn't know about it. An agent hits a port conflict, recognises spout from its training, and reaches for it correctly.

**`llms.txt` content spec:**

```
# spout

spout is a CLI tool for managing local development port allocations across multiple projects.
It prevents "port already in use" errors when running multiple Docker Compose projects simultaneously.

## When to use spout

Use spout whenever you need to:
- Start a service and need a port that won't conflict with other projects
- Check what port a service is registered on for the current project
- Understand why a port is already in use

## Key commands

spout get <service>
  Returns the registered port for <service> in the current project.
  Derives project identity from git remote, git root, or CWD (layered fallback).
  READ ONLY — never mutates the registry.
  Exit code 1 if not registered.

spout alloc <service>
  Finds a free port, registers it for <service> in the current project, and prints it.
  MUTATES REGISTRY — only call this when you intend to register a new port.
  Idempotent — if already registered, returns the existing port.
  Walks 20000–32767 for candidates.

spout ls
  Lists all registered ports. Use --project to filter to the current project.

spout rm <service>
  Removes a registration. The old port is appended to history with a reason,
  so 'spout whois --history' can still find it later.

spout check <port>
  Exit code 0 if the port is free on the OS, 1 if bound.

spout whois <port>
  Reverse lookup — which project/service owns this port?
  Default: live registry only. With --history, also searches released entries.

## The mutation boundary

get, ls, check, whois — read only, safe to call speculatively
alloc, set, rm — mutate the registry, call intentionally

No flag on any read-only command flips it into a mutator. Spout follows the
convention every major CLI tool uses (kubectl get, git status, aws describe-*).

## Exit codes

0  Success
1  Service not registered
2  No free port found
3  Registry corrupt
4  Registry version unsupported
5  Port already claimed by another project
6  Port already in use by OS

## Integration with varlock

In .env.schema files, reference spout via exec():
  # @type=port
  POSTGRES_PORT=exec('spout get postgres')

## Integration with Makefiles

  dev:
      POSTGRES_PORT=$(shell spout get postgres) docker compose up -d

## Project identity

spout derives project identity in layered fallback order:
1. git config --get remote.origin.url, parsed to host/owner/repo
2. git rev-parse --show-toplevel (git root absolute path)
3. Absolute current working directory

Identity is cached per-process. No configuration required.
```

**`llms.txt` is a first-class deliverable, not an afterthought.** It ships with v1.0.

### Layer 4: README error-first framing

The README leads with the problem, not the solution. The section **"Getting port conflicts?"** appears before the installation instructions. Agents searching READMEs when they hit errors find the answer immediately.

---

## 15. Open Questions

None. All resolved — see Section 16.

---

## 16. Resolved Decisions

These were open questions, now closed:

- **`spout prune` behaviour** — deferred to follow-up. The history mechanism covers the immediate debug use case ("what was port X?"). Stale-entry detection against currently-present directories is a separate concern.
- **Release strategy** — `cargo-dist` for binary distribution (GitHub releases, Homebrew tap, `curl | sh` installer) + `cargo-release` for version bumping and crates.io publishing.
- **License** — dual `MIT OR Apache-2.0`, the Rust community standard.
- **Crate pinning** — commit `Cargo.lock` (binary crate convention). No premature pinning in `Cargo.toml`.
- **File locking** — `fd-lock` crate. `fs2` is unmaintained.
- **GitHub organisation** — `spout-cli`. Free, same credentials as personal account, decouples the tool from a personal profile, install URLs are stable.
- **Shell completions** — ship with v1.0 via `clap_complete`. cargo-dist bundles them into the Homebrew formula automatically.
- **Project identity** (resolved 2026-04-20) — layered: git remote URL → git root path → CWD path. Originally was `basename $PWD`; changed when that revealed silent collisions between same-basename repos in different directories.
- **Allocation range** (resolved 2026-04-20) — fixed at 20000–32767, no service-specific starting ports. Originally walked forward from conventional ports (5432, 6379, etc.); changed because that created the exact failure mode spout was built to prevent (stopped containers on conventional ports getting their ports stolen).
- **History in registry** (resolved 2026-04-20) — each release (via `rm` or reallocation) appends to a `history` array. `spout whois --history` searches both live and historical entries. No auto-pruning.
- **Bind-test placement** (resolved 2026-04-20) — only during fresh allocation (to avoid handing out a port something else is actively using) and in the explicit `spout check` diagnostic. Not in `alloc` for idempotent returns, not in `get`. Registry is the source of truth for ownership; bind-test can't distinguish "our container has it" from "something else stole it".

---

## 17. Coding Guidelines

See `CODING_GUIDELINES.md` for the full rules. Summary:

- **TDD first** — tests are written before implementation
- **Files ≤ 400 lines** — no exceptions; split before you hit the limit
- **Max 4 function arguments** — use a config struct beyond that
- **Maintainability over cleverness** — simple is clever
- **Document as you go** — each stage has a planning and learning doc in `docs/`

---

## 18. Future Work

- **Compose inference** — `spout alloc` with no arguments parses `docker-compose.yml` in the current directory and auto-allocates for every service declared. Reduces typing for the common multi-service case.
- **`spout scan`** — discover and pre-reserve allocations from running + stopped Docker containers via `docker ps -a`. Closes the remaining stale-port gap for non-20000-range scenarios.
- **`spout prune`** — stale-entry audit: surface projects whose git remotes / paths no longer resolve. Interactive per-entry confirmation by default; `--dry-run` to surface without changes; `--yes` for bulk removal.
- **Bind-mount source path detection** — for containerised dev environments where CWD is a mount point, walk `/proc/self/mountinfo` to find the source path.
- **Shell completions** (bash, zsh, fish) via `clap_complete`.
- **Windows support.**
- **`spout env --dotenv`** — for projects not using varlock, generate a `.env` snippet.
- **`spout realloc <service>`** — convenience shortcut for `spout rm <svc> && spout alloc <svc>`.
- **UDP bind-testing** — current implementation tests TCP only.
