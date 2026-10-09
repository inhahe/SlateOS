### A-E1000-AND-RTL8139-DROP-EVERY-MULTICAST-FRAME -- 2026-10-03 -- OPEN (lane A)

**Status:** OPEN -- fixed on lane-a-wip (8c3e25af6), awaiting a boot on main

**In short:** two of the kernel's three network-card drivers tell the card to
throw away every multicast frame (one addressed to a group of machines rather
than one machine). On those cards SlateOS cannot hear anything sent to a
multicast group: no mDNS, no SSDP, no router queries -- and, worse, no IPv6
neighbour discovery, whose questions ("who has this address?") are sent to a
multicast address. Joining a group (`IP_ADD_MEMBERSHIP`) succeeds and then
nothing arrives. It has gone unnoticed because the boot test's network
daemon runs on the third card, virtio-net, which QEMU starts in a mode that
accepts everything.

**Where:**

| Driver | What it programs | Effect |
|---|---|---|
| `kernel/src/e1000.rs` (e1000/e1000e) | `RCTL = EN \| BAM \| SECRC`, multicast table `MTA[0..127]` zeroed | only broadcast and unicast to our MAC get through |
| `kernel/src/rtl8139.rs` | `RX_CFG_AM` (accept multicast) set, `MAR0-7` never written | "accept multicast" admits only frames whose hash bit is set in `MAR`, and none is |
| `kernel/src/virtio/net.rs` | negotiates only `VIRTIO_NET_F_MAC` | without `VIRTIO_NET_F_CTRL_RX` QEMU's device stays promiscuous: multicast arrives |

The active card is chosen virtio-net, then e1000, then rtl8139
(`net::send_frame`), so a machine with an Intel or Realtek card -- most
desktops -- is affected.

**To reproduce:** boot with only `-device e1000e` (drop the virtio-net and
rtl8139 devices from `scripts/boot-test.sh`), join a group from a socket and
send to it from the host; nothing arrives. QEMU's own filter
(`e1000x_rx_group_filter`, `rtl8139_do_receive`) drops it.

**The fix (as done):** a multicast filter in the kernel,
`net::mcast_filter`, as Linux's `ndo_set_rx_mode`. Every present card is
programmed (`net::recv_frame` drains them all): e1000 indexes its 4096-bit
`MTA` with bits 47:36 of the address, rtl8139 its 64-bit `MAR` with the top
six bits of the Ethernet CRC; virtio-net already passes everything. Whoever
holds the NIC decides the set:

- the netstack daemon, through the new `SYS_NET_RAW_MCAST` (1135): all-hosts
  (01:00:5e:00:00:01), all-nodes (33:33:00:00:00:01), its solicited-node
  group, and each group a socket is in, pushed at startup and whenever the
  host's groups change;
- the kernel-resident stack otherwise: the same base set plus `net::udp`'s
  joined groups, refreshed when a group enters or leaves its tables, and put
  back when the daemon releases the NIC or dies holding it.

Boot checks: `net::mcast_filter::self_test` (both hashes against an
independent model of QEMU's filters, the tables read back from the cards),
the syscall's dispatch test, and `self_test_udp_multicast` (a joined group's
address enters the filter and leaves with its last member).

**Found alongside, fixed in the same change:** the daemon's serving loop
answered no ARP request, ping, Neighbor Solicitation or ping6 -- only its
startup loop answered ARP and ping -- so a router whose ARP entry for us aged
out could no longer reach us, and IPv6 neighbours could never resolve us.
`answer_link` in `services/netstack` now answers all four, quietly.
