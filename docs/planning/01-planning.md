# Stage 1 — Planning

**Stage:** MVP core  
**Written:** Before coding begins  
**Covers:** Project scaffold through working `get`, `alloc`, `set`, `rm`, `ls`, `check`

---

## What we are building

The complete MVP as defined in the PRD. A Rust CLI that:

- Reads and writes a JSON registry at `~/.spout.json`
- Infers the project name from the current working directory
- Allocates conflict-free ports by checking both the registry and the OS
- Enforces a hard mutation boundary — `get` is read-only, always
- Locks the registry file for safe concurrent access
- Writes atomically — temp file + rename, never direct write
- Emits port numbers to stdout and errors to stderr
- Returns documented exit codes for every failure mode

Not in scope for this stage: `spout gc`, Ratatui TUI, shell completions, Docker scanning.

---

## Dependency decisions

These are fixed before writing a line of code.

| Crate | Purpose | Why |
|-------|---------|-----|
| `clap` | CLI argument parsing | Industry standard, generates shell completions via `clap_complete` |
| `clap_complete` | Shell completion generation | Bundled into Homebrew formula by cargo-dist automatically |
| `serde` + `serde_json` | Registry serialisation | Ubiquitous, zero friction |
| `fd-lock` | File locking | Actively maintained, cross-platform advisory locks. `fs2` is unmaintained. |
| `tempfile` | Atomic writes | Safe temp file creation with auto-cleanup on drop |
| `thiserror` | Error type derivation | Reduces boilerplate on `SpoutError` without hiding what's happening |
| `dirs` | Home directory resolution | Handles edge cases `$HOME` doesn't. More reliable than `std::env::var("HOME")`. |
| `tracing` | Structured instrumentation | `debug!`, `info!`, `warn!` throughout the codebase. Zero cost when disabled. |
| `tracing-subscriber` | Log output | Reads `RUST_LOG`, formats to stderr. Activated by `-v` flag or `RUST_LOG` env var. |

**`Cargo.lock` is committed.** spout is a binary crate. Committing the lockfile gives reproducible CI builds without premature version pinning in `Cargo.toml`.

---

## Module structure

Each module has one job. The dependency graph flows in one direction — no circular dependencies.

```
src/
  main.rs       # CLI dispatch only. No logic. Target: < 60 lines.
  cli.rs        # clap argument definitions. No logic.
  error.rs      # SpoutError enum. Every variant = one exit code.
  project.rs    # Infer project name from CWD.
  services.rs   # Well-known service → default starting port.
  registry.rs   # Read, write, lock registry. The most critical module.
  allocator.rs  # Port walking logic. Depends on registry + services.
```

Dependency graph:

```
main.rs
  └── cli.rs
  └── allocator.rs
        └── registry.rs
              └── error.rs
        └── services.rs
        └── error.rs
  └── registry.rs
  └── project.rs
        └── error.rs
  └── error.rs
```

`error.rs` is the only module everything touches. Everything else is isolated.

---

## Build order

We build modules in dependency order — leaves first, root last. This means every module can be fully tested before the thing that depends on it is written.

### Step 1: `error.rs`

Define `SpoutError` and the exit code mapping. Every other module depends on this. Write it first, write it completely, don't touch it again unless a new error case is discovered.

```rust
pub enum SpoutError {
    ServiceNotRegistered,
    NoFreePortFound { service: String, range_start: u16, range_end: u16 },
    RegistryCorrupt(std::io::Error),
    RegistryVersionUnknown(u32),
    PortAlreadyClaimed { port: u16, project: String },
    PortInUse(u16),
}

impl SpoutError {
    pub fn exit_code(&self) -> i32 { ... }
}
```

Tests: every variant has a test asserting the correct exit code.

---

### Step 2: `project.rs`

Infer project name from `std::env::current_dir()`. Strip to the final path component.

```rust
pub fn current_project() -> Result<String, SpoutError>
```

This is intentionally simple. The monorepo edge case (walk up to git root) is future work and explicitly not in scope. Do not implement it now.

Tests:
- Returns the directory name in the normal case
- Handles the `SPOUT_REGISTRY` env var override (wait — that's registry, not project. Keep concerns separate.)
- Returns an error if the CWD cannot be read

---

### Step 3: `services.rs`

A pure lookup table. No I/O, no errors, no state.

```rust
pub fn default_port(service: &str) -> u16
pub fn env_var_name(service: &str) -> String
```

`default_port` matches against known service names (case-insensitive) and returns the default starting port. Unknown services return 19000.

`env_var_name` applies the transformation rule: uppercase, hyphens to underscores, append `_PORT`, guard against double-appending.

Tests: every known service name maps to the correct port. The env var naming rule is tested exhaustively including the double-`_PORT` guard and hyphenated names.

---

### Step 4: `registry.rs`

The most critical module. Gets the most test coverage. Contains:

```rust
pub fn read(path: &Path) -> Result<Registry, SpoutError>
pub fn write(path: &Path, registry: &Registry) -> Result<(), SpoutError>
pub fn with_lock<F, T>(path: &Path, f: F) -> Result<T, SpoutError>
    where F: FnOnce(&mut Registry) -> Result<T, SpoutError>
```

The public API for mutation is `with_lock` only. It:
1. Opens (or creates) `~/.spout.lock`
2. Acquires an exclusive lock via `fd-lock`
3. Reads `~/.spout.json` (or starts with empty registry if it doesn't exist)
4. Calls `f` with a mutable reference to the registry
5. Writes the result atomically via `tempfile` + `rename()`
6. Releases the lock on drop

`read` is public for read-only commands (`get`, `ls`, `check`). It does not lock — reads are safe without locking because writes are atomic.

**Registry path resolution:**

```rust
pub fn registry_path() -> PathBuf {
    // SPOUT_REGISTRY env var overrides ~/.spout.json
    std::env::var("SPOUT_REGISTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .expect("cannot determine home directory")
                .join(".spout.json")
        })
}
```

**Registry struct:**

```rust
#[derive(Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    pub projects: HashMap<String, HashMap<String, u16>>,
}

impl Registry {
    pub fn get(&self, project: &str, service: &str) -> Option<u16>
    pub fn set(&mut self, project: &str, service: &str, port: u16)
    pub fn remove(&mut self, project: &str, service: &str) -> bool
    pub fn is_port_claimed(&self, port: u16) -> Option<(String, String)>
}
```

Tests:
- `read` returns empty registry when file does not exist
- `read` returns error on corrupt JSON (exit code 3)
- `read` returns error on unknown version (exit code 4)
- `write` produces valid JSON readable by `read`
- `with_lock` is the only way to mutate — verify reads see writes
- Concurrent `with_lock` calls do not corrupt the registry (spawn threads)
- `SPOUT_REGISTRY` env var is respected
- Atomic write: simulate crash mid-write, registry is not corrupted

---

### Step 5: `allocator.rs`

Port walking logic. Depends on `registry.rs` and `services.rs`.

```rust
pub struct AllocOptions {
    pub start_port: u16,
    pub max_walk: u16,   // how far to walk before giving up (default: 1000)
}

pub fn alloc(
    project: &str,
    service: &str,
    opts: AllocOptions,
) -> Result<u16, SpoutError>

pub fn is_port_free_on_os(port: u16) -> bool
```

`alloc`:
1. Opens registry with `with_lock`
2. Checks if already registered — if yes, returns existing port (idempotent)
3. Walks forward from `opts.start_port`
4. For each candidate: checks registry, then checks OS (both IPv4 and IPv6)
5. On success: registers and returns the port
6. On exhaustion: returns `SpoutError::NoFreePortFound`

`is_port_free_on_os` attempts `TcpListener::bind` on both `0.0.0.0:port` and `[::]:port`. The IPv6 check uses a one-time probe to avoid false negatives in environments where IPv6 is disabled.

**IPv6 probe strategy:** At startup (or on first call to `is_port_free_on_os`), attempt `TcpListener::bind("[::]:0")` — port zero, which the OS assigns freely and immediately releases. If this succeeds, IPv6 is available and we check it for every port. If it fails, IPv6 is unavailable on this system and we skip the IPv6 check entirely for all subsequent calls. Cache the result — probe once, not per port.

This handles CI environments and minimal containers that disable IPv6 without falsely marking ports as taken.

```rust
fn ipv6_available() -> bool {
    // Cache result — probe once per process lifetime
    static IPV6: OnceLock<bool> = OnceLock::new();
    *IPV6.get_or_init(|| TcpListener::bind("[::]:0").is_ok())
}
```

Tests:
- Allocates from the correct default starting port for each known service
- Skips ports claimed by other projects in the registry
- Skips ports in use by OS (use a real TcpListener in the test to occupy a port)
- Returns the same port on second call (idempotent)
- Returns error when range is exhausted
- Unknown service starts at 19000

---

### Step 6: `cli.rs`

Clap argument definitions. No logic — only shape.

```rust
#[derive(Parser)]
#[command(name = "spout", about = "Local development port registry")]
pub enum Cli {
    /// Read a registered port [READ ONLY]
    Get { service: String },

    /// Register a new port [MUTATES REGISTRY]
    Alloc { service: String },

    /// Register a specific port manually [MUTATES REGISTRY]
    Set { service: String, port: u16 },

    /// Remove a registration [MUTATES REGISTRY]
    Rm { service: String },

    /// List all registrations
    Ls {
        #[arg(long)]
        project: bool,
    },

    /// Check if a port is available
    Check { port: u16 },

    /// Generate shell completions
    Completions { shell: Shell },
}
```

The `[READ ONLY]` / `[MUTATES REGISTRY]` annotations appear in the doc comments — clap uses these as help text.

Add a `-v` / `--verbose` flag at the top level:

```rust
#[derive(Parser)]
#[command(name = "spout", about = "Local development port registry")]
pub struct Cli {
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}
```

`global = true` means `-v` works on any subcommand: `spout -v alloc postgres`.

Tests: clap's `debug_assert` covers most of this. Verify the help text contains the annotations.

---

### Step 7: `main.rs`

Dispatch only. Parse args, initialise logging, call the right function, handle the result, exit with the right code.

```rust
fn main() {
    let cli = Cli::parse();

    // Initialise tracing. -v flag sets DEBUG level, otherwise RUST_LOG wins,
    // otherwise silent. Always writes to stderr — never pollutes stdout.
    let level = if cli.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::WARN
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let result = match cli.command {
        Commands::Get { service } => cmd::get(&service),
        Commands::Alloc { service } => cmd::alloc(&service),
        // ...
    };
    if let Err(e) = result {
        eprintln!("spout: {e}");
        std::process::exit(e.exit_code());
    }
}
```

`RUST_LOG` takes precedence over `-v` via `EnvFilter::from_default_env()`. Power users can set `RUST_LOG=spout=trace` for maximum verbosity. `-v` sets a floor of DEBUG for users who don't know about `RUST_LOG`.

---

## What "done" looks like for this stage

All of the following must be true before stage 1 is considered complete:

- [ ] `spout get postgres` returns a port or exits 1
- [ ] `spout alloc postgres` registers and returns a port
- [ ] `spout alloc postgres` called twice returns the same port (idempotent)
- [ ] `spout set postgres 5555` registers a specific port
- [ ] `spout rm postgres` removes the registration
- [ ] `spout ls` lists all projects and their ports
- [ ] `spout ls --project` filters to the current project
- [ ] `spout check 5432` exits 0 if free, 1 if taken
- [ ] `spout set postgres 80` exits with a clear error (privileged port)
- [ ] All exit codes are correct per the PRD table
- [ ] stdout contains only the port number — no decoration, no logging
- [ ] stderr receives all error messages and all log output
- [ ] `-v` flag produces debug output to stderr only
- [ ] `RUST_LOG=debug spout alloc postgres` shows port walking decisions
- [ ] `SPOUT_REGISTRY` env var is respected in all commands
- [ ] Lock file path is derived from registry path, not hardcoded
- [ ] Concurrent calls do not corrupt the registry
- [ ] IPv6 probe fires once per process, result is cached
- [ ] No file exceeds 400 lines
- [ ] No function exceeds 40 lines
- [ ] No function has more than 4 arguments
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --all -- --check` passes
- [ ] CI passes on push
- [ ] `docs/planning/01-learning.md` is written

---

## Risks and things to watch

**IPv6 availability** — Resolved above. One-time probe using port 0, cached via `OnceLock`. Probe once per process, not per port check.

**File locking on macOS** — `fd-lock` uses `flock` on Unix. Advisory only, but since spout is the only writer this is not a real risk.

**Lock file path must track registry path** — If `SPOUT_REGISTRY=/tmp/spout-test.json`, the lock file must be `/tmp/spout-test.lock`, not `~/.spout.lock`. Derive the lock path from the registry path by replacing the extension. Failure to do this will cause test isolation issues where tests using different registry paths still contend on the same lock.

**`dirs` crate for home directory** — Use `dirs::home_dir()`. The stdlib `$HOME` env var fallback is fragile in containers and unusual Unix environments.

**Test isolation** — Every test touching the filesystem must set `SPOUT_REGISTRY` to a `tempfile::NamedTempFile` path. Never touch `~/.spout.json` in tests. This is enforced by never calling `registry_path()` directly in tests — always pass the path explicitly.

**Port validation** — `spout set` and `spout check` accept a `u16` port number. Ports 0–1023 are privileged and should be rejected with a clear error. Ports above 65535 cannot exist (u16 max). Validate that the input is in the range 1024–65535.

**Empty or invalid project name** — `basename $PWD` could theoretically return an empty string or `/` in unusual environments (running from the filesystem root). `current_project()` must validate the result and return a clear error rather than registering an empty string as a project name.

**Unicode project names** — Directory names can contain unicode. `serde_json` handles UTF-8 strings correctly. Not a risk, but worth noting so nobody adds unnecessary sanitisation.

**Logging never to stdout** — `tracing_subscriber` must be configured with `.with_writer(std::io::stderr)` explicitly. The default writes to stdout on some configurations. This would break agent pipelines that capture stdout for port numbers. Verify in tests that stdout is clean even with `-v`.

**Signal handling mid-write** — The atomic write (tempfile + rename) means a SIGKILL mid-write leaves the original registry intact. The temp file is cleaned up by `tempfile` on drop, or left as an orphan in `/tmp` — harmless either way. No special signal handling required.

