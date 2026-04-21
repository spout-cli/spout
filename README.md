# spout

> Local development port registry. No daemon. No config. No surprises.

---

## Getting "port already in use"?

```bash
# Install
brew install spout-cli/spout/spout

# In any project directory
cd your-project
spout alloc postgres    # 5436 — registered, conflict-free, permanent
```

Use it in your Makefile:

```makefile
dev:
	POSTGRES_PORT=$(shell spout get postgres) docker compose up -d
```

Or in your `.env.schema` with [varlock](https://varlock.dev):

```
# @type=port
POSTGRES_PORT=exec('spout get postgres')
```

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
spout set <service> <port>  # manually register a port          [MUTATES]
spout rm <service>          # remove a registration             [MUTATES]
spout ls                    # list all projects                 [READ ONLY]
spout ls --project          # list only the current project     [READ ONLY]
spout check <port>          # exit 0 if free, 1 if taken        [READ ONLY]
spout whois <port>          # which project/service owns a port [READ ONLY]
spout whois <port> --history  # include released ports          [READ ONLY]
```

### Listing services

In a terminal, `spout ls` (with or without `--project`) launches a styled, read-only viewer — columns for service, port, allocation date, and `$ENV_VAR` name. Press `q`, `Esc`, or `Ctrl-C` to exit.

When stdout is piped, redirected, or you pass `--no-tui`, the command emits plain text instead. Scripts, Makefiles, and AI agents always see the plain-text path, so nothing changes for automation.

```bash
spout ls                    # interactive viewer in a terminal
spout ls --no-tui           # plain text, even in a terminal
spout ls | cat              # plain text (pipe → no TTY)
```

### Project name

spout uses your current working directory as the project name, matching Docker Compose's convention. No configuration required.

```bash
cd /projects/tyfi
spout alloc postgres      # registered under project "tyfi"
```

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
    "tyfi": { "postgres": 5436, "api": 8081 },
    "myproject": { "postgres": 5434, "redis": 6380 }
  }
}
```

When you run `spout alloc postgres`, spout:

1. Acquires a file lock on `~/.spout.lock`
2. Reads the registry
3. Walks forward from the service's default port (5432 for postgres)
4. Skips ports claimed by other projects, or bound by the OS
5. Registers the first free port to your current project
6. Writes the registry atomically and releases the lock

That's the entire design. No surprises.

---

## Integrations

### With varlock

```
# .env.schema
# @type=port
POSTGRES_PORT=exec('spout get postgres')
REDIS_PORT=exec('spout get redis')
```

Varlock resolves these at runtime. spout knows nothing about varlock — the dependency runs one way.

### With Makefiles

```makefile
dev:
	POSTGRES_PORT=$(shell spout get postgres) \
	REDIS_PORT=$(shell spout get redis) \
	docker compose up -d
```

CLI assignment takes precedence over any `.env` file, so this always wins.

### With raw `.env` files

Run `spout alloc <service>` once during project setup, then paste the number into your `.env` file. spout still prevents collisions — you just do the last mile manually.

For reliable automation, use varlock or a Makefile.

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

Windows is not supported in v1.

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
