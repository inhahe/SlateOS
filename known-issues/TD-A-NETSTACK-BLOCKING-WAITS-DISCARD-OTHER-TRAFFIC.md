### TD-A-NETSTACK-BLOCKING-WAITS-DISCARD-OTHER-TRAFFIC -- 2026-10-03 -- OPEN (lane A)

**Status:** OPEN (lane A)

**In short:** while the netstack daemon waits for one particular answer --
a DNS reply, an ARP or neighbour-discovery reply, a TCP handshake -- it reads
frames off the network card itself and throws away every frame that is not
the one it wants. Whatever else arrives in that time is lost: another
socket's UDP datagram is gone for good, an ARP request for us goes
unanswered, another connection's TCP segment has to be resent. With one
program using the network this rarely shows; with several it means lost
datagrams and stalls that look random.

**Where** (`services/netstack/src/main.rs`): each of these loops calls
`raw_rx` and discards a non-matching frame --

| Wait | Function | Reached from the serving loop by |
|---|---|---|
| ARP reply | `arp_resolve` | the DNS service's next hop at startup |
| Neighbor Advertisement | `ndp_resolve` | `OP_CONNECT6` to an on-link peer (`ring_tcp_process`) |
| DNS answer | `resolve_dns`, `resolve_ptr` | `OP_RESOLVE_A`, `OP_RESOLVE_PTR` |
| UDP reply | `udp_exchange` | `OP_UDP_EXCHANGE` |
| TCP segments | `recv_tcp_seg`/`recv_tcp_seg6` via `TcpConn::recv_one_seg` | blocking `connect`/`recv` on a `TcpConn` |

Only `pump` (via `recv_any`) classifies every frame and gives each to its
owner; since 2026-10-03 it also answers ARP, ping, neighbour solicitations
and ping6 (`answer_link`).

**To reproduce:** bind a UDP socket and have a peer send it a stream of
datagrams while another program resolves a name through `OP_RESOLVE_A`
(or opens a TCP connection to a slow peer): datagrams that arrive during the
wait never reach the socket.

**The proper fix:** one receive path. Every wait becomes "pump until my
answer is here": the loop calls the same classifier `pump` uses, which hands
each frame to its owner -- the TCP connection it belongs to, the UDP socket's
queue, `answer_link`, the multicast query handler -- and the waiter checks a
slot its own frame is delivered to (a pending ARP/NDP/DNS entry keyed by
what was asked). That removes the six private frame readers, and with them
the class of bug; it is a restructuring of the daemon's receive side, not a
patch to each loop.
