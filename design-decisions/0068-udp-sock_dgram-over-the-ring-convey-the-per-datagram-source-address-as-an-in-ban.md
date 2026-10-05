## 68. UDP `SOCK_DGRAM` over the ring — convey the per-datagram source address as an in-band header prefix, not by widening the CQE

**Date:** 2026-07-15
**Decided by:** Claude (autonomous). Sub-implementation call under §63/§65
(the netstack userspace migration and its io_uring-style ring). Reversible: the
ring ABI is versioned (`RING_VERSION`) and no UDP client depends on it yet — the
daemon/kernel UDP layers are still to be built on top of this ABI.

**Context.** The stream socket path is done; the next daemon-backed socket
feature is connectionless UDP (`SOCK_DGRAM`): `bind` a local port, then `sendto`
arbitrary destinations and `recvfrom` arbitrary senders. Stream ops never need a
per-op peer address (the connection *is* the peer), but every UDP datagram
carries its own source (on recv) and destination (on send). Destination is easy
— it fits the existing 48-bit `[ip:4][port_be:2]` `Sqe::aux` endpoint packing
(same as `OP_CONNECT`). The hard part is the **recv** direction: the 16-byte
`Cqe` (echoed `user_data` + `i32` result + `u32` flags) has no room for a
source address, so the daemon needs another channel to report *who* a received
datagram came from.

**Decision: prepend a fixed 24-byte source-address header to the recv data
window.** `OP_UDP_RECV` has the daemon write, at the front of the SQE's data
window, a `UDP_ADDR_HDR_LEN` (24-byte) header — `[family:2][port_be:2][ip:16]
[reserved:4]` (`Sqe::pack_udp_addr`/`unpack_udp_addr`) — immediately followed by
the datagram payload. The CQE `result` reports the *payload* length only (the
header is not counted). New opcodes `OP_UDP_BIND`/`OP_UDP_SEND`/`OP_UDP_RECV`
(0x0C–0x0E) and sentinels `ERR_ADDR_IN_USE`/`ERR_MSG_SIZE`.

**Alternatives considered.**
- *Widen the CQE to 32 bytes with an address field.* Pro: semantically cleaner
  (the address rides the completion, not the data buffer); the payload window is
  "just payload". Con: the CQE layout is **shared by every opcode** and its
  serialization (`Cqe::to_bytes`/`from_bytes`) is on the hot path for the stream
  sockets too — widening it perturbs the whole ring for a UDP-only need, and 16
  bytes still can't hold a 16-byte IPv6 address + port + family anyway (would
  need 32B, doubling CQ memory for all ops). Larger blast radius, reworks proven
  stream code.
- *A side channel / second ring for addresses.* Pro: keeps both the CQE and the
  payload window pure. Con: a whole extra SPSC structure and index dance per
  datagram; far more moving parts than a fixed prefix; more to get wrong.
- *IPv4-only 8-byte prefix now, extend later for IPv6.* Pro: smallest header.
  Con: a second, incompatible header layout when IPv6 datagram sockets land — a
  gratuitous ABI fork. The 24-byte header already carries a full IPv6 address, so
  one layout serves both families forever.

**Reasoning.** The in-band prefix keeps the CQE and the entire stream-socket ring
path **byte-for-byte unchanged** (zero regression risk to the working TCP
sockets), needs no new ring structures, and is trivially forward-compatible with
IPv6 (the 16-byte `ip` slot holds a v4 address left-packed or a full v6 address,
selected by `family`). The only cost is that the UDP recv payload window is
offset by a fixed 24 bytes — a one-line arithmetic detail on both sides, and the
same in-band-metadata pattern Linux itself uses for ancillary data. The chosen
`result = payload-length` convention (header excluded) means callers size and
copy exactly the datagram bytes, matching `recvfrom` semantics.

**Tracking.** known-issues D-NETSOCK-SYNC (UDP `SOCK_DGRAM` listed as a remaining
gap); roadmap netstack Phase 5. This commit lands the ring ABI + helpers +
unit tests; the daemon UDP socket table, kernel `UdpConn` client, and the
`sys_socket(SOCK_DGRAM)`/`sendto`/`recvfrom` fd wiring build on it in follow-ups.
