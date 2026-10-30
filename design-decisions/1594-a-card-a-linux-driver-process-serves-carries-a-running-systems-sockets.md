## 1594. A card a Linux driver process serves carries a running system's sockets

**Date:** 2026-10-30 · **Decided by:** Claude (autonomous) -- the mechanism behind known-issues A-NETSTACK-SERVES-SOCKETS-ONLY-OVER-THE-KERNELS-NIC's proper fix; which card a given machine uses is the machine's configuration, and the defaults change nothing · **Lane:** A

**In short:** Until now a network card driven by one of Linux's own drivers
(decision 1588) carried traffic only inside the kernel's self-tests: nothing
started its driver on a running system, and the network service (the
netstack daemon) always used the kernel's own card. Two boot options now
let a machine use such a card for everything -- the test PC's WiFi card,
which only a Linux driver can drive, being the first that will need it. Both
are off by default.

### The decision

1. **`linuxdrv.serve=<driver>@<BB:DD.F>`** (`linuxdrv::serve_at_boot`): past
   the kernel's self-tests, the kernel starts `linuxdrv/<driver>` and never
   reaps it, serving the card at that PCI address as `nic.<driver>` for the
   system's life (the driver's `serve=` mode with no `stacks=` limit). It
   holds the card and the right to register that one name -- and, for a
   driver that reads its card's firmware (`FIRMWARE_DRIVERS`: `mt7925e`),
   the right to read beneath the firmware directories alone
   (`FileTree`, decision 1595) -- never the File capability, which is
   every file. The card is chosen as `linuxdrv.card` chooses one
   (`card_choice`, `CardKind::Network`): a value that is not a driver's
   name and a PCI address, a driver not loaded, a function not there, one
   that is not a network controller (PCI class 02 -- a slip naming the GPU
   is refused before its decoding is touched) or one a driver holds is
   refused and said (`WARNING` at boot, and the linuxdrv self-test fails),
   the boot going on.
2. **`net.nic=<service>`** (`linuxdrv::netstack_card`,
   `proc::spawn::run_persistent_netstack`): the persistent netstack daemon
   serves `serve-net <service>` -- attaching to the card over its port
   (decision 1589) -- holding the card's key and the right to register
   `net.stack` alone, not the kernel's NIC, which stays the kernel's. It is
   honoured only when it names the card `linuxdrv.serve` started serving;
   empty, or naming another, or with no card served, it is said and the
   daemon serves the kernel's NIC. It is started even when the kernel has no
   network of its own, and given 60 s, not 3, to register -- it registers
   once it has attached (its own limit 30 s) and taken an address (about
   10 s) -- the kernel sleeping between looks and saying at once, with its
   exit code, a daemon that ended first. Both options are read only with
   `net.userspace` on; without it, either given is said to be unread.
3. **Over the card, the root namespace's network is the daemon's.** Native
   sockets and the resolver go to it (`netstack_client::serves_a_card`, as
   `raw::is_claimed` does while it holds the kernel's NIC), the resolver's
   server is its lease's, asked each time (`card_dns`), and
   `SYS_NET_IF_INFO` says its interface. The kernel's own stack keeps the
   kernel's NIC, which a container's namespace still uses (A-Q29).
4. **The daemon says its own interface** (`netipc::OP_IFINFO`,
   `netstack_client::daemon_interface`): its card's MAC, IPv4 address, mask
   and gateway, IPv6 link-local address, DNS server and link. Every kernel
   self-test that loops traffic back to the daemon -- IPv4 and IPv6 alike
   -- or asks its DNS server, asks it, rather than deriving the daemon's
   addresses from the kernel's NIC, which over another card belong to no
   one. The kernel's multicast filter is checked only where the daemon
   serves the kernel's NIC; a driver's card keeps its own filter across the
   port.

### Alternatives

- **Every card a driver process serves, one daemon each** -- no option to
  set, but several stacks, several addresses, and sockets with no rule for
  which stack is theirs; one card a machine is what exists.
- **The service manager starting the driver process** -- where it belongs
  once there is configuration beyond the kernel's command line; the kernel
  starts the persistent netstack daemon today, so the driver process it
  needs starts beside it, and both move together later.
- **The kernel reading the card's MAC from the port registration** -- no
  daemon request, but the kernel would keep a second record of what the
  daemon holds; asking the daemon cannot drift.

5. **Served for life, both wait for the card's network rather than give
   up on it.** The driver serves the card at once, link or none -- the port
   tells the stack its carrier at every turn; the daemon waits for its first
   lease as it does for a lost one, tried again later and later yet
   (4 s doubling to 64 s) and at once when the link comes up; the kernel
   treats the card as the network from the daemon's start, and after 60 s
   without it goes on without the persistent-daemon checks, saying why. A
   cable plugged in after boot, or a WiFi network joined, is a link coming
   up like any other.

### What it does not do yet

- Nothing restarts either process, and the daemon's sockets do not survive
  its address going (it serves only while it holds a lease): known-issues
  `A-PERSISTENT-NETWORK-PROCESSES-ARE-NOT-RESTARTED`.
- `/proc/net`, `/sys/devices/net`, `netstat` and `SYS_NET_IF_CONFIG` still
  describe and configure the kernel's NIC alone: known-issues
  `A-INTERFACE-VIEWS-SHOW-ONLY-THE-KERNELS-NIC`.

### Checked by

`check_parse_serve` (every boot): `linuxdrv.serve`'s value read, and each
malformed one refused as what it is; and the linuxdrv self-test chooses the
card a `linuxdrv.serve` given names, as `serve_at_boot` will. Every boot
test's card boot (`scripts/boot-test.sh` `card_boot`, since 2026-10-30):
once the main boot has passed, the same QEMU machine is booted again with
`linuxdrv.serve=e1000e@00:05.0 net.nic=nic.e1000e` and self-tests skipped,
the spare 82574 on a network of its own (10.0.5.0/24) and its cable out
until the daemon says it waits for the card's network, and the run fails
unless Linux's e1000e served it, the daemon waited and then took its
address there, `SYS_NET_IF_INFO` said that address, every
persistent-daemon check finished without a failure (as the main boot's
checks define one), and the daemon reached over the card whatever the main
boot's reached beyond QEMU's network. Before it, a QEMU boot by hand
with `linuxdrv.serve=e1000e@00:05.0 net.nic=nic.e1000e` (2026-10-30):
Linux's e1000e serving the spare 82574 for the system's life, the daemon
attached and given an address by DHCP through it, and every
persistent-daemon check over it -- DNS, TCP, UDP, non-blocking I/O, poll,
listen/accept, IPv6 connect and datagrams, the blocking receive, connected
UDP, multicast, the native socket and resolver parity, and the ring-3 HTTP
fetch -- passing. The default boot keeps the daemon on the kernel's NIC,
whose persistent path every boot test checks, `OP_IFINFO` with it.
