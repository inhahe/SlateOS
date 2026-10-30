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
   holds the card, a File capability to read the card's firmware, and the
   right to register that one name. A value that is not a driver's name and
   a PCI address, a driver not loaded, a function not there or one a kernel
   driver holds is refused and said (`WARNING` at boot), the boot going on.
2. **`net.nic=<service>`** (`proc::spawn::run_persistent_netstack`): the
   persistent netstack daemon serves `serve-net <service>` -- attaching to
   the card over its port (decision 1589) -- holding the card's key and the
   right to register `net.stack` alone, not the kernel's NIC, which stays the
   kernel's. It is started even when the kernel has no network of its own,
   and given 40 s, not 3, to register: it registers once it has attached and
   taken an address.
3. **The daemon says its own interface** (`netipc::OP_IFINFO`,
   `netstack_client::daemon_interface`): its card's MAC, IPv4 address, IPv6
   link-local address and DNS server. Every kernel self-test that loops
   traffic back to the daemon, or asks its DNS server, asks it, rather than
   deriving the daemon's addresses from the kernel's NIC -- which over
   another card belong to no one (an IPv6 datagram the daemon would never
   hear left a blocking receive waiting until the boot's deadline). The
   kernel's multicast filter is checked only where the daemon serves the
   kernel's NIC; a driver's card keeps its own filter across the port.

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

### Checked by

`check_parse_serve` (every boot): `linuxdrv.serve`'s value read, and each
malformed one refused as what it is. A QEMU boot with
`linuxdrv.serve=e1000e@00:05.0 net.nic=nic.e1000e` (2026-10-30): Linux's
e1000e serving the spare 82574 for the system's life, the daemon attached
and given an address by DHCP through it, and every persistent-daemon check
over it -- DNS, TCP, UDP, non-blocking I/O, poll, listen/accept, IPv6
connect and datagrams, the blocking receive, connected UDP, multicast, and
the ring-3 HTTP fetch -- passing. The default boot keeps the daemon on the
kernel's NIC, whose persistent path every boot test checks, `OP_IFINFO`
with it.
