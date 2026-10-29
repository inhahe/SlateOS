## A-NETSTACK-SERVES-SOCKETS-ONLY-OVER-THE-KERNELS-NIC (lane A, 2026-10-29) — OPEN

**Status:** OPEN

**What is wrong.** The netstack daemon's serving modes serve sockets over
a card a Linux driver process serves (design-decisions 1589) when named one
(`serve-dns <service>`, `serve-net <service>`; narrowed 2026-10-29, the
boot test's linuxdrv self-test resolving a name through the spare 82574 so).
But nothing names one -- the persistent daemon the kernel starts serves the
kernel's NIC -- and over a port the daemon waits as over the kernel's NIC,
by sleeping 5 ms after reading nothing, not by the driver's doorbell, and
reads nothing between requests. So a card only a driver process drives
carries no socket's traffic on a running system, and would carry it slowly.

What stands between the two:

- **No one NIC interface.** The daemon's NIC calls pick per call: `raw_tx`
  and `raw_rx` go to the port when one is attached; `query_if_info`,
  `raw_open`, `raw_close` and `sync_mcast_filter` are the kernel's only.
  Over a port the filter goes to `port::set_mcast`, and is pushed only once
  the driver's answer (`port::mcast_settled`) says it is in force -- a
  refused list retried at the next change, as the kernel path does, and the
  list given again to a card attached anew. The proper shape is one NIC chosen once at start-up -- the kernel's,
  or a port named on the command line or by configuration -- behind which
  every call goes, with the address configuration coming from the kernel's
  for the one and the daemon's own DHCP for the other.
- **The link is learnt and not acted on.** `port::link()` reports the
  driver's word, but nothing in the daemon reacts: a link that comes back
  should make the daemon confirm its lease (RFC 2131 INIT-REBOOT) and send
  its gratuitous ARP and IPv6 neighbour advertisements again; a link down
  should fail sends at once rather than let them queue in the driver.
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
