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

### Phase 5 — cut over + delete kernel stack  [-] in progress
Flip default from in-kernel to daemon; remove `kernel/src/net/` protocol modules;
keep only the thin NIC shim + raw-frame syscalls. Update roadmap item to `[x]`.

**Cutover strategy (design-decisions.md §66, operator-approved 2026-07-14):**
Q22a → **Option C, phased deletion** (delete the L2–L4 core first, re-home each
app-protocol server/client — ssh/httpd/ftp/… — to userspace in its own follow-up).
Q22b → **(ii) staged cutover** behind a default-off boot switch; prove parity in
QEMU with the switch on, flip the default, then delete.

**Key discovery (2026-07-14):** the Linux socket syscalls (`sys_socket`/`connect`/
`sendto`/`recvfrom`/… in `kernel/src/syscall/linux.rs`) are today **pure errno-
gating stubs that return `ENOSYS`** — there is *no* AF_INET socket object and no
dispatch into `kernel/src/net/`. The in-kernel `net/` stack is reached only by the
in-kernel app servers via their own internal API, not via the POSIX/Linux socket
ABI. So "forward the socket syscalls to the daemon" is really **implementing
AF_INET/AF_INET6 sockets in the Linux ABI for the first time, backed by the
daemon** — and the §64 switch is fundamentally about *NIC ownership at boot*.

**Increment plan (each buildable + boot-testable):**
- **5.4 — persistent daemon lifetime + kernel ring-client refactor.** Extract the
  connect/send/recv/close ring-driving client currently inlined in the
  `spawn.rs` self-tests into a reusable kernel `netstack` client module. Add the
  `net.userspace` boot switch (default **off**). No syscall behavior change yet.
- **5.5 — AF_INET SOCK_STREAM sockets over the daemon (switch-gated). [DONE]**
  Give the
  kernel a socket-fd object (in the Linux fd table) backed by a daemon ring
  connection; wire `sys_socket`/`connect`/`sendto`/`recvfrom`/`read`/`write`/
  `close` for AF_INET SOCK_STREAM to it *when the switch is on*. Validate with a
  real ring-3 Linux process doing `socket()`/`connect()`/`write()`/`read()` to
  fetch HTTP, run inside the bounded daemon window. Switch off → unchanged
  (`ENOSYS`), in-kernel stack still owns the NIC.
- **5.6 — persistent daemon spawn at boot + NIC ownership handoff. [DONE]** When
  the switch is on, the boot path launches the daemon persistently at boot (new
  `serve-net` daemon mode + `proc::spawn::run_persistent_netstack`); it claims the
  NIC for the system's lifetime and the in-kernel resident stack's bounded
  self-tests are skipped (avoiding §64 exclusive-raw-NIC contention). Proven in
  QEMU with the switch on: the daemon spawned at boot, claimed the NIC, registered
  `net.stack`, and DNS (`example.com`) + TCP (HTTP over the daemon) + UDP (DNS
  datagram over the daemon) parity all passed (serial 1800–1810). Switch off →
  unchanged: no persistent daemon, in-kernel stack owns the NIC, bounded netstack
  self-tests run. Boot self-test socket assertions in `syscall/linux.rs` were made
  switch-aware (`assert_stream_socket_gate`) so they hold in both cutover states.
- **5.7 — flip the default** to the daemon once parity holds.
- **5.8 — delete the L2–L4 core** (`ethernet, arp, ipv4/ipv6, icmp/icmpv6, tcp,
  udp, dns, dhcp, frag, interface, ndisc`), keeping the NIC shim; then re-home
  each app-protocol module (ssh/httpd/…) to userspace in its own follow-up,
  deleting it from `kernel/src/net/` as it lands (Q22a → C).

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
- 2026-07-14: Phase 4 increment 2 landed — **reverse DNS (PTR) over IPC.**
  Added permanent name-*decoding* to `netproto::dns`: `read_name` (compression-
  pointer decompression with a 128-jump loop guard), `Message::first_ptr`,
  `write_ptr_query` (`d.c.b.a.in-addr.arpa`) + `TYPE_PTR` — 9 new host tests
  (16 total in dns). The daemon serves `[OP_RESOLVE_PTR|4 IP bytes]` →
  `[status|dotted-name]` via a shared DNS transport (`tx_dns_query` /
  `dns_response_msg`, refactored out of `resolve_dns`). Kernel self-test adds a
  second round-trip reverse-resolving 8.8.8.8. Boot-validated end-to-end: the
  daemon decoded `dns.google` and the kernel logged `PTR name = dns.google`.
- 2026-07-14: Phase 4 increment 3 landed — **shared `netipc` schema crate.**
  Extracted the kernel↔daemon control-message wire format (the previously-inline
  opcodes/status codes + hand-rolled encode/decode, which had drifted into magic
  constants duplicated in the daemon *and* the kernel self-test) into a new
  dependency-free `no_std` `#![forbid(unsafe_code)]` `netipc` crate — the single
  source of truth both sides link, mirroring `netproto`. It exposes `OP_*`/`ST_*`
  consts, `Request` (parse), `encode_resolve_a`/`encode_resolve_ptr`,
  `encode_ok_ipv4`/`encode_ok_name`/`encode_fail`, and typed reply decoders
  `parse_ipv4_reply` (`Ipv4Reply`) / `parse_name_reply` (`NameReply`); 9 host
  tests. Daemon `handle_request` and both kernel round-trips now go through it;
  the kernel gained its first `netipc` path-dependency (builds clean for the
  custom target). This is the "first concrete Phase 4 step" the plan called for,
  now that two ops justify the abstraction. Boot-validated: A resolve + PTR
  `dns.google` still decode end-to-end.
- 2026-07-14: Phase 4 increment 4 landed — **one-shot TCP fetch over IPC.** The
  daemon gained a minimal userspace TCP client (`tcp_fetch`/`send_tcp`/
  `recv_tcp_seg` + a generalized `send_ipv4` framing helper) built on
  `netproto::tcp` (wire-format `Builder`/`Segment`): SYN/SYN-ACK/ACK handshake,
  in-order data reception with cumulative ACKs, bounded SYN + request-payload
  retransmit, and a graceful FIN close. It serves `netipc::Request::TcpFetch
  {ip, port, payload}` → `[status|response-bytes]` (new `OP_TCP_FETCH` +
  `encode_ok_bytes`/`parse_bytes_reply` in `netipc`, 4 new host tests, 13 total).
  Kernel self-test (`netstack_tcp_fetch_roundtrip`) reuses the A-resolved
  example.com address, issues an HTTP/1.0 HEAD, and validates the reply is an
  HTTP status line. Boot-validated end-to-end: kernel→daemon→real TCP handshake
  to example.com:80 (104.20.23.154)→HTTP HEAD→`HTTP/1.1 200 OK` back over IPC.
  Client limitations (no reassembly / congestion control / outbound segmentation
  / multi-socket; 512 B response cap) are documented in known-issues.md
  (D-NETSTACK-TCP-MINIMAL) — they all resolve with the Phase-5 shared-mem data
  ring + full per-connection state machine. Next: UDP send/recv control op, then
  the shared-mem data ring + wiring the real `sys_dns_resolve`/`sys_tcp_*`
  forwarders ahead of Phase 5 cutover.
- 2026-07-14: Phase 4 increment 5 landed — **generic UDP exchange over IPC.**
  Rounds out core L4 control-path coverage: the daemon serves
  `netipc::Request::UdpExchange {ip, port, payload}` → `[status|response-bytes]`
  (new `OP_UDP_EXCHANGE` + `encode_udp_exchange`, shared `[op][ip:4][port:2]
  [payload]` layout factored with `encode_tcp_fetch`; 15 netipc host tests). The
  daemon `udp_exchange` sends one datagram via the generalized `send_ipv4` helper
  and returns the first matching response datagram — the generic sibling of the
  DNS-specific resolve op (suits NTP/STUN/custom request-response UDP). Kernel
  self-test (`netstack_udp_dns_roundtrip`) sends a static DNS/A query blob for
  example.com to the configured resolver on :53 and validates the returned
  datagram is a DNS *response* (echoed ID + QR bit) — no DNS logic duplicated in
  the kernel, just the generic UDP path exercised. Boot-validated end-to-end
  (`netstack UDP-exchange-over-IPC (ring 3): OK — DNS response datagram returned`;
  one flaky boot hit the pre-existing intermittent prctl-batch spawn-dispatch
  race — see known-issues.md — and an immediate re-run passed in 88s with all
  four ops green). Control-path L4 coverage (DNS A/PTR, TCP fetch, UDP exchange)
  is now complete; next is the Phase-5 shared-memory data ring for streaming +
  wiring the real socket-syscall forwarders.
- 2026-07-14: Phase 4 increment 6 landed — **shared-memory data-ring ABI**
  (`netipc/src/ring.rs`, wired via `pub mod ring;`). Defines the io_uring-style
  zero-copy bulk path that streaming `send`/`recv` will use instead of per-call
  channel copies (the control path stays on channel messages). One SHM region =
  header + SQ (kernel→daemon: connect/send/recv/close) + CQ (daemon→kernel:
  result + echoed `user_data`) + separate bulk data area; SQE/CQE carry only a
  `(data_off,data_len)` window so no stream bytes cross the channel. Fixed 32 B
  `Sqe` / 16 B `Cqe` with byte (de)serialization, free-running u32 indices with
  power-of-two `slot = index & (entries-1)` masking, four indices on separate
  cache lines (`HEADER_LEN=320`, 5 lines) to kill producer/consumer false
  sharing, and pure region-sizing/layout helpers. Module is deliberately
  mapping-agnostic and atomic-free — keeps `netipc` `no_std`, dependency-free,
  `#![forbid(unsafe_code)]`; the acquire/release atomics + SHM mapping live at
  the kernel/daemon integration sites (next increments). 10 new host tests
  (sqe/cqe round-trips, short-slice None, SPSC empty/full/wrap incl. u32-boundary
  wrap, slot-is-modulo, power-of-two, region layout, cache-line separation) —
  25 netipc tests total, all green (`cargo test --target x86_64-pc-windows-msvc`).
  Design rationale + the deferred recv-notification sub-choice (futex vs eventfd
  vs channel-signal) recorded in design-decisions.md §65. Next: wire the ring
  into the kernel (`SYS_SHM_CREATE` + map into the daemon) and the daemon
  (persistent per-connection TCP state machines driving the ring), validated
  under §64 bounded self-tests.
- 2026-07-14: Phase 4 increment 7 landed — **cross-address-space shared memory
  (`SYS_SHM_MAP`/`SYS_SHM_UNMAP`) + SHM-ping handshake self-test.** Shared memory
  was previously kernel-only (`shm::kernel_addr` HHDM view); a ring-3 process had
  no way to map a region. Added `SYS_SHM_MAP` (233) — reserves a user VA gap with
  a `Fixed` VMA, `ref_inc`s each backing frame, and maps them into the caller's
  page table (never executable; RW opt-in) — and `SYS_SHM_UNMAP` (234), which
  delegates to `sys_munmap` (SHM frames are allocator-owned, so the generic
  refcount-aware `free_frame` path drops the mapping's reference correctly). The
  frame refcount model makes every teardown ordering safe: mapping bumps refcount
  1→2, so `shm::close` or daemon exit each just decrements — the last reference
  (handle *or* mapping, in any order) frees the frames; no double-free, no leak.
  New netipc control op `OP_SHM_PING` (`[handle_le:8][size_le:4]`) + magics: the
  kernel creates a region, writes `SHM_PING_REQUEST_MAGIC` at offset 0, and asks
  the daemon to map it, verify that magic (proving it mapped the *same* frames),
  write `SHM_PING_RESPONSE_MAGIC` at offset 8, and unmap; the kernel then reads
  offset 8 back through its own view and checks the response magic — both
  directions verified ⇒ genuinely shared, not a private copy. 27 netipc host
  tests (2 new). Boot-validated: "netstack SHM-ping-over-IPC (ring 3): OK —
  cross-address-space SYS_SHM_MAP verified (daemon read kernel magic + kernel
  read daemon magic)", clean 84s boot with all five netstack ops green. This is
  the exact bootstrap the Phase-5 data ring uses to hand the daemon its
  SQ/CQ/data region; next increment builds the ring drivers (RingHeader init +
  atomic SQ/CQ producer/consumer over a mapped `netipc::ring` region) on top.
  Known limitation logged in known-issues.md: `SYS_SHM_MAP` does not verify the
  caller *owns* the handle (consistent with existing `SYS_SHM_SIZE`/`CLOSE`) —
  fine for the kernel↔trusted-daemon path, but wants a capability check before
  untrusted processes use SHM.
- 2026-07-14: Phase 4 increment 8 landed — **`netring`: the shared atomic SPSC
  driver over the `netipc::ring` ABI.** New `no_std` crate (`netring/`,
  workspace-`exclude`d, depends only on `netipc`) that adds the one piece the
  pure ABI deliberately omits: the *unsafe* Acquire/Release atomic accesses to
  the shared indices. `Ring::init` writes the header geometry and publishes the
  magic **last** (release-store after a release fence) so a peer that
  `Ring::attach`es and sees the magic is guaranteed to also see the geometry;
  `attach` Acquire-loads the magic, checks the version, and re-derives the
  canonical `sqe/cqe/data` offsets, rejecting any region whose stored offsets
  disagree or whose declared size overflows the mapping (a corrupt/hostile
  region can never make the driver read or write out of bounds). Hot-path
  `sq_push`/`sq_pop`/`cq_push`/`cq_pop` follow the SPSC discipline from §65:
  producer owns its tail (Relaxed) + observes the peer head (Acquire) + publishes
  the tail (Release); consumer symmetric on the head — so the entry bytes and, for
  `OP_SEND`, the data-area payload written before the push, are visible once the
  peer Acquire-loads the index. `write_data`/`read_data` are bounds-checked
  against the length-validated data area. All `unsafe` is isolated here (one
  audited crate) so the memory-ordering logic is written, reviewed, and tested
  once and linked verbatim into both the kernel forwarder and the daemon —
  `netipc` stays `#![forbid(unsafe_code)]`. 9 host tests (init/attach geometry,
  attach-rejects-uninitialised, power-of-two/overflow rejection, SQ/CQ
  push-pop round-trips, fill-reports-full, 1000-iteration wrap through a 2-slot
  ring, data-area bounds, and a full end-to-end echo: kernel `init` → submit
  `OP_SEND "ping"` → daemon `attach` → read/upper-case in place → `cq_push` →
  kernel `cq_pop` + reads back `"PING"`) — all green
  (`cargo test --target x86_64-pc-windows-msvc`); clippy clean; builds for
  `x86_64-unknown-none`. Design split recorded in design-decisions.md §65 (new
  "shared atomic driver in a separate crate" sub-choice). Next: wire `netring`
  into the kernel + daemon Cargo.toml, add a ring-echo control op (kernel creates
  SHM + `Ring::init` + submits an SQE; daemon maps + `Ring::attach` + processes +
  posts a CQE; kernel reaps the CQE), validated under a §64 bounded self-test +
  boot test.
- 2026-07-14: Phase 4 increment 9 landed — **`netring` wired end-to-end: the
  `OP_RING_ECHO` ring self-test drives the SQ/CQ across the address-space
  boundary.** Added `netring` as a path dep of both the kernel and the daemon
  (same crate on both sides ⇒ ring driver written/tested once). New netipc
  control op `OP_RING_ECHO` (`[handle_le:8][size_le:4]`, shared operand layout
  with `OP_SHM_PING` via new `parse_handle_size`/`encode_handle_size` helpers) +
  `Request::RingEcho` + `encode_ring_echo` + `RING_ECHO_USER_DATA`. Kernel side
  (`netstack_ring_echo_roundtrip` in spawn.rs): `shm::create(region_size(4,4,256))`,
  `Ring::init` through the HHDM view, `write_data` a fixed lowercase payload,
  `sq_push` one `OP_SEND` SQE stamped with `RING_ECHO_USER_DATA`, then ask the
  daemon to process; after `ST_OK`, `cq_pop` the completion and verify (a) echoed
  `user_data`, (b) `result` == payload length, (c) the data window now holds the
  upper-cased bytes. Daemon side (`ring_echo`/`ring_echo_process` in
  services/netstack): `SYS_SHM_MAP` the region RW, `Ring::attach`, `sq_pop`,
  `read_data` → ASCII-upper-case → `write_data`, `cq_push` the completion,
  `SYS_SHM_UNMAP`. This proves the whole zero-copy data path — kernel produces →
  daemon consumes/transforms → kernel reaps — with no socket bytes copied through
  the control channel (only the 13-byte handle+size request is). 29 netipc host
  tests (2 new: ring_echo_round_trip + short_ring_echo_request_is_none), all
  green; kernel + daemon build clean; clippy clean on both new crates and no new
  kernel warnings. Boot-validated: "netstack ring-echo-over-IPC (ring 3): OK —
  SQ/CQ driver verified (kernel submitted SQE + daemon transformed payload +
  kernel reaped CQE)", clean 84s boot with all six netstack ops green (DNS, TCP,
  UDP, SHM-ping, ring-echo, reverse-DNS). The ring transport is now proven; next
  Phase 4/5 work builds persistent per-connection TCP/UDP state machines that
  drive this ring for real streaming `send`/`recv`, replacing the one-shot
  `OP_TCP_FETCH`/`OP_UDP_EXCHANGE` control ops.
- 2026-07-14: Phase 4 increment 10 landed — **batched SQ drain + per-opcode
  completion dispatch (the io_uring submission model, cross-address-space).** The
  daemon's ring handler was a single `sq_pop`; it now **drains the whole SQ** in a
  loop, dispatching each SQE by opcode (`OP_NOP` → complete `result=0`; `OP_SEND`
  → upper-case the data window, `result=len`; unknown → `result=-1`) and posting
  one CQE per entry in FIFO order (`ring_echo_process` loop + `ring_send_transform`
  helper). The kernel self-test now submits a **3-SQE batch in one pass**
  (`OP_SEND` + two `OP_NOP`s, each with a distinct `user_data = base+index`) and
  reaps all three CQEs, asserting FIFO ordering (echoed `user_data` matches
  submission order), the expected per-op `result`, no stray extra completion, and
  the upper-cased payload. This proves the core io_uring value prop — many SQEs
  submitted/completed per round-trip — works across the address-space boundary,
  and is the mechanical foundation the real socket dispatch (connect/send/recv/
  close as distinct opcodes over the ring) will build on. Daemon + kernel build
  clean; clippy clean; boot-validated: "netstack ring-echo-over-IPC (ring 3): OK —
  SQ/CQ driver verified (kernel submitted 3-SQE batch + daemon drained SQ + kernel
  reaped 3 CQEs in order)", clean 85s boot with all six netstack ops green. Next:
  give the ring the real socket opcodes (`OP_CONNECT` w/ endpoint in `aux` via
  `pack_endpoint`, `OP_SEND`/`OP_RECV` streaming through the data window,
  `OP_CLOSE`) driving a live TCP transaction — the ring-native equivalent of the
  one-shot `OP_TCP_FETCH` control op, self-tested in the §64 bounded model.
- 2026-07-14: Phase 4 increment 11 landed — **real socket opcodes driving a live
  TCP fetch entirely over the ring (the Phase-4 capstone).** The daemon's
  monolithic one-shot `tcp_fetch` was refactored into a reusable stateful
  `TcpConn` struct (`connect`/`send`/`recv`/`close`), so a *single* TCP
  implementation now backs **both** the `OP_TCP_FETCH` control op (now a thin
  `TcpConn` wrapper — boot parity preserved) **and** the new ring path — no
  duplicated TCP client. New netipc control op `OP_RING_TCP`
  (`[handle_le:8][size_le:4]`, sharing the `parse_handle_size`/`encode_handle_size`
  helpers with `OP_SHM_PING`/`OP_RING_ECHO`) + `Request::RingTcp` +
  `encode_ring_tcp` (2 new host tests → 31 total, green). Daemon
  `ring_tcp`/`ring_tcp_process` (services/netstack): `SYS_SHM_MAP` RW,
  `Ring::attach`, then **drain the SQ driving one `Option<TcpConn>`** — `OP_CONNECT`
  (`unpack_endpoint(aux)` → `TcpConn::connect`), `OP_SEND` (read data window →
  `conn.send`), `OP_RECV` (`conn.recv` into scratch → `write_data` back into the
  window), `OP_CLOSE` (`conn.close`) — one CQE per SQE, then `SYS_SHM_UNMAP`.
  Kernel `netstack_ring_tcp_roundtrip` (spawn.rs): `Ring::init` a
  `region_size(8,8,1024)` region, `write_data` an HTTP/1.0 HEAD request at off 0,
  submit the 4-SQE `connect→send→recv→close` batch (endpoint packed into the
  connect SQE's `aux`, recv window at off 512), ask the daemon via one
  `OP_RING_TCP` message, reap all four CQEs in FIFO order (verifying echoed
  `user_data`), then `read_data` the recv window and check it's an `HTTP/` reply.
  **Bug found + fixed in the same increment:** making a *second* TCP connection
  per daemon lifetime (tcp_fetch then ring_tcp) reused an *identical 4-tuple*
  (fixed local port `0xC000` + fixed ISN + same server) — the server treated the
  second SYN as a stale TIME_WAIT duplicate and dropped it, so ring-TCP initially
  got no response while OP_TCP_FETCH succeeded. Proper fix: `TcpConn::connect` now
  rotates the ephemeral local port (`0xC000 | (seed_ipid & 0x3FFF)`) and ISN per
  connection, as real TCP stacks do. Boot-validated: **both** OP_TCP_FETCH and
  OP_RING_TCP now return `HTTP/1.1 200 OK`; "netstack ring-TCP-over-IPC (ring 3):
  OK — live TCP fetch over the ring (kernel submitted connect/send/recv/close
  batch + daemon drove one TcpConn + HTTP response returned through the ring data
  window)", clean 76s boot with all seven netstack ops green (DNS, TCP-fetch, UDP,
  SHM-ping, ring-echo, **ring-TCP**, reverse-DNS). This proves a real TCP fetch
  flowing entirely over the zero-copy ring — the exact shape the Phase-5 streaming
  socket API is built on. Next Phase 4/5 work makes the connection *persistent*
  (multiple send/recv SQEs against a live `TcpConn` across separate ring
  submissions, not a one-shot batch) and begins Phase 5 cutover (persistent
  daemon, delete `kernel/src/net/`, keep the NIC shim).

### Phase 5 — cut over + delete kernel stack  [-] in progress
Flip default from in-kernel to daemon; remove `kernel/src/net/` protocol modules;
keep only the thin NIC shim + raw-frame syscalls. Two forks in this phase have no
obviously-correct answer and are costly/irreversible, so they're queued for the
operator as **open-questions.md Q22** (deletion scope: L2–L4-only vs. delete-all
vs. phased; cutover mechanism: big-bang vs. staged-behind-a-flag; Claude's
recommendation: phased + staged). Work that is unblocked under *any* answer — the
persistent multi-connection socket server the forwarders need — proceeds now.

- 2026-07-14: Phase 5 increment 1 landed — **`conn_id`-keyed multiplexed
  connection table (the socket-server foundation).** The daemon's ring handler
  drove a single `Option<TcpConn>`; it now owns a fixed 8-slot `RingConns` table
  keyed by the SQE `conn_id` (client-chosen — in the forwarder design, the
  identity of a userspace socket). `ring_tcp_process` dispatches: `OP_CONNECT`
  reserves a free slot and installs the fresh `TcpConn` under `sqe.conn_id`
  (`result=0`; `-1` + graceful close on duplicate id / full table — no leaked
  half-open peer connection); `OP_SEND`/`OP_RECV` look the conn up by `conn_id`;
  `OP_CLOSE` evicts it, freeing the slot for reuse. `RingConns::reserve` hands
  back a `&mut` to the empty slot *before* the connection is moved in, so a
  rejected connect keeps ownership to close cleanly without a large-`Err` Result
  (dodges `clippy::result_large_err` on the 1 KiB `TcpConn`). New kernel self-test
  `netstack_ring_tcp_multi_roundtrip` (spawn.rs): one ring, two request + two
  response windows, an **8-SQE batch** that opens *both* connections (conn_id 7 &
  9) before tearing either down — `CONNECT#7, CONNECT#9, SEND#7, RECV#7, CLOSE#7,
  SEND#9, RECV#9, CLOSE#9` — so the two `TcpConn`s genuinely **coexist** in the
  daemon's table, each returning HTTP through its own ring data window; the kernel
  reaps all 8 CQEs in FIFO order and verifies both windows begin with `HTTP/`.
  Boot-validated: "netstack ring-TCP-multi-over-IPC (ring 3): OK — two connections
  multiplexed over one ring by conn_id"; both conn7 and conn9 returned
  `HTTP/1.1 200 OK`, clean 98s boot with every netstack op green. Daemon clippy is
  now fully clean (also fixed 3 pre-existing warnings: 1 collapsible-if + 2
  unnecessary-cast); kernel builds clean. **Known limitation (logged
  known-issues.md D-NETSTACK-RX-DEMUX):** the receive path reads one NIC frame and
  drops it if it doesn't match the *current* connection's 4-tuple — there is no
  shared RX demux, so two connections must not be in their `OP_RECV` phase
  simultaneously. The self-test's ordering (peers silent between handshake and
  request) respects this; the proper fix is a shared per-4-tuple RX pump in the
  daemon's top-level poll loop, whose structure the Q22b cutover mechanism decides
  — so it's built as part of (or just before) the persistent socket-server work,
  not now. Next (unblocked): persistence *across separate ring submissions* (the
  table survives multiple `OP_RING_TCP` batches / the daemon's poll loop), then —
  gated on Q22 — the socket-syscall forwarders + kernel-stack deletion.

- 2026-07-14: Phase 5 increment 2 landed — **persistent ring session across
  separate submissions (the socket-daemon lifetime shape).** Increment 1's
  `RingConns` table was still born-and-buried inside one `OP_RING_TCP` control
  call: `ring_tcp` mapped the ring, drained one batch, and unmapped — so a
  connection could not outlive a single submission. The daemon now owns a
  `RingSession { handle, va, size, conns, ipid }` that lives in `run_dns_service`
  across control calls. An `OP_RING_TCP` for a handle the session already holds
  *re-attaches* the still-mapped ring (SQ/CQ head/tail live in the shared region,
  so a fresh `Ring` view resumes exactly where the last call left off) and drains
  the new SQEs against the **same** `RingConns` — so `OP_CONNECT` in one call and
  `OP_SEND`/`OP_RECV`/`OP_CLOSE` in later calls drive the same live `TcpConn`. A
  new `OP_STOP` opcode (netipc `ring.rs` 0x05) ends a session: the daemon drains
  remaining SQEs, `close_all`s any still-live conns, and unmaps; the serve loop
  also tears the session down at its idle deadline so nothing is left mapped/half-
  open. A different handle transparently opens a fresh session (tearing the old one
  down first). New kernel self-test `netstack_ring_tcp_persist_roundtrip`
  (spawn.rs) drives ONE connection (conn_id 7) across **three separate**
  `OP_RING_TCP` control calls — each a fresh `net.stack` service connection, over
  one kernel-side ring kept mapped throughout: round 1 `CONNECT#7`, round 2
  `SEND#7`+`RECV#7`, round 3 `CLOSE#7`+`OP_STOP`. The load-bearing assertion is
  that round 2's send succeeds: had the daemon torn its session down after round 1
  (increment-1 behaviour), `conn_id 7` would be gone and the send would complete
  `-1` — so a non-negative send *is* the proof the session survived across
  submissions. Boot-validated: "netstack ring-TCP-persist-over-IPC (ring 3): OK —
  one connection driven across three separate OP_RING_TCP calls"; the persisted
  conn returned `HTTP/1.1 200 OK`, clean 91s boot with every netstack op green,
  kernel + daemon build and clippy clean, netipc 31 host tests pass. This is the
  daemon's persistent-lifetime shape (map once, serve many submissions, explicit
  teardown) that the future always-on socket daemon becomes — minimising Q22b
  rework. Next (unblocked): fold this into a bounded top-level poll loop that also
  hosts the shared per-4-tuple RX pump (D-NETSTACK-RX-DEMUX), so concurrent
  connections can receive; then — gated on Q22 — the socket-syscall forwarders +
  kernel-stack deletion.

- 2026-07-14: Phase 5 increment 3 landed — **shared RX demux (D-NETSTACK-RX-DEMUX
  resolved).** Removed the last barrier to genuinely concurrent connections: the
  receive path no longer reads one NIC frame filtered to a single 4-tuple (dropping
  any sibling's frame). A new `ring_pump(conns, me)` drains *every* pending NIC
  frame and routes each TCP segment to its owning `TcpConn` by 4-tuple —
  `recv_tcp_any` parses a frame and returns the peer identity `(src_ip, src_port,
  dst_port)` instead of pass/fail, and `RingConns::find_by_tuple` locates the owner.
  All TCP-receive logic now lives in one shared core, `TcpConn::ingest_seg`: buffer
  in-order payload into the conn's own `rx_buf` (new per-conn `rx_buf`/`rx_len`),
  advance `rcv_nxt`, emit the cumulative ACK, honor FIN/RST; out-of-order → dup-ACK.
  `OP_RECV` (`ring_tcp_recv`) now polls `ring_pump` until the *target* conn has data
  / EOF / times out, then `take_rx`s the target's buffer into the SQE window — so
  while it waits, a sibling's inbound frames are delivered to *that* sibling instead
  of being discarded. The single-connection `TcpConn::recv` (one-shot `tcp_fetch`
  control op) shares the identical `ingest_seg`/`take_rx`/`maybe_retransmit` core —
  one TCP implementation, no duplication. New kernel self-test
  `netstack_ring_tcp_demux_roundtrip` (spawn.rs) submits an interleaved 8-SQE batch
  `CONNECT#7, CONNECT#9, SEND#7, SEND#9, RECV#7, RECV#9, CLOSE#7, CLOSE#9` — **both
  sends before both recvs**, so conn9's response arrives while the daemon is blocked
  in conn7's RECV (the exact concurrency the old filtered read broke). Boot-
  validated: "netstack ring-TCP-demux-over-IPC (ring 3): OK — two connections
  received concurrently … no sibling frames dropped"; both conn7 and conn9 returned
  `HTTP/1.1 200 OK`, clean 101s boot with every netstack op green (multi + persist
  still pass), kernel + daemon build and clippy clean. Next — gated on Q22 (still
  open) — the socket-syscall forwarders + kernel-stack deletion; the daemon now has
  the persistent-lifetime shape *and* the concurrent-receive demux the always-on
  socket server needs.
- 2026-07-14: Phase 5 increment 5.4 landed — **reusable kernel netstack client +
  `net.userspace` boot switch.** Extracted the connect/send/recv/close ring-driving
  logic that was hand-inlined in the `spawn.rs` persist self-test into a reusable
  kernel module `kernel/src/net/netstack_client.rs`: a `NetstackConn` type owning
  one SHM ring + one daemon TCP connection, with `open`/`connect`/`send`/`recv`/
  `close` methods, each op a single `OP_RING_TCP` control round-trip against the
  daemon's persistent session. Send chunks to ≤`SND_CAP`=1024 (daemon `TCP_SND_BUF`);
  recv returns ≤`RCV_CAP`=512 (daemon `MSG_CAP`) per call; fixed data window
  (SND_OFF=0/RCV_OFF=1024/data_len=1536, sq=cq=8). `Drop` tears the daemon session
  down (best effort) and always releases the SHM. Added the staged-cutover boot
  switch `netstack_client::userspace_enabled()` → `kernparam::is_set("net.userspace")`
  (default **off**; §66 Q22b) — recorded/surfaced by the boot self-test only, no
  socket routing yet. The single-connection persist self-test
  (`netstack_ring_tcp_persist_roundtrip`) was **deleted** and replaced by
  `netstack_client::self_test_http`, which drives the same connect→send→recv→close
  through the reusable client (a successful send after a separate-round connect
  still proves session persistence — no duplicate ring test). No syscall behavior
  change. Next — 5.5: AF_INET SOCK_STREAM sockets in the Linux ABI backed by this
  client, switch-gated.
- 2026-07-14: Phase 5 increment 5.5 landed — **AF_INET/AF_INET6 SOCK_STREAM
  sockets in the Linux ABI, switch-gated.** New object layer
  `kernel/src/net/socket.rs`: a `SocketHandle`/`SockState` stream socket wrapping
  one `NetstackConn` behind an `Arc<Mutex<SocketInner>>` in a global
  `SOCKET_TABLE`, with fd-refcounting (`create`/`dup`/`close`) and a strict lock
  discipline (never hold the table lock across a blocking daemon round-trip; the
  final `Arc` drop / teardown runs after the table lock is released). New
  `HandleKind::Socket` fd kind and `ResourceType::NetSocket` per-process IPC
  handle, so a socket's daemon connection is released on process exit
  (`ipc::cleanup_handles`) and refcount-dup'd across `fork` (`fork::dup_one`) —
  mirroring the memfd pattern exactly. Wired, **only when `net.userspace` is on**:
  `sys_socket` (AF_INET/AF_INET6 + SOCK_STREAM → real fd; else unchanged ENOSYS),
  `sys_connect` (parses `sockaddr_in`, AF_INET6 → EAFNOSUPPORT), `sys_sendto`/
  `sys_recvfrom` (delegate to send/recv; MSG_* flags and the recvfrom src-addr
  out-params ignored), `read`/`write`/`close` dispatch, and `getpeername`
  (returns the connected peer). All ~21 exhaustive `HandleKind` match sites across
  `linux.rs`/`procfs.rs` extended with a `Socket` arm (fstat → S_IFSOCK|0777,
  fsync/ftruncate/etc → EINVAL/ESPIPE, poll → best-effort ready, kcmp ordering,
  /proc/fd → `socket:[ino]`). `O_NONBLOCK` is tracked authoritatively in the fd
  table (fcntl F_GETFL/F_SETFL), not duplicated in the socket. Added
  `socket::self_test` (create/dup/close refcount + closed-handle error surface),
  run in the net self-test aggregator. Switch off → syscall surface unchanged;
  the in-kernel resident stack still owns the NIC. Next — 5.6: persistent daemon
  spawn at boot + NIC ownership handoff.
- 2026-07-14: Phase 5 increment 5.6 landed — **persistent daemon spawn at boot +
  NIC ownership handoff, switch-gated.** New daemon mode `serve-net` in
  `services/netstack` (`run_dns_service(me, persistent=true)`: ignores the idle
  deadline, owns the NIC for the system's lifetime, returns only on unrecoverable
  fault). New `proc::spawn::run_persistent_netstack`: gated on a live network
  (`ifinfo.up && ip != 0.0.0.0`), it `include_bytes!`-spawns the daemon with caps
  `[(NetRaw, WRITE), (Service, WRITE)]` and argv `["netstack","serve-net"]`, waits
  (3s) for `service::is_registered("net.stack")`, then validates parity against
  the live daemon via service-name round-trips: `netstack_resolve_a("example.com")`
  → `netstack_tcp_fetch_roundtrip(ip,80)` → `netstack_udp_dns_roundtrip(dns)`; the
  daemon is left running (not reaped). `kernel/src/main.rs` boot section is now
  switch-gated: switch **on** → spawn the persistent daemon and skip the bounded
  in-kernel netstack self-tests (avoids §64 exclusive raw-NIC claim contention);
  switch **off** → unchanged (bounded self-tests, in-kernel stack owns the NIC).
  Boot self-test socket-creation assertions in `syscall/linux.rs` made
  switch-aware via a new `assert_stream_socket_gate` helper (ENOSYS when off;
  "not ENOSYS"/EBADF + close-any-leaked-fd when on) across all 5 AF_INET/AF_INET6
  SOCK_STREAM sites. `hrtimer::self_test` Tests 3/5 made robust to a persistent
  background daemon holding a pending kernel hrtimer (relative pending-count
  baseline under `without_interrupts`, replacing the absolute `== 1`/`== 0`
  asserts that assumed a globally-empty pending list). Proven in QEMU with the
  switch on (serial 1800–1810): daemon spawned, claimed NIC, registered
  `net.stack`, DNS+TCP+UDP parity all OK. Boot-test hangs seen this session are
  the known moving-location container-exec / ring-3 spawn-dispatch race (see
  known-issues.md), not a 5.6 regression — switch-off boot is behaviourally
  unchanged. Next — 5.7: flip the default to the daemon once parity holds.
- 2026-07-14 (follow-up, same increment): made the `net.userspace` switch
  **actually usable at runtime** and proved the full cutover end-to-end via the
  real Limine cmdline (not the earlier force-on hack). Three fixes:
  (1) **Limine kernel-file request ID typo** in `kernel/src/limine.rs`: the
  second feature-id word was `0x31eb_5d10_c871_c930`, which never matched
  Limine's `LIMINE_{KERNEL,EXECUTABLE}_FILE_REQUEST` magic
  (`0x31eb_5d1c_5ff2_3b69`, per limine.h in Limine 8.7.0). The response was
  therefore *always null*, so `boot::kernel_cmdline()` always returned `None` —
  meaning the boot cmdline (and kernel-file symbolization) had silently never
  worked. Corrected the word; the cmdline now round-trips.
  (2) **`kernparam::init_defaults()` was never called at boot** — only lazily
  from the `kernparam` shell command — so the param store stayed `None` and
  `is_set("net.userspace")` always returned false regardless of the cmdline.
  Wired a boot-time `fs::kernparam::init_defaults()` call into `kernel_main`
  right after `sysctl::init()`.
  (3) **Persistent daemon spawn deferred past kernel POST.** The daemon owns the
  NIC and runs continuously; leaving it running during the later timing-sensitive
  timeout self-tests (channel/futex/eventfd recv-with-timeout) and the hrtimer
  pending-count asserts perturbed them. Moved the switch-on spawn to just before
  `BOOT_OK` (alongside the container health monitor, which is deferred for the
  same reason), so POST runs in a quiet system and the NIC is owned by the time
  userspace comes up. Also fixed a latent lost-wakeup in the futex timeout
  self-test's waker (`timeout_waker_task`): it wakes without changing the futex
  word, so a wake that precedes the waiter's park was lost → spurious
  `TimedOut`; now it retries `futex_wake` until it reports it woke a waiter
  (bounded). Verified in QEMU: with `cmdline: net.userspace` the switch reads
  **on**, the persistent daemon spawns after POST, registers `net.stack`, and
  DNS+TCP+UDP parity is green with the NIC owned for the system's lifetime;
  with the cmdline removed the switch reads **off** and the resident stack stays
  authoritative — both paths pass all boot self-tests.

- **[2026-07-14] O_NONBLOCK receive parity (post-5.6, toward 5.7).** First
  D-NETSOCK-SYNC parity gap closed: the daemon-backed stream socket now honours
  `O_NONBLOCK` on the *receive* side. Previously the ring client was fully
  synchronous — a `recv` on a socket with `O_NONBLOCK` set still blocked the
  caller until the daemon's receive deadline. Added a `netipc::ring::RECV_NONBLOCK`
  flag (carried in the `OP_RECV` SQE `aux`, orthogonal to `OP_CONNECT`'s endpoint
  packing) and an `ERR_WOULD_BLOCK` (-11) completion sentinel. The daemon's
  `ring_tcp_recv` reads the flag: on a non-blocking recv it drains the shared RX
  pump exactly once, and if the target connection has no buffered in-order bytes
  and the stream is still open it returns `ERR_WOULD_BLOCK` instead of polling.
  The kernel plumbs it through `NetstackConn::recv(_, nonblock)` →
  `net::socket::recv(_, nonblock)` → `dispatch_socket_read` (which reads the fd's
  `O_NONBLOCK` status flag), mapping the sentinel to `KernelError::WouldBlock` →
  `EAGAIN`. Boot-validated switch-on: the persistent-daemon parity block runs a
  new `netstack_client::self_test_nonblock_recv` that connects to `example.com:80`
  and, before sending any request, issues a non-blocking recv — the server sends
  nothing unsolicited, so the daemon returns WouldBlock and the socket reports
  EAGAIN promptly (`serial: "non-blocking recv on idle socket returned WouldBlock
  (EAGAIN) as expected"`). Switch-off boot unchanged (daemon not spawned; new
  code is switch-gated). Still synchronous, remaining before the 5.7 flip:
  non-blocking connect/send, honest poll/epoll readiness (needs a non-destructive
  peek op), listen/accept, and IPv6 connect.
