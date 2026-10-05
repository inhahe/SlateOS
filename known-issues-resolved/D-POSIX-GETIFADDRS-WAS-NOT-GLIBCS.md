### [D] D-POSIX-GETIFADDRS-WAS-NOT-GLIBCS — 2026-09-27 — FIXED 2026-09-27

**Where:** `posix/src/socket.rs` (`getifaddrs`, `for_each_ipv4_interface`).

**What it was.** `getifaddrs` listed one `AF_INET` entry per interface that
had an address.  glibc 2.39's list, printed field by field in network
sandboxes shaped like this system (`posix/tools/oracle/ifaddrs_oracle.c`: `lo`, and a
veth named `eth0` with QEMU's MAC), differs in four ways:

| | Was | glibc (now) |
|---|---|---|
| Link entries | none | one `AF_PACKET` entry per interface, before the addresses: `ifa_addr` a `sockaddr_ll` with the hardware address (`ARPHRD_LOOPBACK` and zeros, or `ARPHRD_ETHER` and the NIC's MAC), `ifa_broadaddr` the broadcast MAC, `ifa_netmask` NULL, and `ifa_data` pointing at the link's `struct rtnl_link_stats` -- which programs read without a NULL check |
| Flags | 0x49 for `lo`, 0x1043 for an up `eth0` | 0x10049 and 0x11043: `IFF_LOWER_UP` as well, which netlink reports and glibc passes on |
| `lo`'s `ifa_broadaddr` | NULL | 127.0.0.1: glibc reads the loopback's netlink answer as a point-to-point link's |
| A down `eth0` | left out | listed, with its address and flags 0x1002: Linux keeps an address on a link that is down |

The last rule now holds for `AI_ADDRCONFIG` and `host.conf`'s `reorder` too
(`for_each_ipv4_interface`), as glibc's `check_pf` and `SIOCGIFCONF` count an
address on a down link; routing (`route_source`) still needs the link up.  A
/31 or /32 reports the address itself as its broadcast address, as glibc does
where Linux sets none.

**What still differs.** `lo`'s counters are zero: the kernel counts no
loopback traffic.  Of `eth0`'s, the kernel keeps six -- bytes, packets and
errors sent, bytes, packets and drops received (`SYS_NET_STAT`) -- and the
other eighteen are zero.
