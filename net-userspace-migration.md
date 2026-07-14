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

### Phase 2 — `netstack` daemon skeleton  [x] landed 2026-07-14
- New `services/netstack/` bare-metal daemon (`no_std`/`no_main`, hand-rolled
  syscall wrappers — same shape as the other `services/*`; a `std`-on-SlateOS
  port and a shared `no_std` protocol crate come with Phase 3's larger port).
- Opens the raw iface via `SYS_NET_RAW_OPEN`, queries `SYS_NET_IF_INFO` for
  IP/MAC/gateway, runs a raw-frame poll loop, and speaks two protocols wholly
  in userspace: **ARP** (broadcasts a request for the gateway to prove TX+RX,
  and answers inbound requests for our IP) and **ICMP echo** (unicasts a ping
  reply back to the requester's L2 address).
- Validated end-to-end in QEMU by a kernel ring-3 self-test
  (`spawn::self_test_userspace_netstack`, wired in `main.rs`): spawns the real
  daemon ELF holding a single `NetRaw` capability, and asserts a clean exit
  after the gateway ARP round-trip. Boot log:
  `[netstack] claimed raw NIC → sent ARP request → ARP reply: gateway resolved
  → released raw NIC → SUCCESS`. Skips gracefully when there's no network.
- Confirmed no regression: after the daemon releases the claim, `net::poll()`
  resumes and the rest of the boot self-tests run normally (BOOT_OK reached).
- Deferred to Phase 3: moving the real protocol *parsers/state machines* into
  the daemon (this skeleton hand-builds only ARP/ICMP frames); a shared
  `no_std` protocol crate; batched raw TX/RX.

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
