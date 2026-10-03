### A-E1000-AND-RTL8139-DROP-EVERY-MULTICAST-FRAME -- 2026-10-03 -- OPEN (lane A)

**Status:** OPEN (lane A) -- being fixed next, on `lane-a-wip`.

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

**The fix:** a raw-NIC multicast filter the netstack daemon sets, as Linux's
`ndo_set_rx_mode` does: a new `SYS_NET_RAW_*` call taking the list of
multicast MAC addresses to accept, which the active driver programs into its
hash filter (e1000: bits 47:36 of the address index the 4096-bit `MTA`;
rtl8139: the top six bits of the Ethernet CRC index the 64-bit `MAR`). The
daemon always includes the all-hosts group (01:00:5e:00:00:01), IPv6
all-nodes (33:33:00:00:00:01) and its solicited-node group
(33:33:ff + the last three bytes of its address), and adds or removes a
group's address as the host joins or leaves it. Releasing the NIC clears the
filter.
