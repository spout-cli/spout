# Using spout in this project

This project uses [spout](https://github.com/spout-cli/spout) to manage local development ports. Drop this file (or merge it) into your repo's `CLAUDE.md` so Claude Code, Cursor, Aider, and other coding agents know the rules.

## The safety contract

spout splits its commands into two disjoint sets. Agents should only call mutating commands with clear user intent.

| Command | Effect | When to call |
|---|---|---|
| `spout get <service>` | **Read only.** Prints the registered port, exits 1 if not registered. | Any time you need to reference a port in generated code, config, or a command. |
| `spout env` | **Read only.** Prints `KEY=VALUE` lines for every registered service in the project. | When you need every port at once for templating, env files, or shell sourcing. |
| `spout ls` | **Read only.** Shows registered services. | For relaying state to a human ("what's running?"). Never as agent decision input — use `spout get` or `spout env` for that. |
| `spout check <port>` | **Read only.** Exit 0 if the port is free, 1 if taken. | Pre-flight checks. |
| `spout whois <port>` | **Read only.** Reverse lookup — which project/service claims a port. | Debugging "why is port X in use?" |
| `spout completions <shell>` | **Read only.** Emits shell completion scripts. | Setup time only. |
| `spout alloc <service>` | **Mutates.** Registers a new port if not already claimed for this project. Idempotent — safe to re-run. Exits 10 if the name already exists under a sibling project identity (run `spout reproject` first). | When a service is new to the project and needs a port. |
| `spout set <service> <port>` | **Mutates.** Registers a specific port manually. | Only when the user explicitly says "use port N for X". Prefer `alloc` otherwise. |
| `spout rm <service>` | **Mutates.** Removes a registration. | Only when the user explicitly asks to release a port. |

Mutating commands take a file lock on `~/.spout.lock`. Read commands don't.

## How to reference ports

**In Makefiles, scripts, or `.env` schemas — always shell out to `spout get`. Never hardcode port numbers.**

```makefile
dev:
	POSTGRES_PORT=$$(spout get postgres) \
	REDIS_PORT=$$(spout get redis) \
	docker compose up -d
```

```
# .env.schema (varlock)
# @type=port
POSTGRES_PORT=exec('spout get postgres')
```

If `spout get` exits 1, **read the stderr message before doing anything**. It tells you which project you're in, lists the services that *are* registered, and shows any removal-history or sibling-identity context. Four outcomes:

- The error lists the service under a different name (e.g. you asked for `acme-postgres`, available is `postgres`) — use the real name; do **not** allocate a duplicate.
- The error includes a `recently removed:` line for the name you asked for — the user may have just removed it. Check the date before allocating; if it's recent, ask the user before resurrecting.
- The error includes a `registered under different identity:` line — the project's git identity changed (typically after `git init`) and the old entries are orphans. Run `spout reproject --from <old-identity> --to <new-identity>` to migrate the entries, then retry. Don't allocate a duplicate.
- The error says no services are registered yet, or the service genuinely isn't in the list (and isn't in `recently removed:` or `registered under different identity:`) — then `spout alloc <service>` is the right fix.

**Service names are scoped to the project.** spout already knows which project you're in (from CWD). Use the bare service name (`postgres`), never project-prefixed (`acme-postgres`). If you don't know what the project has, run `spout env` once to enumerate everything.

## Project identity

spout identifies projects by git remote URL (falling back to git root, then CWD). It also walks up looking for a `docker-compose.yml` / `compose.yaml` in an ancestor directory to auto-detect monorepo subprojects.

If auto-detection gets it wrong, override with `SPOUT_PROJECT` in a `.envrc` (direnv) or shell rc:

```bash
export SPOUT_PROJECT="my-monorepo/web"
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Service not registered (for `get` / `rm`) |
| 2 | No free port found in range |
| 3 | Registry file corrupt or unreadable |
| 4 | Registry version unsupported |
| 5 | Port already registered to another project |
| 6 | Port already in use by the OS |
| 7 | I/O error (stdout/stdin closed mid-command) |
| 8 | Compose file missing or malformed (for `spout alloc`) |
| 9 | Usage error (invalid flag combination) |
| 10 | `alloc` refused — service exists under sibling project identity (run `spout reproject`) |
| 11 | `reproject` refused — services exist in both source and target |

## Things not to do

- **Don't edit `~/.spout.json` directly.** Always go through the CLI so locking and history are respected.
- **Don't invent port numbers.** If you need a port and `spout get` fails, run `spout alloc` and use what it returns.
- **Don't parse `spout ls` for agent logic.** When stdout is a pipe or `--no-tui` is passed, the output is stable plain text — but for programmatic reads use `spout get <service>` or `spout env`. `ls` is for showing state to a human.
