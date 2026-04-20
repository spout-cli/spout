# spout — Product Requirements Document

**Status:** Draft  
**Version:** 0.1  
**Last Updated:** April 2026

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
5436

# Allocate a new port (MUTATES REGISTRY — explicit, intentional)
$ spout alloc postgres
5437
```

The registry is a single JSON file. No daemon. No service. No background process. Readable by anything.

---

## 3. Design Decisions

### 3.1 Project Name Inference

**spout derives its project namespace from the current working directory name, matching Docker Compose's convention exactly.**

Docker Compose defaults to the directory name of wherever `docker-compose.yml` lives. If your project is at `/Users/you/projects/tyfi`, the compose project name is `tyfi`. That is what ends up in the `com.docker.compose.project` label on containers.

spout does the same: `basename $PWD` is the project name. No flag required.

```bash
# From /Users/you/projects/tyfi
spout get postgres   # looks up "tyfi" + "postgres"
spout alloc postgres # allocates for "tyfi" + "postgres"
```

This means agents running inside a project directory automatically get the right namespace. The naming convention is already load-bearing everywhere — Docker container names, compose labels, what developers call the project in conversation — spout plugs into it rather than inventing a new one.

**Known limitation (future work):** Monorepos. If `/projects/myapp/services/api` and `/projects/myapp/services/worker` exist, the directory names `api` and `worker` will collide across different monorepos. The correct fix is to walk up the directory tree to find the git root or `docker-compose.yml`, as Docker Compose itself does. This is explicitly deferred to v1.1. For MVP, directory name is correct and covers 95% of cases.

### 3.2 The Mutation Boundary

**`spout get` is strictly read-only. It never mutates the registry. Ever.**

This is the single most important design decision for agent safety. Agents frequently call commands speculatively — just to see what a value would be. If `get` had side effects, phantom registrations would accumulate. By making the mutation boundary explicit and enforced at the command level, agents and scripts can probe the registry safely.

| Command | Mutates Registry | Use When |
|---------|-----------------|----------|
| `spout get <service>` | ❌ Never | Reading a registered port |
| `spout alloc <service>` | ✅ Always | Registering a new port for the first time |
| `spout set <service> <port>` | ✅ Always | Manually registering a specific port |
| `spout rm <service>` | ✅ Always | Removing a registration |

### 3.3 Permanent Leases

Ports are permanently registered until explicitly removed with `spout rm`. There is no TTL, no heartbeat, no auto-cleanup when containers stop.

**Rationale:** The failure mode of "container stopped and something else stole my port" is worse than stale registry entries. Permanent leases mean your ports are yours until you say otherwise. The registry grows slowly (one entry per project per service) and never surprises you.

`spout gc` is provided for periodic hygiene — it audits the registry against currently running Docker projects and surfaces stale entries for review, but does not auto-delete.

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
    "tyfi": {
      "postgres": 5436,
      "api": 8081,
      "web": 5173,
      "mailpit-smtp": 1025,
      "mailpit-ui": 8025
    },
    "myproject": {
      "postgres": 5434,
      "api": 8080,
      "web": 3000
    }
  }
}
```

The `version` field is mandatory. It exists for future migration paths. If the version field is missing or unrecognised, spout exits with a clear error — it does not attempt to parse an unknown format.

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
spout alloc <service>

# Register a specific port manually [MUTATES]
spout set <service> <port>

# Remove a registration [MUTATES]
spout rm <service>

# List all registrations
spout ls

# List registrations for the current project
spout ls --project

# Check if a specific port is available (exit 0 = free, exit 1 = taken)
spout check <port>

# Audit stale registry entries against running Docker projects
spout gc

# Print version
spout --version
```

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
    alloc <service>         Register a new port [MUTATES REGISTRY]
    set <service> <port>    Register a specific port [MUTATES REGISTRY]
    rm <service>            Remove a registration [MUTATES REGISTRY]
    ls                      List all registrations
    check <port>            Check if a port is available
    gc                      Audit stale entries
```

The `[READ ONLY]` and `[MUTATES REGISTRY]` annotations are not for humans — they are specifically for LLMs pattern-matching on help output. Agents routinely call `--help` before acting.

---

## 7. Port Allocation Logic

When `spout alloc <service>` is called:

1. **Check the registry** — is this project+service already registered? If yes, print it and exit 0 (idempotent).
2. **Determine the default starting port** — well-known service types have defaults:

| Service name(s) | Default start port |
|-----------------|-------------------|
| `postgres`, `postgresql` | 5432 |
| `mysql`, `mariadb` | 3306 |
| `redis` | 6379 |
| `mongodb`, `mongo` | 27017 |
| `rabbitmq` | 5672 |
| `elasticsearch` | 9200 |
| `meilisearch` | 7700 |
| `api`, `http`, `server` | 8080 |
| `web`, `frontend`, `ui` | 3000 |
| `mailpit-smtp`, `smtp` | 1025 |
| `mailpit-ui` | 8025 |
| Unknown | 19000 |

3. **Walk forward from the default** — for each candidate port:
   - Check the registry (is another project claiming it?)
   - Check the OS via `net::TcpListener::bind` — check **both IPv4 and IPv6**
   - If both clear → register and return
4. **Upper bound:** Stop at default start + 1000. If no port found, exit with code 2 and a clear error message.
5. **Write back** — atomic write: temp file + `rename()`.

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

When generating environment variable names from service names (for documentation and `spout gc` output):

**Rule:** Uppercase, hyphens to underscores, append `_PORT`.

```
postgres      → POSTGRES_PORT
mailpit-smtp  → MAILPIT_SMTP_PORT
worker-2      → WORKER_2_PORT
```

**Guard:** If the service name already ends in `port` (case-insensitive), do not append `_PORT`.

This rule must be explicitly documented in `spout help` output and the README, because projects must match these names in their `.env.schema` files.

---

## 10. Error Handling

- **Corrupt registry:** Exit code 3, clear message to stderr: `spout: registry file is malformed. Run 'spout gc --check' to diagnose or delete ~/.spout.json to reset.`
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
- `spout get` / `spout alloc` / `spout set` / `spout rm` / `spout ls` / `spout check`
- CWD-based project name inference
- `~/.spout.json` read/write with file locking and atomic writes
- `SPOUT_REGISTRY` env var override
- Well-known default port ranges
- IPv4 + IPv6 port availability checks
- Exit code table (all codes implemented from day one)
- stdout/stderr contract strictly enforced
- Registry version field

**Explicitly out of scope for MVP:**
- `spout gc`
- Docker container scanning (`spout scan`)
- Monorepo / git-root detection
- Windows support
- Shell completions

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
  Infers the project name from the current working directory.
  READ ONLY — never mutates the registry.
  Exit code 1 if not registered.

spout alloc <service>
  Finds a free port, registers it for <service> in the current project, and prints it.
  MUTATES REGISTRY — only call this when you intend to register a new port.
  Idempotent — if already registered, returns the existing port.

spout ls
  Lists all registered ports. Use --project to filter to the current project.

spout rm <service>
  Removes a registration. Use when decommissioning a service.

spout gc
  Audits stale entries (projects whose directories no longer exist).
  Use --prune to remove them.

spout check <port>
  Exit code 0 if the port is free, 1 if taken.

## The mutation boundary

get, ls, check — read only, safe to call speculatively
alloc, set, rm, gc --prune — mutate the registry, call intentionally

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

## Project name inference

spout uses the current working directory name as the project name,
matching Docker Compose's convention. No configuration required.
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

- **`spout gc` behaviour** — surface-only by default. `--prune` flag for auto-removal. Only flags entries where the project directory no longer exists — stopped containers are not stale.
- **Release strategy** — `cargo-dist` for binary distribution (GitHub releases, Homebrew tap, `curl | sh` installer) + `cargo-release` for version bumping and crates.io publishing.
- **License** — dual `MIT OR Apache-2.0`, the Rust community standard.
- **Crate pinning** — commit `Cargo.lock` (binary crate convention). No premature pinning in `Cargo.toml`.
- **File locking** — `fd-lock` crate. `fs2` is unmaintained.
- **`spout env`** — dropped. Varlock owns env management. Makefile pattern covers the raw case.
- **GitHub organisation** — `spout-cli`. Free, same credentials as personal account, decouples the tool from a personal profile, install URLs are stable.
- **Shell completions** — ship with v1.0 via `clap_complete`. cargo-dist bundles them into the Homebrew formula automatically.

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

- `spout scan` — discover allocations from running Docker containers via compose labels
- Monorepo support — walk up to git root / `docker-compose.yml` instead of bare `basename $PWD`
- Shell completions (bash, zsh, fish)
- Windows support
- `spout env --dotenv` — for projects not using varlock, generate a `.env` snippet
