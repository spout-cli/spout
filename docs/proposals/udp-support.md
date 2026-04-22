# `spout` UDP support — proposal

_Status: proposal, 2026-04-22. Not committed to a stage yet._

## Context

Spout is TCP-only today. The allocator's OS-probe (`is_port_free_on_os`)
binds a `TcpListener` on IPv4 and IPv6; the registry schema stores
`{port, allocated}` with no protocol field. Most dev services are
TCP, so TCP-only shipped and has served fine for 0.1.0.

The gap: rare but real UDP services (DNS, some game servers, mDNS-alike
tooling, QUIC dev stacks) sit outside spout today. The user runs one
on an arbitrary port and spout has no awareness of it, which means
`spout alloc` can hand out a port that collides with a UDP container
on startup — the exact failure mode spout was built to prevent,
happening again for UDP services.

## Goals

1. `spout alloc --udp <service>` walks the 20000–32767 range and
   hands out a port free on UDP.
2. `spout set --udp <service> <port>` registers a specific UDP port,
   with the same "claimed elsewhere" and "bound on OS" checks as TCP.
3. `spout check --udp <port>` reports whether a UDP port is currently
   bound.
4. `spout whois <port>` surfaces both TCP and UDP registrations for
   that port number, because asking "who owns 5432?" is a
   protocol-ambiguous question in the wild.
5. TCP 20000 and UDP 20000 coexist in the registry as independent
   allocations — on real kernels they are independent, so spout
   shouldn't conflate them.

## Non-goals

- SCTP, DCCP, or any other L4 protocol. `--udp` is a boolean axis,
  not the first member of an open set.
- Dual-bind services (one service wanting both TCP and UDP on the
  same port). Handled by using two service names: `coredns-tcp` and
  `coredns-udp`. Not elegant, but simple and explicit. Revisit only
  if real use demands it.
- Windows. Still out of scope per the PRD.
- Changing `spout get <service>` to take a protocol. Service name
  remains the unique key per project — see §Service-name uniqueness
  below.

## Design

### Registry schema — bump to version 2

Current `Entry` (`src/registry.rs:33-37`):

```rust
pub struct Entry {
    pub port: u16,
    pub allocated: String,
}
```

Proposed:

```rust
pub struct Entry {
    pub port: u16,
    pub allocated: String,
    #[serde(default)]
    pub protocol: Protocol,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Tcp,
    Udp,
}
```

Same field and derive added to `HistoryEntry`.

`CURRENT_VERSION: u32 = 2`. The read path (`src/registry.rs:130-143`)
widens its accepted range:

```rust
if registry.version != 1 && registry.version != 2 {
    return Err(SpoutError::RegistryVersionUnknown(registry.version));
}
```

Migration is data-free: serde `#[serde(default)]` fills in
`Protocol::Tcp` for every entry in a v1 file. The next mutating
command writes version 2 (`write` always serialises with
`version: CURRENT_VERSION`), completing the migration. An older
spout binary run against a v2 registry errors loudly with exit code 4
— correct behaviour, prevents silent protocol-mismatch corruption.

### OS probe — one function, protocol-dispatched

`src/allocator.rs` adds a UDP mirror and dispatches:

```rust
use std::net::{TcpListener, UdpSocket};

pub fn is_port_free_on_os(port: u16, protocol: Protocol) -> bool {
    match protocol {
        Protocol::Tcp => is_tcp_port_free(port),
        Protocol::Udp => is_udp_port_free(port),
    }
}

fn is_tcp_port_free(port: u16) -> bool {
    TcpListener::bind(("0.0.0.0", port)).is_ok()
        && (!ipv6_available() || TcpListener::bind(("::", port)).is_ok())
}

fn is_udp_port_free(port: u16) -> bool {
    UdpSocket::bind(("0.0.0.0", port)).is_ok()
        && (!ipv6_available() || UdpSocket::bind(("::", port)).is_ok())
}
```

`UdpSocket::bind` has the same "fail if another socket is already
bound to this exact (addr, port)" semantics as `TcpListener::bind` —
std doesn't set `SO_REUSEADDR`/`SO_REUSEPORT` by default, so a plain
`bind` is a faithful probe. UDP has no `TIME_WAIT`, so the port is
reusable immediately after drop (arguably cleaner than TCP).

`ipv6_available()` stays as it is — it probes TCP `[::]:0` once and
caches. IPv6 availability is an OS-level property, not a protocol one.

### Registry queries — protocol-aware

`is_port_claimed` becomes protocol-aware (`src/registry.rs:92-102`):

```rust
pub fn is_port_claimed(
    &self,
    port: u16,
    protocol: Protocol,
) -> Option<(String, String)> {
    for (project, services) in &self.projects {
        for (service, entry) in services {
            if entry.port == port && entry.protocol == protocol {
                return Some((project.clone(), service.clone()));
            }
        }
    }
    None
}
```

TCP 5432 registered to project A does not block UDP 5432 for project B.

`probe_bound_ports` already returns `HashSet<u16>` per-registry; since
every `Entry` now carries its own protocol, the probe passes
`entry.protocol` through to `is_port_free_on_os`. No signature change.

### Service-name uniqueness

Keeping the current shape — service name is unique per project,
regardless of protocol. Two consequences:

- A project can't have `coredns` registered as both TCP and UDP.
  User names them `coredns-tcp` and `coredns-udp` (or whatever reads
  naturally). `spout get coredns-udp` returns the UDP port.
- `spout get <service>` never needs a `--udp` flag. The service name
  carries all the routing info already.

This keeps the existing data shape (`HashMap<service, Entry>`)
untouched, preserves `spout get <service>` as a trivial one-line
lookup, and sidesteps a "which of the two do I return?" question.
The ergonomic cost is user-visible — if you need both, you type two
service names — but it's honest and easy to teach.

### CLI surface

Additions to `src/cli.rs`:

- `Alloc { service, #[arg(long)] udp: bool }` — `--udp` switch;
  default off means TCP.
- `Set { service, port, #[arg(long)] udp: bool }`.
- `Check { port, #[arg(long)] udp: bool }`.
- `Whois { port, #[arg(long)] history: bool }` — **no** `--udp`
  flag. `whois` surfaces every protocol for that port, because the
  most common `whois` question ("what's on 5432?") is
  protocol-ambiguous. Output format changes to list each match on
  its own line.

Short flag `--udp` preferred over `--protocol <tcp|udp>`: boolean
axis, terser, and leaves us room to rename to `--protocol` later if
we ever need a third value (we won't).

### Output changes

- `spout whois 5432` with both protocols registered:
  ```
  5432/tcp: github.com/acme/api/postgres     (active, allocated 2026-04-10)
  5432/udp: github.com/acme/game/session     (active, allocated 2026-04-18)
  ```
  One line per hit, port suffixed with `/tcp` or `/udp`.
- `spout ls` adds a `PROTO` column (plain-text and TUI). UDP rows
  show `udp`; TCP rows show `tcp`. We do not hide TCP because it
  makes the columns asymmetric and forces the reader to infer.
- `spout env` continues to emit `KEY=VALUE` lines; the env-var name
  derivation (§9 of the PRD) gets a protocol suffix when a service
  has a non-default protocol? **Open question** — see below.
- `spout alloc --udp coredns-udp` prints the port on stdout same as
  TCP. stdout contract preserved.
- Error messages from `PortAlreadyClaimed` and `PortInUse` gain the
  protocol: `port 5432/udp is already in use by the operating system`.

### Mutation-boundary table (PRD §3.2)

No change. `alloc/set/rm` still mutate; `get/ls/env/check/whois` are
still read-only. `--udp` is a selector on the existing commands, not
a new command.

### Tests

New coverage (outline, not exhaustive):

- `allocator`: UDP-only free-port detection, UDP bind-check finds a
  process's live UDP socket, a port bound on TCP is still free on
  UDP and vice versa.
- `registry`: v1 file loads with every entry defaulted to TCP, next
  write upgrades to v2, v2 file round-trips with mixed TCP/UDP
  entries.
- `commands::alloc --udp`: basic allocation, idempotency, UDP port
  doesn't collide with a same-numbered TCP registration.
- `commands::set --udp`: conflict check ignores opposite-protocol
  registrations.
- `commands::whois`: multi-protocol hits both listed, ordering
  stable.
- `commands::check --udp`: bound UDP port returns false, free
  returns true.

Expect test count to grow from 117 → ~135. No new test _modules_;
additions stay inside existing `#[cfg(test)] mod tests` blocks.

## Size impact

Estimated diff, using line counts from allocator/registry explored
on current HEAD:

- `src/allocator.rs` (currently 206): +40 lines. `Protocol` dispatch +
  UDP probe + signature update.
- `src/registry.rs` (currently 385): +10 lines. `Protocol` enum +
  field + version bump. **Tight against the 400-line cap** — if the
  cap is hit, `Protocol` moves to its own tiny module
  (`src/protocol.rs`).
- `src/commands.rs` (currently 337): +25 lines. Pass `Protocol`
  through `alloc`, `set`, `check`, update `whois` for multi-match
  output.
- `src/cli.rs` (currently 177): +10 lines. `--udp` flags on four
  commands.
- `src/format.rs` / `src/tui.rs`: +15 lines for the PROTO column.
- `src/error.rs` (currently 111): +5 lines. Protocol-aware error
  strings on `PortAlreadyClaimed` and `PortInUse`.

Total: ~105 lines across src, plus ~80 lines of tests. None of the
files blow the 400-line cap, though `registry.rs` at 385 is the one
to watch — the `Protocol` escape hatch (separate module) is ready if
needed.

## Open questions for review

1. **`spout env` and UDP.** If a project has both `coredns-tcp` and
   `coredns-udp`, `spout env` emits `COREDNS_TCP_PORT=...` and
   `COREDNS_UDP_PORT=...` naturally from the service name rule (no
   special-casing needed). If a project has only `coredns` on UDP,
   it emits `COREDNS_PORT=...`. **Should the env-var name encode the
   protocol when the service name doesn't already?** Recommend no —
   the service name is the contract, env-var derivation stays
   protocol-blind. Users who want the protocol in the name say so
   in the service name. Revisit if feedback disagrees.

2. **`whois` always multi-protocol, or add `--tcp`/`--udp` filters?**
   Recommend always-multi. The point of `whois` is "surprise me —
   what's here?" Filtering defeats the use case.

3. **Default protocol if `--udp` is absent.** Recommend TCP, because
   the overwhelming majority of services are TCP and 0.1.0's entire
   installed base is TCP. Every existing command behaves identically
   without the flag — full backward compatibility at the CLI surface.

## Stage shape

One stage, roughly Stage 6-sized. Plan + learning doc per CLAUDE.md
§Process. Commit breakdown (imagined):

1. `feat(registry): Protocol enum, bump schema to v2`
2. `feat(allocator): UDP OS probe, protocol-dispatched is_port_free_on_os`
3. `feat(commands): --udp on alloc/set/check, multi-protocol whois`
4. `feat(cli): --udp flags`
5. `feat(format,tui): PROTO column on ls and whois`
6. `docs: CHANGELOG, README, PRD update`

## Verification

1. Fresh v1 registry read by v2 binary: all entries TCP, first
   mutating command upgrades `version` to 2.
2. v2 registry read by v1 binary: exits code 4 with "unsupported
   version". Confirms migration safety.
3. `spout alloc --udp dns` and `spout alloc dns-tcp` both succeed,
   both write to the same registry with different protocols, both
   visible in `spout ls` with distinct `PROTO` values.
4. Holding a `UdpSocket` at port P and running `spout check --udp P`
   returns exit 1. Same port free for TCP: `spout check P` returns 0.
5. `spout whois P` with TCP and UDP registrations on the same number
   surfaces both, sorted stably.
6. Exit codes unchanged from the PRD table. The only new exit-code
   surface is the error text, which now includes the protocol.

## Next step

Decide on open question (1) about env-var naming, approve the stage
shape, then open a `docs/planning/04-planning.md` (or whatever the
next stage number is, pending the planning-doc gap decision from the
last pass).
