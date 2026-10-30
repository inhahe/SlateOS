## A-NETSTACK-SERVES-SOCKETS-ONLY-OVER-THE-KERNELS-NIC (lane A, 2026-10-29) — FIXED 2026-10-30

**Status:** FIXED 2026-10-30 -- design-decisions 1594, on main at 83cc422be, which passed the boot test; every boot test checks it since (its card boot)

**What is wrong.** The netstack daemon's serving modes serve sockets over
a card a Linux driver process serves (design-decisions 1589) when named one
(`serve-dns <service>`, `serve-net <service>`; narrowed 2026-10-29, the
boot test's linuxdrv self-test resolving a name through the spare 82574 so).
But nothing names one -- the persistent daemon the kernel starts serves the
kernel's NIC -- so a card only a driver process drives carries no socket's
traffic on a running system. (Over a port the daemon waits by the driver's
doorbell and serves the card between requests, and acts on its link -- a
frame sent while it is down dropped, the lease confirmed and the
addresses announced when it comes back -- and keeps the card's lease in
the serving loop, renewing it at T1, rebinding at T2, confirming it by
INIT-REBOOT after a flap (`netproto::dhcp_client`) -- since 2026-10-29,
every use of the card going through one module, `nic`. Over the kernel's NIC, which has
no wait, it still sleeps 5 ms after reading nothing, and between requests
reads the NIC when a client comes or every half second.)

What stands between the two:

- **Choosing a card.** Nothing yet says which `nic.<name>` the daemon
  should attach to; with several cards, which are the daemon's.

**Where.** Whoever starts the persistent daemon -- the kernel today
(`proc::spawn`'s `serve-net`), the service manager later -- and
`services/netstack/src/main.rs`'s start-up, which takes the card named.

**Who sees it.** No one yet: no machine's only card is a driver process's.
It bites the first time one is -- the test PC's WiFi, say, once a Linux
WiFi driver serves it.

**The proper fix.** The daemon told which card is its by configuration
-- a `nic.<name>` in the network's settings, or every card a driver
process serves, one daemon each -- and started with it, the kernel's NIC
the fallback where no card is named.

**Fixed (lane A, 2026-10-30, design-decisions 1594).** Two boot options,
both off by default: `linuxdrv.serve=<driver>@<BB:DD.F>` starts that Linux
driver process at boot, serving the card as `nic.<driver>` for the
system's life; `net.nic=<service>` has the persistent netstack daemon serve
sockets over it, holding its key, not the kernel's NIC. The daemon now says
its own interface (`netipc::OP_IFINFO`), which the kernel's self-tests ask
rather than assume the kernel's NIC. Checked by a QEMU boot with both set:
every persistent-daemon check passed over Linux's e1000e serving the spare
82574.
