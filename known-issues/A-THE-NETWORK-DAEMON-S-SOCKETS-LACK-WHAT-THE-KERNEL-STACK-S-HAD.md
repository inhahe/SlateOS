## A-THE-NETWORK-DAEMON-S-SOCKETS-LACK-WHAT-THE-KERNEL-STACK-S-HAD (lane A, 2026-10-16) — OPEN

**Status:** OPEN (lane A). Found by the Opus and Fable reviews of lane A's
batch 0713e88ab..a53272118, which moved native sockets onto the daemon
(36448ec86). What could be fixed in the kernel's half was, in the follow-up;
these need the daemon (`services/netstack`).

**In short:** native programs' network connections now go through the network
daemon, as Linux programs' already did. The daemon's sockets lack a few things
the kernel's own stack had: a datagram larger than one network frame cannot be
sent, a socket queues only two incoming datagrams before it drops the oldest,
TCP keepalive probes are never sent, and a close cannot reset a connection. A
program that relies on any of them gets an error, or loses data UDP is allowed
to lose, rather than wrong behaviour it cannot see.

### What is missing, and what a program sees

| Missing | Kernel stack had | A program sees now |
|---|---|---|
| IP fragmentation | UDP datagrams up to 65,507 bytes | a send above `netipc::ring::UDP_DGRAM_MAX` (1480) is `MsgSize`; a larger datagram from the network is never delivered |
| Receive queue depth | 64 datagrams per socket | 2 (`UDP_QLEN`), the oldest dropped first |
| TCP keepalive | probes after an idle time | `SYS_TCP_SET_KEEPALIVE` on is `NotSupported`; Linux's `SO_KEEPALIVE` is accepted and does nothing |
| Reset on close | `SO_LINGER` 0 sent `RST` | an orderly close either way (`native_socket::release`) |

### The fix

- **Queue depth:** the daemon's `UdpSock` keeps its queue inline, in a `Net`
  on the daemon's main stack, so a deeper one (8 datagrams of 1480 bytes,
  for 8 sockets: 96 KB) risks the stack. Move each queue to the heap and give
  it Linux's measure: bytes, not datagrams (`SO_RCVBUF`).
- **Keepalive:** an idle timer per connection in the daemon's TCP, probing as
  the kernel stack did (`net::tcp::set_keepalive`'s defaults: 75 s idle,
  10 s interval, 9 probes), and `OP_SETOPT` codes to set it.
- **Fragmentation:** IPv4 and IPv6 fragmentation and reassembly in the daemon
  (`netproto`), with a reassembly timeout and a cap on what it holds.
- **Reset on close:** an `OP_ABORT`, sent by `native_socket::release` for
  `Ending::Abort` and by the Linux path for `SO_LINGER` 0.
