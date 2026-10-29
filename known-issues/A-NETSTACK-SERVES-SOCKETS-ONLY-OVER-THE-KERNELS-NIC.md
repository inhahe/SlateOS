## A-NETSTACK-SERVES-SOCKETS-ONLY-OVER-THE-KERNELS-NIC (lane A, 2026-10-29) — OPEN

**Status:** OPEN

**What is wrong.** The netstack daemon's serving modes serve sockets over
a card a Linux driver process serves (design-decisions 1589) when named one
(`serve-dns <service>`; narrowed 2026-10-29, the
boot test's linuxdrv self-test resolving a name through the spare 82574 so).
But nothing names one -- the persistent daemon the kernel starts serves the
kernel's NIC -- so a card only a driver process drives carries no socket's
traffic on a running system. (Over a port the daemon waits by the driver's
doorbell and serves the card between requests, since 2026-10-29; over the
kernel's NIC it still sleeps 5 ms after reading nothing and reads nothing
between requests, that NIC having no wait.)

What stands between the two:

- **No one NIC interface.** The NIC is chosen once at start-up now -- the
  kernel's (`query_if_info`, `raw_open`) or the port named (`serve_card`,
  DHCP) -- and let go once (`release_nic`), but each call still picks its
  path by whether a port is attached: `raw_tx`, `raw_rx`, `nic_idle`,
  `next_client`, `sync_mcast_filter`. The proper shape is a `nic` module
  with one implementation per kind, chosen once, behind which every call
  goes.
- **The link is learnt and not acted on.** `port::link()` reports the
  driver's word, but nothing in the daemon reacts: a link that comes back
  should make the daemon confirm its lease (RFC 2131 INIT-REBOOT) and send
  its gratuitous ARP and IPv6 neighbour advertisements again; a link down
  should fail sends at once rather than let them queue in the driver.
- **Renewing the card's lease.** Over a card the address is the daemon's
  own, taken once by DHCP; nothing renews it (RFC 2131 RENEWING at T1,
  REBINDING at T2), so the persistent `serve-net` refuses a card until
  something does. The bounded `serve-dns` lives seconds.
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
