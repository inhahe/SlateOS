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

### Phase 1 — kernel raw-frame boundary  [ ] not started
Expose the NIC to userspace without moving the driver:
- `sys_net_raw_open(if_index, flags) -> raw_handle` (capability-checked).
- `sys_net_raw_tx(handle, frame_ptr, len)` — single + batched variant.
- `sys_net_raw_rx(handle, buf_ptr, cap, flags) -> len` — blocking/non-blocking,
  batched variant.
- `sys_net_if_query()` — enumerate interfaces (index, name, MAC, MTU, flags).
- RX demux: while the in-kernel stack still runs, a raw handle must get a *copy*
  of frames (promiscuous tap) OR claim exclusive ownership of the NIC. Decision
  for Phase 1: raw handle = **exclusive** claim of one interface (the daemon owns
  it); the in-kernel stack binds the other/none. Simplest correct first step.
- Tests: userspace sends an ARP request via `raw_tx`, receives the reply via
  `raw_rx`; `if_query` returns the virtio-net MAC/MTU.

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
