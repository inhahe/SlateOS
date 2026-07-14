# TCP/IP → userspace migration (roadmap: "Move to userspace service")

Strategy: **Path B** — move the protocol *stack* into a userspace `netstack`
daemon, keep a thin capability-gated kernel NIC shim. Full userspace NIC
*drivers* (Path A, IOMMU-sandboxed) are a separate, later, optional track. See
`design-decisions.md` §63 for the decision + rationale.

The kernel-resident stack (`kernel/src/net/`) keeps working throughout; the
daemon is built alongside and cut over only at parity. Each phase is
independently testable.

## Guiding constraints

- **Capabilities, not ambient authority.** Raw-frame access is gated by an
  unforgeable handle; only the `netstack` daemon (and, later, explicitly
  privileged tools like a packet sniffer) may open it.
- **Bytes, not UTF-8**, on every boundary (frames, addrs).
- **Perf** (net is perf-critical): batch raw-frame TX/RX (io_uring-style,
  many frames/syscall); shared-memory rings for the socket data path. Measure
  vs. the current in-kernel numbers before cutover; don't regress targets.
- **Reversible increments.** Nothing deletes the in-kernel stack until Phase 5.

## Phases

### Phase 1 — kernel raw-frame boundary  [x] landed 2026-07-14 (commit 89f37fb05)
Expose the NIC to userspace without moving the driver. Implemented:
- `net::raw` shim (`kernel/src/net/raw.rs`): exclusive NIC claim with atomic
  owner PID + self-healing reclamation on owner death; `transmit`/`receive`.
- `SYS_NET_RAW_OPEN/TX/RX/CLOSE` (865-868), capability-gated on the new
  `ResourceType::NetRaw`, owner-checked, user-pointer validated, frame-size
  bounded (14..=1522).
- `net::poll()` skips the physical-NIC drain while a raw owner holds the claim
  (exclusive-ownership model chosen over a promiscuous tap — simplest correct
  first step; the in-kernel stack stays the active path until a daemon claims).
- fork/ipc-cleanup arms: NetRaw is non-inheritable and needs no fd cleanup.

Deferred to later increments (not blocking Phase 2):
- Batched TX/RX (io_uring-style) — single-frame per syscall for now.
- `sys_net_if_query` (MAC/MTU enumeration) — Phase 2 reuses existing
  `SYS_NET_IF_INFO` (842) for the MAC; MTU is fixed at 1500 for now.
- End-to-end ARP send/recv test — arrives with the Phase 2 daemon that drives
  the raw path. This commit's validation is: build clean + boot test confirms
  the `poll()` gate did not regress in-kernel networking (no raw owner present).

### Phase 2 — `netstack` daemon skeleton  [ ] not started
- New `netstack/` userspace crate (Rust, `no_std`? — runs as a normal user
  process, so `std`-on-SlateOS/POSIX where available; reuse kernel net modules'
  logic by moving to a shared `no_std` protocol crate where practical).
- Open raw iface, run a poll/event loop, answer ARP + ICMP echo (ping
  responder). Proves the loop end-to-end against QEMU.

### Phase 3 — port protocol layers  [ ] not started
Move parsers/state machines into the daemon (or a shared crate): Ethernet, ARP,
IPv4, IPv6, ICMP(v6), UDP, TCP, DHCP(v6), DNS, fragmentation, firewall/conntrack.
Most of `kernel/src/net/*.rs` is privilege-free and moves largely as-is.

### Phase 4 — socket syscalls → IPC  [ ] not started
Redirect `SYS_TCP_*` / `SYS_UDP_*` / `SYS_DNS_RESOLVE` etc. to IPC calls into
`netstack` (shared-memory data path for bulk transfer). POSIX socket layer
delegates to the daemon.

### Phase 5 — cut over + delete kernel stack  [ ] not started
Flip default from in-kernel to daemon; remove `kernel/src/net/` protocol modules;
keep only the thin NIC shim + raw-frame syscalls. Update roadmap item to `[x]`.

## Status log
- 2026-07-14: Decision recorded (§63, Path B). Plan drafted. Starting Phase 1.
