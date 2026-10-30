## A-PERSISTENT-NETWORK-PROCESSES-ARE-NOT-RESTARTED (lane A, 2026-10-30) — OPEN

**Status:** OPEN

**What is wrong.** With `linuxdrv.serve` and `net.nic` set (design-decisions
1594), a running system's network is two processes the kernel starts once:
the Linux driver process serving the card (`linuxdrv::serve_at_boot`) and
the persistent netstack daemon serving sockets over it
(`proc::spawn::run_persistent_netstack`). Nothing restarts either, and each
gives up at start-up on a condition a real machine meets:

| Process | Gives up when | After |
|---|---|---|
| driver (`lx_nic_serve`, `linuxdrv/emul/lx_nic.c`) | the card has no link | 10 s (`lx_nic_link`) |
| daemon (`serve_card`, `services/netstack/src/main.rs`) | no driver attaches it to the card | 30 s (`attach_card`) |
| daemon | no DHCP server answers | ~10 s (`dhcp_lease`: two exchanges, 5 tries of 1 s) |
| kernel | the daemon has not registered `net.stack` | 60 s, then a WARNING |

With `net.userspace` on the kernel's own stack has stood down, so any one
of these leaves the machine without a network until it is restarted -- and
so does either process dying later (a driver crash, a daemon fault).

**Who sees it.** No one yet: both options are off by default, and the
only configuration using them is the boot test's QEMU, whose link and DHCP
are always there. It bites the first machine whose only network is a card a
driver process serves -- the test PC's WiFi -- booted with the cable out, out
of range of its access point, or with the router still starting.

**Where.** `linuxdrv/emul/lx_nic.c` (`lx_nic_serve`'s bounded link wait),
`services/netstack/src/main.rs` (`serve_card`: the attach and the lease
before serving), `kernel/src/proc/spawn.rs` (`run_persistent_netstack`'s
registration wait), and whoever supervises services -- nothing yet.

**The proper fix.** Two halves:

1. *For-life modes that wait instead of ending.* The driver serving for
   the system's life (no `stacks=`) serves the card at once and leaves the
   link to the port, which already tells the stack its carrier at every
   turn; the daemon serving a card registers `net.stack` without a lease and
   takes one in its serving loop (`dhcp_client` from INIT, not only renewing),
   answering sockets with "network unreachable" meanwhile; and the kernel
   stops holding its boot for it. The self-tests that need a lease then wait
   for one inside their own bound.
2. *Supervision.* The service manager restarts a persistent driver process
   or daemon that ends, the driver first; the daemon re-attaches to the
   card's service name (the kit already accepts the next stack, design-
   decisions 1589). Until there is a service manager, the kernel could do
   it for these two -- it started them.
