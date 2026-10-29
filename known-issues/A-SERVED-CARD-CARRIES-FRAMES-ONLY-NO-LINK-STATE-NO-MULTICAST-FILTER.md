## A-SERVED-CARD-CARRIES-FRAMES-ONLY-NO-LINK-STATE-NO-MULTICAST-FILTER (lane A, 2026-10-29) — OPEN

**Status:** OPEN -- fixed on lane-a-wip (the port's version 2: link and
multicast messages, design-decisions 1589 item 7), awaiting a boot on main.
What is left on the daemon's side -- one NIC interface, a port in its
serving mode, the link acted on -- is
`A-NETSTACK-SERVES-SOCKETS-ONLY-OVER-THE-KERNELS-NIC`.

**What is wrong.** A network card a Linux driver process serves to the
netstack daemon (design-decisions 1589) carries frames and nothing else.
Two things a network stack needs from its card do not cross the port:

- **Link state.** The driver serves the card once its link is up
  (`lx_nic_link`), and a stack learns of the link going down only by frames
  ceasing. It cannot tell a cable pulled from a quiet network.
- **The multicast filter.** The daemon programs the cards' filter through
  the kernel (`sync_mcast_filter` -> `SYS_NET_RAW_MCAST`, which needs the
  kernel's NIC claimed). A served card keeps the filter its driver set at
  probe -- its own address and broadcast -- so IPv6 neighbour discovery,
  MLD and IGMP queries, and multicast sockets get nothing through it.
  The daemon's other calls on its NIC (`query_if_info`, `raw_open`,
  `raw_close`) are the kernel's NIC's too; only `raw_tx` and `raw_rx` reach
  a port.

**Where.** `netipc/src/nicport.rs` (the protocol: only the attach message
and the ring), `linuxdrv/kit/src/nicport.rs`, `linuxdrv/emul/lx_nic.c`,
`services/netstack/src/main.rs` (`sync_mcast_filter`, `raw_*`) and
`services/netstack/src/port.rs`.

**Who sees it.** Nothing yet: the only stack attached to a served card is
`nic-check` (the boot test's), which uses unicast and broadcast alone. It
bites the first time `serve-net` runs over a port.

**The proper fix.** Messages on the connection besides the doorbell --
the driver telling link changes (from Linux's `netif_carrier_on/off`, a
netdevice notifier in `lx_nic.c`), the stack setting the card's multicast
list (applied with `dev_mc_add`/`dev_set_rx_mode`) -- and one NIC
interface in the daemon, chosen once, behind which every call goes to the
kernel's NIC or the port, rather than per-call checks.
