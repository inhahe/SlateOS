## A-PERSISTENT-NETWORK-PROCESSES-ARE-NOT-RESTARTED (lane A, 2026-10-30) — OPEN

**Status:** OPEN

**What is wrong.** With `linuxdrv.serve` and `net.nic` set (design-decisions
1594), a running system's network is two processes the kernel starts once:
the Linux driver process serving the card (`linuxdrv::serve_at_boot`) and
the persistent netstack daemon serving sockets over it
(`proc::spawn::run_persistent_netstack`). Two things are still wrong:

1. **Nothing restarts either.** A driver crash, a daemon fault, or a
   driver that never registered its card (the daemon gives up attaching
   after 30 s, `attach_card`) leaves the machine without a network until it
   is restarted. With `net.userspace` on, the kernel's own stack has stood
   down on the root namespace, so nothing else carries its sockets.
2. **Sockets do not survive the address going.** The daemon registers
   `net.stack` only once it holds a lease, and leaves its serving loop when
   the lease is lost (refused, or run out unrenewed), taking a new one before
   it serves again (`run_dns_service`, `retake_lease`). Every open socket is
   lost then, even when the new lease gives the same address; and until the
   first lease, a program asking for a socket is told there is no network
   service at all rather than that the network is unreachable.

**Fixed already (2026-10-30).** Neither gives up at start-up any longer on a
card with no link or a network with no DHCP answer: the driver serving for
the system's life serves the card at once, its link told to the stack as it
comes (`lx_nic_serve`), and the daemon waits for its first lease as it does
for a lost one -- tried again later and later yet, and at once when the link
comes up (`serve_card`, `rest_until_link_up`); the kernel goes on after 60 s
without the persistent-daemon checks, the card still the network
(`set_serves_a_card` at the daemon's start). Every boot test's card boot
starts with the card's cable out.

**Who sees it.** No one yet: both options are off by default. The first is
the test PC once its WiFi carries its network -- a driver that faults, or a
lease the access point refuses.

**Where.** Whoever supervises services -- nothing yet -- and, for 2,
`services/netstack/src/main.rs`'s serving loop (`IfInfo` is fixed for a
`run_dns_service` call: the address, mask, gateway and DNS server are read
from it throughout).

**The proper fix.**

1. *Supervision.* The service manager restarts a persistent driver process
   or daemon that ends, the driver first; the daemon re-attaches to the
   card's service name (the kit already accepts the next stack, design-
   decisions 1589). Until there is a service manager, the kernel could do
   it for these two -- it started them.
2. *The address as state of the serving loop.* The daemon registers
   `net.stack` at once and holds its interface's addresses as state the loop
   changes: the first lease taken there (`dhcp_client` from INIT --
   DISCOVER, OFFER, REQUEST, ACK -- not only renewing), a lost one given up
   and a new one taken without leaving the loop, the sockets bound to the old
   address reset and the others kept; meanwhile a connect answers "network
   unreachable".
