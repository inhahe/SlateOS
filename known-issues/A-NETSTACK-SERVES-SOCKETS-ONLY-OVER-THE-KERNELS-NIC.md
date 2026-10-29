## A-NETSTACK-SERVES-SOCKETS-ONLY-OVER-THE-KERNELS-NIC (lane A, 2026-10-29) — OPEN

**Status:** OPEN

**What is wrong.** The netstack daemon's serving modes serve sockets over
a card a Linux driver process serves (design-decisions 1589) when named one
(`serve-dns <service>`; narrowed 2026-10-29, the
boot test's linuxdrv self-test resolving a name through the spare 82574 so).
But nothing names one -- the persistent daemon the kernel starts serves the
kernel's NIC -- so a card only a driver process drives carries no socket's
traffic on a running system. (Over a port the daemon waits by the driver's
doorbell and serves the card between requests, and acts on its link -- a
send refused at once while it is down, the lease confirmed and the
addresses announced when it comes back -- since 2026-10-29, every use of
the card going through one module, `nic`. Over the kernel's NIC, which has
no wait, it still sleeps 5 ms after reading nothing, and between requests
reads the NIC when a client comes or every half second.)

What stands between the two:

- **Renewing the card's lease.** Over a card the address is the daemon's
  own, taken once by DHCP and confirmed when the link comes back; nothing
  renews it as it nears its end (RFC 2131 RENEWING at T1, REBINDING at
  T2), so the persistent `serve-net` refuses a card until something does.
  The bounded `serve-dns` lives seconds. Renewal and the lease's
  confirmation when the link comes back (`relink`) should be states of the
  serving loop: `relink` now blocks it for the DHCP exchange -- up to 5 s
  when no server answers -- serving no client meanwhile and answering only
  ARP, neighbour solicitations and pings, a session's TCP segments left to
  their retransmission.
- **Choosing a card.** Nothing yet says which `nic.<name>` the daemon
  should attach to; with several cards, which are the daemon's.

**Where.** `services/netstack/src/main.rs` (`main`'s serving path,
`query_if_info`, `raw_*`, `sync_mcast_filter`) and
`services/netstack/src/port.rs`.

**Who sees it.** No one yet: no machine's only card is a driver process's.
It bites the first time one is -- the test PC's WiFi, say, once a Linux
WiFi driver serves it.

**The proper fix.** As above: a `nic` module in the daemon with one
implementation per kind, chosen once; the serving mode attaching to a port
when told to; the link's changes acted on as a host does on a cable
plugged back.
