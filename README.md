# spout

> Local development port registry. No daemon. No config. No surprises.

---

## Getting "port already in use"?

```bash
# Install
brew install spout-cli/spout/spout

# In any project directory
cd your-project
spout alloc postgres    # 20000 — registered, conflict-free, permanent
```

Use it from any shell:

```bash
POSTGRES_PORT=$(spout get postgres) docker compose up -d
```

Or wire it into whatever you already use — direnv, varlock, just, Make, or a one-time paste into `.env`. See [Integrations](#integrations) below.

Done. Your port is yours, permanently, across every restart.

---

## What it is

A tiny Rust CLI that maintains a JSON registry of which local projects own which ports. When you run multiple Docker Compose projects on one machine, spout stops them fighting over 5432, 6379, 8080.

Think of it as a filing cabinet for port numbers. One drawer per project, one slot per service. Write once, read forever.

- **No daemon.** No background process, no service to manage.
- **No config.** Project name is inferred from your working directory.
- **Permanent leases.** Your ports stay yours until you explicitly release them.
- **Agent-first.** Clear read-only vs mutating command split. Clean stdout/stderr contract. Works flawlessly with Claude Code, Cursor, Aider, and anything that speaks a shell.

---

## Installation

### Homebrew (macOS and Linux)

```bash
brew install spout-cli/spout/spout
```

### curl installer

```bash
curl -sSfL https://spout.dev/install.sh | sh
```

### From source

```bash
cargo install spout
```

### Shell completions

Once spout is installed, generate a completion script for your shell and drop it in the conventional location:

```bash
# bash
spout completions bash | sudo tee /etc/bash_completion.d/spout >/dev/null

# zsh — your fpath needs to include the target directory
mkdir -p ~/.zsh/completions
spout completions zsh > ~/.zsh/completions/_spout

# fish
spout completions fish > ~/.config/fish/completions/spout.fish
```

Restart your shell (or `source` the file) and subcommand + flag completion works under `spout <TAB>`.

Elvish and PowerShell are also supported via `spout completions elvish` and `spout completions powershell` — install per your shell's conventions.

---

## Usage

### Core commands

```bash
spout get <service>         # read registered port              [READ ONLY]
spout alloc <service>       # register new port if needed       [MUTATES]
spout alloc <service> --udp # same, UDP instead of TCP          [MUTATES]
spout set <service> <port>  # manually register a port          [MUTATES]
spout rm <service>          # remove a registration             [MUTATES]
spout ls                    # list all projects                 [READ ONLY]
spout ls --project          # list only the current project     [READ ONLY]
spout check <port>          # exit 0 if free, 1 if taken        [READ ONLY]
spout check <port> --udp    # same, UDP instead of TCP          [READ ONLY]
spout whois <port>          # which project/service owns a port [READ ONLY]
spout whois <port> --history  # include released ports          [READ ONLY]
```

### Listing services

In a terminal, `spout ls` (with or without `--project`) launches a styled, read-only viewer — columns for service, port, allocation date, and project. Press `q`, `Esc`, or `Ctrl-C` to exit.

When stdout is piped, redirected, or you pass `--no-tui`, the command emits plain text instead. Scripts, Makefiles, and AI agents always see the plain-text path, so nothing changes for automation.

```bash
spout ls                    # interactive viewer in a terminal
spout ls --no-tui           # plain text, even in a terminal
spout ls | cat              # plain text (pipe → no TTY)
```

### UDP services

Most dev services are TCP — that's the default for every command, and
every existing invocation works unchanged. For services that bind UDP
(DNS, some game servers, QUIC dev stacks, mDNS-alike tooling), add
`--udp`:

```bash
spout alloc dns --udp         # pick a free UDP port in 20000–32767
spout set dns 5353 --udp      # register a specific UDP port
spout check 5353 --udp        # is this UDP port free on the OS?
```

TCP 5432 and UDP 5432 are independent in the registry — kernels treat
them as separate, and so does spout. A single service name is one
protocol: if you need both sides, register two names (`coredns-tcp`,
`coredns-udp`).

`spout whois <port>` has no `--udp` flag because the interesting
question is always "what's on this port?" across every protocol — it
lists every match, TCP first:

```
$ spout whois 5432
5432/tcp: github.com/acme/api/postgres    (active, allocated 2026-04-10)
5432/udp: github.com/acme/game/session    (active, allocated 2026-04-18)
```

### Personalizing the viewer

You can prefix service names with an icon of your choice via `SPOUT_ICONS`:

```bash
export SPOUT_ICONS='postgres=🐘,redis=🔴,api=🌐,mailpit=📨'
spout ls
```

spout ships no built-in mapping — names are yours to define. The variable is read once per invocation; drop it in your shell rc if you want it everywhere. It affects only the terminal viewer; `--no-tui` and piped output are unchanged, so scripts and CI see the same plain text either way.

### Project name

spout infers project identity from your git remote, falling back to your git root, and finally to your absolute working directory. Two projects with the same directory name don't collide.

```bash
cd /projects/myapp
spout alloc postgres      # registered under the project's git remote identity
```

#### Monorepos

In a monorepo, spout auto-detects subprojects by looking for a `docker-compose.yml`, `docker-compose.yaml`, `compose.yml`, or `compose.yaml` file. If it finds one in an ancestor directory of your CWD (below the git root), that ancestor's path becomes part of the project identity:

```
~/work/my-monorepo/apps/web/docker-compose.yml  →  github.com/acme/my-monorepo/apps/web
~/work/my-monorepo/apps/api/compose.yaml        →  github.com/acme/my-monorepo/apps/api
~/work/my-monorepo/docker-compose.yml           →  github.com/acme/my-monorepo  (root marker adds nothing)
~/work/my-monorepo/                             →  github.com/acme/my-monorepo  (no markers)
```

Nearest marker wins — a `docker-compose.yml` at `apps/web` wins over one at the repo root. No configuration needed.

If the auto-detect gets it wrong for your layout, override it with `SPOUT_PROJECT`:

```bash
# apps/web/.envrc  (direnv)
export SPOUT_PROJECT="my-monorepo/web"
```

Unset or empty `SPOUT_PROJECT` falls through to auto-detect, which falls through to today's git-remote / git-root / CWD layering.

### The mutation boundary

`get`, `ls`, `check`, `whois`, and `completions` never touch the registry. You can call them speculatively from scripts or agents without side effects.

`alloc`, `set`, and `rm` mutate the registry and require a file lock. Call them intentionally.

---

## How it works

A single JSON file at `~/.spout.json`:

```json
{
  "version": 1,
  "projects": {
    "myapp": {
      "postgres": { "port": 20000, "allocated": "2026-04-20" },
      "api":      { "port": 20001, "allocated": "2026-04-20" }
    },
    "myproject": {
      "postgres": { "port": 20002, "allocated": "2026-04-21" }
    }
  },
  "history": []
}
```

When you run `spout alloc postgres`, spout:

1. Acquires a file lock on `~/.spout.lock`
2. Reads the registry
3. Walks 20000–32767 in order
4. Skips ports claimed by other projects or bound by the OS
5. Registers the first free port to your current project
6. Writes the registry atomically and releases the lock

Releasing a port (`spout rm`) appends to `history` rather than erasing it, so `spout whois <port> --history` can tell you what used to live there.

That's the entire design. No surprises.

---

## Integrations

spout's output is a port number on stdout. Anything that can read a shell env var or set one can consume it — that includes `docker compose`, which substitutes `${POSTGRES_PORT}` from the shell environment or an adjacent `.env` file. Pick whichever of the below matches your project; they're peers, not a ranked list.

### Plain shell

The lowest common denominator. Works everywhere.

```bash
POSTGRES_PORT=$(spout get postgres) docker compose up -d
```

### direnv

Put this in `.envrc` at your project root, then `direnv allow`. Every shell you open in the project picks up the env; direnv unloads it when you `cd` out.

```bash
# .envrc
export POSTGRES_PORT=$(spout get postgres)
export REDIS_PORT=$(spout get redis)
```

### varlock

Purpose-built for dynamic values in dotenv files.

```
# .env.schema
# @type=port
POSTGRES_PORT=exec('spout get postgres')
REDIS_PORT=exec('spout get redis')
```

varlock resolves these at runtime. spout knows nothing about varlock — the dependency runs one way.

### just or Make

For task-runner workflows:

```makefile
# Makefile
dev:
	POSTGRES_PORT=$(shell spout get postgres) \
	REDIS_PORT=$(shell spout get redis) \
	docker compose up -d
```

```just
# justfile
dev:
    POSTGRES_PORT=$(spout get postgres) REDIS_PORT=$(spout get redis) docker compose up -d
```

CLI-assigned env wins over any `.env` file, so these always take precedence.

### Paste-once `.env`

Leases are permanent, so for pure `.env` setups the simplest workflow is allocate once, paste, commit:

```bash
spout alloc postgres    # 20000
spout alloc redis       # 20001
# paste those numbers into .env
```

spout still prevents cross-project collisions — you just do the last mile manually, once per service.

---

## For AI agents

spout is designed to be used by agents as much as by humans. Three things make this work:

- Every mutating command is annotated `[MUTATES REGISTRY]` in its help text
- `get`/`ls`/`check` are guaranteed read-only — safe to call speculatively
- Exit codes are stable and documented

Drop [this CLAUDE.md template](templates/CLAUDE.md) into your project to teach Claude Code (and others) how to use spout.

An [`llms.txt`](https://spout.dev/llms.txt) is served for ambient model grounding.

---

## Why not...

**PortHub?** Too heavy. Daemon, REST API, web UI, TTL leases. spout is a single binary, a single JSON file, and permanent registrations.

**A reverse proxy like Traefik or nginx?** Great for HTTP routing, useless for raw TCP ports like postgres and redis. Also much heavier.

**Hardcoded port ranges per project?** Works for one developer on one machine. Doesn't survive an agent guessing port numbers.

**Just documenting conventions?** Humans don't follow docs reliably. Agents really don't.

---

## No telemetry

spout does not send anything anywhere. No usage metrics, no error reporting, no phone-home, no analytics, no opt-in tracking. The registry file on your machine is the only thing spout ever writes.

This is a permanent design commitment, not a "not yet."

---

## Requirements

- macOS or Linux
- Docker optional but likely why you're here

Windows is not supported natively. **Windows users: install and run spout inside WSL2.** It's Linux underneath, so spout works exactly as it does on native Linux — which covers the overwhelming majority of Windows-based Docker development in practice.

---

## Exit codes

| Code | Meaning                                |
| ---- | -------------------------------------- |
| 0    | Success                                |
| 1    | Service not registered (for `get`)     |
| 2    | No free port found within range        |
| 3    | Registry file corrupt or unreadable    |
| 4    | Registry version unsupported           |
| 5    | Port already registered to another project |
| 6    | Port already in use by OS              |

---

## Debugging

Set `RUST_LOG=debug` for verbose output, or pass `-v`:

```bash
RUST_LOG=debug spout alloc postgres
spout -v alloc postgres
```

All log output goes to stderr — stdout stays clean for scripting.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and [CODING_GUIDELINES.md](docs/CODING_GUIDELINES.md). TL;DR: TDD, files under 400 lines, functions under 40 lines, four-argument max, no `unwrap()` in production paths.

---

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
