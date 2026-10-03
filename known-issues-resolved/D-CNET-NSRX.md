### D-CNET-NSRX. Per-namespace veth RX + TX dispatch — RESOLVED (RX threading + container veth TX egress both landed)

**Status (2026-07-01): both halves landed and boot-validated.** The whole
ingress chain carries the arrival namespace (a container server socket bound
in its own netns is matched by the per-ns socket lookup), **and** the egress
path now routes container traffic (IPv4 data, fragments, ARP requests, ARP
replies) onto the container's veth instead of the physical NIC. Container
inbound/outbound over a user-defined network is functional end-to-end. The
one residual limitation is the **shared (non-namespaced) ARP cache** — see
the note at the end.

**What landed (RX threading).** `ns_id` is threaded as a parameter through
the entire RX chain:
- `ethernet::process_frame(data, ns_id)` — `is_for_us` now compares against
  the receiving namespace's interface MAC via the new
  `interface::ns_mac(ns_id)` (physical NIC MAC in root; veth-endpoint MAC in a
  container ns) instead of always the host NIC MAC.
- `ipv4::process_ipv4(payload, ns_id)` — `is_for_us` uses `ns_info(ns_id)`;
  inbound firewall uses `check_inbound_ns(ns_id, …)`; dispatches to
  tcp/udp/icmp with `ns_id`; `dispatch_reassembled` threads it too.
- `ipv6::process_ipv6(payload, ns_id)` — same shape (transport socket lookup
  is ns-scoped; NDP/SLAAC stay physical-NIC based, IPv6 container addressing
  is future work).
- `arp::process_arp(payload, ns_id)` — the "request for our IP?" check and
  reply source use `ns_ip`/`ns_mac`.
- `tcp::process_tcp/_v6(pkt, ns_id)` — pass `ns_id` to `process_tcp_common`
  instead of the old hardcoded `ROOT_NS`.
- `udp::process_udp/_v6(pkt, ns_id)` — the delivery loop now filters by
  `sock.ns_id` (root permissive, mirroring the TCP listener rule).
- `icmp::process_icmp(pkt, ns_id)` — echo replies are sent from the arrival
  namespace via `ipv4::send_ns`.
- Call sites: `net::mod::poll` and `bridge::forward_all`'s host-stack flood
  pass `ROOT_NS`; `veth::poll_all` passes each drained endpoint's own
  `ns_id`.

Boot-validated: `[udp]   Namespace isolation: OK` (extended to cover
delivery-level scoping — a datagram arriving in ns1 reaches only the ns1
socket, root arrival is permissive), full `[net] Network self-test PASSED`,
and the ARP ns tests — no physical-NIC regression.

**What landed (container veth TX egress).** The ns-aware send path now has a
veth egress branch keyed on the namespace:
- `net::send_frame_ns(ns_id, frame)` (`net/mod.rs`) — the single egress
  chokepoint: for `ns_id != ROOT_NS` with a veth endpoint
  (`veth::find_endpoint_for_ns`), it captures TX, `veth::send(pair, end,
  frame)` (→ enqueues on the peer host end A's RX → `bridge::forward_all`
  switches to the peer / floods to the host NAT stack), and records TX;
  otherwise it falls through to `send_frame` (physical NIC). Root traffic is
  unchanged.
- `ipv4::send_ns_ecn` and `send_fragmentable_ns` (both single-frame and the
  fragmentation while-loop) now source MAC/IP from `interface::ns_mac(ns_id)`
  / `ns_ip(ns_id)` / `ns_info(ns_id)`, resolve the next hop via
  `arp::resolve_ns(ns_id, …)`, and egress via `send_frame_ns(ns_id, …)`.
- `arp::send_request_ns` / `resolve_ns` — ARP requests are sourced from the
  ns interface and egress the ns link; `resolve_ns`'s poll loop drives
  `net::poll` (drains veth+bridge), so a peer reply returning through the
  bridge is learned into the cache. `resolve`/`send_request` delegate to the
  `_ns` forms with `ROOT_NS`.
- `arp::send_reply` — **no longer drops** non-root replies; it egresses via
  `send_frame_ns(ns_id, …)`, so a container answers ARP for its own IP on its
  user-defined network.

The container-creation path already assigns the ns interface IP/mask/gw/dns
(`netns::configure_interface`) and sets up the veth pair
(`setup_container_veth`), and `resolve_next_hop` for non-root uses
`netns::route_lookup` → ns gateway → direct-to-dst fallback, so
`resolve_next_hop`/`is_for_us` line up.

Boot-validated: new `[veth]   test 11 (send_frame_ns veth egress): OK`
(`[veth] Self-test PASSED (11 tests)`) — asserts a non-root
`send_frame_ns` lands on the peer host end's RX and a root-ns frame does NOT
leak into the veth — plus the RX-side `[udp]   Namespace isolation: OK` and
full `[net] Network self-test PASSED`, no physical-NIC regression.

**Per-namespace ARP cache — RESOLVED (was: shared ARP cache).** The former
residual (a single global ARP cache shared across all namespaces, so two
container networks reusing a subnet/IP could collide) is now closed. The
per-namespace ARP cache infrastructure that already existed (`NS_ARP`,
`ns_init`/`ns_destroy`/`ns_lookup`/`ns_insert`/`ns_flush`) is now wired into
the real paths:
- `container::setup_container_veth` calls `arp::ns_init(net_ns)` (and
  container removal calls `arp::ns_destroy(net_ns)`), so every networked
  container gets its own active ARP cache.
- `arp::process_arp` learns the sender's MAC into the *arrival* namespace's
  cache via `ns_insert(ns_id, …)` (delegates to global for ROOT_NS) instead
  of always `cache_insert` (global).
- `arp::resolve_ns` reads/waits on `ns_lookup(ns_id, …)` instead of the
  global `lookup`.
Boot-validated by a new `[arp]   ns process_arp learns into ns cache: OK`
(`[arp-ns] Per-namespace ARP self-test PASSED (4 tests)`), which asserts a
reply arriving in a namespace is learned into that ns's cache and does NOT
leak into the global cache. Root-namespace behavior is unchanged (still uses
the global `ARP_CACHE`).

**Discovered/analyzed:** 2026-07-01 (embedded-DNS work). **RX threading
landed:** 2026-07-01. **TX egress landed:** 2026-07-01. **Per-ns ARP cache
wired:** 2026-07-01.
