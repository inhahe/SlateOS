## A-INTERFACE-VIEWS-SHOW-ONLY-THE-KERNELS-NIC (lane A, 2026-10-30) — OPEN

**Status:** OPEN

**What is wrong.** With `net.nic` set (design-decisions 1594) the root
namespace's sockets go over a card a Linux driver process serves, and
`SYS_NET_IF_INFO` says that card, as the network daemon says it. But the
kernel's other views of "the network interface" still show the kernel's own
NIC alone, which then carries none of those sockets:

- `/proc/net` (`fs/procfs.rs` `gen_net`) and `/sys/devices/net`
  (`fs/sysfs.rs` `gen_net_file`, `net_present`);
- `netstat`'s interface lines (`net/netstat.rs`) and the kernel shell's;
- `SYS_NET_IF_CONFIG`, which sets the kernel's NIC's addresses, with nothing
  that sets the card's (the daemon takes them by DHCP alone).

The card is in none of them, and a machine whose only network is the card
(the test PC's WiFi) shows an interface with no address there.

**Why not simply ask the daemon there too.** `SYS_NET_IF_INFO` asks it per
call, a channel round trip of up to 6 s; `/sys` path classification calls
`net_present` under the VFS's locks (`sysfs.rs`, lock order "VFS, then
..."), and a daemon round trip there would hold them across IPC.

**The proper fix.** An interface table in the kernel, one entry per
interface -- the kernel's NIC and each card a driver process serves --
the daemon telling the kernel its card's state as it changes (attached,
lease taken or lost, link up or down) through a syscall of its own, and
every view reading the table: `/proc/net` and `/sys/devices/net/<name>` an
entry per interface, `SYS_NET_IF_INFO` the one the caller's sockets use
without a round trip, and `SYS_NET_IF_CONFIG` reaching the daemon for a
card's static addresses.

**Who sees it.** No one yet: `net.nic` is off by default and set only by
hand in QEMU. The first is the test PC once its WiFi carries its network.
