## 1532. mDNS is a userspace responder's job: the kernel's mDNS module gets no system calls

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous), applying `design.txt`
line 78 ("don't put networking in the kernel") and §63 · **Lane:** A

**In short:** programs that find printers and other machines on the local
network by name (`avahi-resolve`, `avahi-browse`, `avahi-publish`) use mDNS
(multicast DNS: names ending in `.local`, answered by the machines themselves
rather than by a DNS server). The kernel has an mDNS module, reachable only
from the kernel's debug shell. On 2026-09-11 lane A told lane B to wait for
system calls into it. It will not add them. The answering should be done by an
ordinary program, as on Linux (`avahi-daemon`). That program needs only UDP
sockets that can join a multicast group (one address many machines listen on),
and most of that exists.

| | For | Against |
|---|---|---|
| **A userspace responder over UDP multicast (chosen)** | what `design.txt` asks and §63 is doing to the rest of the network stack; restartable, and a fault in it cannot take the kernel down; one owner of port 5353, as `avahi-daemon` is | lane B writes or ports it, and three gaps must close first (below) |
| System calls into `kernel/src/net/mdns.rs` | the responder exists and passes its self-tests; lane B could write clients today | grows the kernel-resident stack §63–66 are retiring, by an ABI that outlives it; the module answers for the whole machine as `neo`, a name fixed in its source; registering a name on the network would want a new right, decided for an interface that is meant to go |

**What a userspace responder has now:** at boot nothing holds UDP port 5353.
`mdns::init` runs only from the kshell `mdns init` command. A native program can
bind it and join 224.0.0.251 (`IP_ADD_MEMBERSHIP` →
`SYS_UDP_MCAST_JOIN`, a `Socket` capability with `WRITE`).

**The gaps, lane A's to close as the responder needs them** (`roadmap.md`, the
row under the netstack cutover):
- no IPv6 group join from userspace -- the kernel has `udp::join_group_v6`, but
  no call reaches it, and libc's `IPV6_JOIN_GROUP` has nowhere to go;
- no per-socket multicast TTL or loop setting: the kernel's UDP sends at TTL 64,
  RFC 6762 wants 255 on what a responder sends, and libc accepts
  `IP_MULTICAST_TTL` and `IP_MULTICAST_LOOP` and does nothing with them (lane
  D's half, once the kernel has somewhere to send them);
- the Linux ABI's `setsockopt` answers `ENOPROTOOPT` for every `IPPROTO_IP` and
  `IPPROTO_IPV6` option (its sockets are the netstack daemon's, which has no
  multicast yet).

**Not done:** the kernel's `net/mdns.rs` stays, kshell-only, until the
kernel-resident stack's deletion (§66) takes it or a responder replaces it.
Its RFC 6762/6763 encoding and decoding is the part worth carrying into
whatever lane B builds.
