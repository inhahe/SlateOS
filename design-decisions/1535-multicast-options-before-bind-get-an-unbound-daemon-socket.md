## 1535. Multicast options set before `bind(2)` get an unbound socket in the netstack daemon

**Date:** 2026-10-03 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a Linux program may join a multicast group (a set of machines
that all receive datagrams sent to one shared address) or set how far its
multicast travels *before* it picks a port with `bind(2)`. Many do: an mDNS
responder typically joins 224.0.0.251 and then binds port 5353. The netstack
daemon, which carries SlateOS's sockets, used to create its UDP socket only
at bind time, so there was nothing to hold such an option. Now the first
multicast option on an unbound socket creates the daemon's socket without a
port; the program's own `bind` later gives it one, keeping the groups and
settings. Nothing changes for a program that binds first.

**Where it bites:** `netipc::ring::UDP_BIND_UNBOUND` (the ring flag),
`services/netstack` `UdpSocks::open`/`bind`, `kernel/src/net/netstack_client.rs`
`udp_open`, `kernel/src/net/socket.rs` `SocketInner::opened` and
`dgram_setopt`.

**The choice:**

| | For | Against |
|---|---|---|
| **An unbound daemon socket (chosen)** | one place holds the state, the daemon, so `getsockopt` reads what is really in force; every refusal comes back from the call that caused it, as on Linux -- a full group table is `ENOBUFS` from `setsockopt(IP_ADD_MEMBERSHIP)`, the membership report goes out at the join | a daemon socket can now exist with port 0, so every lookup by port and every send/receive must refuse port 0 (done: `by_port`, `get_mut`) |
| Remember the options in the kernel and replay them at bind | no daemon change | two copies of the state; a join that fails at replay would have to fail the *bind* -- `bind` answering `ENOBUFS` for a group table is an error no program expects -- or be dropped silently, and a program that thinks it is in a group but hears nothing is the worst outcome |
| Bind an ephemeral port at the first option | trivial | the program's own `bind(5353)` then fails with `EINVAL` (already bound): breaks exactly the mDNS pattern this exists for |

**A related divergence, also mine:** Linux lets a TCP socket set
`IP_MULTICAST_LOOP`/`IPV6_MULTICAST_LOOP`, which it never uses. Here a stream
socket is refused them (`ENOPROTOOPT`) so that what its `getsockopt` reports
-- always the default -- can never contradict a value it was given. Every
other errno matches Linux 6.x, including where IPv4 and IPv6 differ (a leave
of a non-multicast address is `EADDRNOTAVAIL` for IPv4, `EINVAL` for IPv6);
the translation and its tests are `netipc/src/sockopt.rs`.

**Revisit if:** the daemon gains a per-socket "create at `socket(2)`" op for
other reasons, in which case the unbound state becomes the normal one and
`UDP_BIND_UNBOUND` can go.
