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

### Phase 3 — port protocol layers  [-] in progress
Move parsers/state machines into the daemon (or a shared crate): Ethernet, ARP,
IPv4, IPv6, ICMP(v6), UDP, TCP, DHCP(v6), DNS, fragmentation, firewall/conntrack.
Most of `kernel/src/net/*.rs` is privilege-free and moves largely as-is.

**Increment 1 — shared `netproto` crate + netstack cutover  [x] landed 2026-07-14**
Created `netproto/` at the repo root: a dependency-free, `no_std`,
`#![forbid(unsafe_code)]` crate holding the privilege-free wire-format logic as
a single source of truth for both the daemon and (later) the kernel stack.
Modules landed this increment:
- `checksum` — RFC 1071 Internet checksum (`internet`, plus `accumulate`/`fold`/
  `internet_continue` for split pseudo-header + payload sums).
- `ethernet` — Ethernet II `Frame::parse` + `write_header`, EtherType consts,
  broadcast/multicast predicates.
- `arp` — RFC 826 `Packet` parse/serialize, `request()` and `reply_to()` frame
  builders (broadcast request; unicast reply).
- `ipv4` — RFC 791 fixed-header parse (verifies the header checksum, clamps the
  payload to `total_len`, IHL>5 options tolerated) + `Builder::build_header`.
- `icmp` — ICMPv4 echo `Echo::parse` + `write_echo`/`reply_to` (checksum-verified).

22 host unit tests pass (`cd netproto && cargo test --target x86_64-pc-windows-gnu`);
crate also builds clean for `x86_64-unknown-none`. Added `netproto` to the
workspace `exclude` list. `services/netstack` now takes a path dependency on it
and its hand-rolled Ethernet/ARP/IPv4/ICMP framing + checksum (~200 lines) was
deleted in favour of the shared parsers/builders. Re-validated end-to-end in
QEMU: the daemon claimed the NIC, ARP-resolved the gateway, and exited SUCCESS
on the netproto code path; BOOT_OK, no regression.

**Increment 2 — UDP  [x] landed 2026-07-14**
`udp` module: RFC 768 datagram parse/write over IPv4 with the pseudo-header
checksum (built on `checksum::accumulate`/`fold`). Honours the RFC 768 zero-
checksum conventions and validates the length field. (28 tests.)

**Increment 3 — TCP segment header  [x] landed 2026-07-14**
`tcp` module: RFC 793 segment header parse/write over IPv4 with the pseudo-
header checksum — ports, seq/ack, flag bits, window, urgent pointer; options
tolerated. Wire format only; the connection state machine stays with whoever
owns per-connection state. (33 tests.)

**Increment 4 — IPv6 base header  [x] landed 2026-07-14**
`ipv6` module: RFC 8200 base-header parse/build + `pseudo_header_sum()` for
upper-layer checksums. Extension headers left to the caller; no L3 checksum.
(37 tests.)

**Increment 5 — DNS  [x] landed 2026-07-14**
`dns` module: `write_query()` (standard recursive A/AAAA query) + `Message`
answer-section walk that transparently follows compression pointers
(RFC 1035 §4.1.4); `first_ipv4()`/`first_ipv6()` extract the first address
record. Allocation-free; the answer iterator stops rather than panics on
truncated/malformed records. Foundation for `SYS_DNS_RESOLVE`. (45 tests.)

**Increment 6 — DHCPv4 client  [x] landed 2026-07-14**
`dhcp` module: `build_discover()`/`build_request()` + `Message` option TLV
walk (msg type, subnet mask, router, DNS, lease, server id) for the
DISCOVER→OFFER→REQUEST→ACK exchange. Lets the daemon own interface config
in later phases. (51 tests.)

**netproto core L2–L4 coverage is now essentially complete** for the
daemon's needs: `checksum, ethernet, arp, ipv4, ipv6, icmp, udp, tcp, dns,
dhcp` (10 modules, 51 host tests, builds for `x86_64-unknown-none`).

Remaining Phase 3 increments are specialized and can land as-needed:
ICMPv6/NDP (IPv6 ARP analogue), IPv4/IPv6 fragmentation reassembly, and
firewall/conntrack. Note: migrating the *kernel* stack (`kernel/src/net/*.rs`)
onto `netproto` is largely throwaway since Phase 5 deletes that stack — the
priority is growing the daemon on `netproto` toward the Phase 4 socket-IPC
cutover, not retrofitting the doomed in-kernel modules.

### Phase 4 — socket syscalls → IPC  [-] in progress
Redirect `SYS_TCP_*` / `SYS_UDP_*` / `SYS_DNS_RESOLVE` etc. to IPC calls into
`netstack` (shared-memory data path for bulk transfer). POSIX socket layer
delegates to the daemon.

**Transport decided (§64): the kernel Service Registry (`ipc/service.rs`).**
The daemon `register`s a service name (`net.stack`); the kernel-side syscall
handler `connect`s to get a client channel endpoint; request/reply ride
`channel::Message` byte payloads. No new IPC mechanism needed. A shared-mem data
ring is added later for TCP/UDP bulk streaming; the one-shot control path
(starting with DNS) uses channel messages.

**NIC-ownership sequencing constraint (§64): the raw-NIC claim is exclusive.**
`sys_net_raw_open` grants an *exclusive* claim and `net::poll()` skips draining
the physical NIC while a raw owner holds it. A persistent daemon owning the NIC
would starve the still-live kernel stack's RX. So Phase 4 is validated with
**bounded self-tests** (Phase-2 style: claim NIC → register → serve one request
→ release), NOT a permanent daemon takeover. Persistent cutover is deferred to
Phase 5, where the kernel stack is deleted and the exclusive claim becomes
correct rather than a conflict.

**Kickoff groundwork (surveyed 2026-07-14).** The socket-syscall ABI the daemon
must serve already exists and stays stable across the cutover (Phase 5 keeps
these numbers as thin forwarders):
- **TCP** `SYS_TCP_*` 800–808 (connect/send/recv/close/bind/accept/
  close_listener/abort/peer_addr), plus status/tuning 840–855
  (list, listener_list, poll_status, listener_ready, shutdown, info,
  set_nodelay, set_keepalive[_params]).
- **UDP** `SYS_UDP_*` 810–817 (bind/send/recv/close/mcast_join/mcast_leave/
  connect/local_port), plus rx_ready 847 / rx_front_bytes 848.
- **DNS** `SYS_DNS_RESOLVE` 820 (→ `netproto::dns`).
- (`SYS_SOCKETPAIR_*` 300–310 are AF_UNIX-style local pairs, unrelated to the
  IP stack — leave them alone.)

**Proposed architecture (forwarder, ABI-stable — matches §63 Path B):**
each `SYS_TCP_*`/`SYS_UDP_*`/`SYS_DNS_RESOLVE` handler stops driving the
in-kernel stack and instead marshals a request onto a **channel** (the OS's
mandated primary IPC) to the `netstack` daemon, then blocks on the reply.
- Control ops (connect/bind/close/accept/status) → small fixed request/reply
  messages over the channel.
- Bulk data (send/recv) → **shared-memory ring** (io_uring-style: the design
  spec already calls for zero-copy IPC that moves pages, not copies), with the
  channel carrying only submission/completion notifications. Avoid a per-byte
  kernel→daemon copy.
- Daemon side: a request dispatcher on the channel drives per-connection TCP
  state machines built on `netproto` (TCP/UDP/IP framing) over the existing
  `SYS_NET_RAW_*` TX/RX path it already owns.

**Open design point to weigh before building (candidate for `open-questions.md`
if the operator wants input):** forwarding every socket op through
kernel→daemon IPC adds a round-trip vs. today's in-kernel fast path. The
microkernel-purity win (operator already chose Path B in §63) is the premise,
but the *data-path* design (shared-mem ring granularity, how `recv` blocking
maps onto channel wait, batching) is where the latency is won or lost — that's
the first real Phase 4 task and the one most worth getting right per the
perf targets (IPC round-trip < 2µs).

**First concrete Phase 4 step:** define the netstack request/reply message
schema + the shared-mem ring layout in a small shared module (likely alongside
`netproto`, or a sibling `netipc` crate so both kernel forwarders and the
daemon share the wire format), then implement `SYS_DNS_RESOLVE` end-to-end as
the simplest one-shot op (no per-connection state) to prove the channel path
before tackling TCP/UDP streaming.

### Phase 5 — cut over + delete kernel stack  [ ] not started
Flip default from in-kernel to daemon; remove `kernel/src/net/` protocol modules;
keep only the thin NIC shim + raw-frame syscalls. Update roadmap item to `[x]`.

## Status log
- 2026-07-14: Decision recorded (§63, Path B). Plan drafted. Starting Phase 1.
- 2026-07-14: Phase 1 (raw-frame boundary) + Phase 2 (netstack daemon skeleton)
  landed. Phase 3 increment 1: `netproto` shared crate created; netstack cut
  over onto it (hand-rolled framing deleted). Boot-validated end-to-end.
- 2026-07-14: Phase 3 increments 2–6 landed — `netproto` grew UDP, TCP, IPv6,
  DNS, DHCPv4 (10 modules, 51 host tests). Core L2–L4 coverage complete.
  Surveyed the socket-syscall ABI and drafted the Phase 4 forwarder
  architecture (channel control + shared-mem data ring; DNS_RESOLVE first).
- 2026-07-14: Phase 4 increment 1 landed — **DNS resolve over IPC, end-to-end.**
  Transport + NIC-ownership constraint recorded (§64). The `netstack` daemon
  gained a `serve-dns` mode: it `register`s `net.stack` (Service Registry),
  ARP-resolves the next hop, and answers `[OP_RESOLVE_A|hostname]` requests by
  doing a real DNS-over-UDP query on its raw NIC via `netproto` (dns/udp/ipv4),
  replying `[status|ip]`. A bounded kernel self-test (`self_test_netstack_dns_ipc`)
  spawns the daemon (NetRaw+Service caps), waits for registration, connects,
  and resolves `example.com`. Boot-validated: kernel→daemon→kernel round-trip
  returned 172.66.147.243. Daemon releases the NIC at its idle deadline, so the
  in-kernel stack stays the live path until Phase 5. Next: TCP/UDP control ops
  + shared-mem data ring; then wire the real `sys_dns_resolve` forwarder.
