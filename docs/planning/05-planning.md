# Stage 5 — UDP support

## Goal

Add UDP as a first-class protocol alongside TCP. Rare but real need —
DNS, QUIC dev stacks, some game servers, mDNS-alike tooling. Today
`spout alloc` can hand out a port a stopped UDP container owned, then
collide on its next startup. The exact failure mode spout was built
to prevent, still open for UDP services.

Design is specified in full in `docs/proposals/udp-support.md`
(commit `c45950b`). The three open questions from that proposal all
resolved as recommended:

- Env-var naming stays protocol-blind (service name is the contract).
- `whois` is always multi-protocol — no `--tcp`/`--udp` filters.
- TCP remains the default when `--udp` is absent; zero change to
  existing invocations.

This doc is the execution plan. For the *why* of each decision, read
the proposal.

## Scope

- `spout alloc --udp <service>` allocates from 20000–32767 probing UDP.
- `spout set --udp <service> <port>` and `spout check --udp <port>`
  take the same flag.
- `spout whois <port>` surfaces every registration for that port
  number across both protocols.
- Registry schema bumps to v2; v1 files read transparently with
  protocol defaulted to Tcp, upgraded to v2 on the next mutation.
- `spout ls` and the TUI grow a `PROTO` column.
- Service name stays unique per project regardless of protocol — if
  a project needs both, use two service names (`coredns-tcp`,
  `coredns-udp`).

Explicitly out of scope: SCTP/DCCP, dual-bind services on one name,
Windows, changing `spout get` to take a protocol.

## Commit sequence

TDD per CLAUDE.md. Tests before implementation in every commit; fmt,
clippy, and test suite green between commits.

1. **`feat(registry): Protocol enum, bump schema to v2`** — `Protocol`
   enum, `protocol: Protocol` field on `Entry` and `HistoryEntry`,
   `CURRENT_VERSION = 2`, `read()` accepts v1 or v2 and defaults
   missing protocol to Tcp. Tests: v1-reads-as-tcp, v1-upgrades-on-write,
   mixed round-trip, v3-errors.

2. **`feat(allocator): UDP OS probe, protocol-dispatched is_port_free_on_os`** —
   `is_port_free_on_os(port, protocol)`, split into
   `is_tcp_port_free` / `is_udp_port_free`, `UdpSocket::bind` probe
   on `0.0.0.0` and `[::]` (gated by existing IPv6 availability
   cache). Tests: udp-free, udp-bound, tcp-and-udp-independent,
   mixed-protocol probe_bound_ports.

3. **`feat(registry): protocol-aware is_port_claimed`** —
   `is_port_claimed(port, protocol)` filters by both fields.
   `history_for_port` unchanged (whois still multi-protocol).
   Tests: tcp-claimed-not-udp, same-protocol-returns-project.

4. **`feat(commands,cli): --udp on alloc/set/check, multi-protocol whois`** —
   clap `#[arg(long)] udp: bool` on `Alloc`, `Set`, `Check`. Not on
   `Whois`. Command handlers thread `Protocol` through. `whois`
   returns `Vec<String>`, one line per match, formatted
   `port/proto: project/service ...`. `SpoutError::PortAlreadyClaimed`
   and `PortInUse` gain protocol in their display strings. Tests:
   alloc-udp-picks-free, alloc-udp-ignores-tcp, set-udp-cross-protocol,
   check-udp-bound, whois-both-protocols, whois-ordering.

5. **`feat(format,tui): PROTO column on ls, proto-aware whois output`** —
   `format::all` and `format::project_block` render `PROTO`; TUI table
   matches. TCP rows show `tcp` (no hiding — asymmetric columns
   confuse). Tests: format golden for mixed list.

6. **`docs: CHANGELOG, README, PRD, llms.txt for UDP`** — Unreleased
   entry, short README UDP section, remove "UDP bind-testing" from
   PRD §18 Future Work, `llms.txt` paragraph for agents.

## Files

Line budgets under the 400-line cap:

| File | Now | After | Headroom |
|---|---|---|---|
| `src/registry.rs` | 385 | ~395 | 5 (tight — extract `Protocol` to `src/protocol.rs` if needed) |
| `src/allocator.rs` | 206 | ~245 | 155 |
| `src/commands.rs` | 337 | ~360 | 40 |
| `src/cli.rs` | 177 | ~190 | 210 |
| `src/error.rs` | 111 | ~115 | 285 |
| `src/format.rs` | 114 | ~130 | 270 |
| `src/tui.rs` | 316 | ~335 | 65 |

Test count expected to grow from 117 to ~135.

## Verification

1. `cargo test` green after every commit.
2. Post-Commit 1: hand-craft a v1 `~/.spout-test.json` (`version: 1`,
   one entry with no `protocol` field), read with new binary, verify
   in-memory `Protocol::Tcp`. Run a mutating command, verify on-disk
   `version: 2` and `"protocol": "tcp"` appears.
3. Post-Commit 4: two-terminal test —
   `SPOUT_REGISTRY=/tmp/s.json spout alloc --udp dns` and
   `SPOUT_REGISTRY=/tmp/s.json spout alloc dns-tcp`, confirm both
   registered, both visible in `ls`, `whois <shared-port>` lists both.
4. End-to-end UDP probe: `nc -u -l <port>` in one shell,
   `spout check --udp <port>` in another — exit 1. Kill nc, rerun —
   exit 0.
5. Final gate: `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`,
   `cargo test`, `wc -l src/*.rs` all green.

## Risks

- `registry.rs` at 385/400 is the tightest. If adding the enum and
  fields trips the cap, extract `Protocol` into `src/protocol.rs` —
  already the planned escape hatch.
- `UdpSocket::bind` inherits the same TOCTOU race as TCP; same
  mitigation (the allocator retries naturally, the user's `docker
  compose up` fails loud if they lose the race after alloc).
- Privileged ports (<1024) will bind-fail with `EACCES` on UDP and
  read as "not free." Pre-existing behaviour for TCP; spout never
  allocates below 20000 anyway, so only affects `spout check --udp 53`
  for the curious. Document in the learning doc if it surprises.

## Deferred to learning doc

The usual — what the plan got wrong, commits that ended up merged or
split differently, any test patterns worth calling out.
