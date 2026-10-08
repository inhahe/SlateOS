## 1502. The network widget shows the link the machine is on, and the traffic over it

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the desktop's widgets included a "Network" that drew only its
name and that nothing offered. "Add widget > Network" now puts one on the
desktop saying whether the machine is connected, the address it has
("10.0.2.15/24"), the traffic in and out, the gateway and the name server --
and, given two rows, the last minute's traffic as a graph, the system
monitor's own. With no network adapter it says so; on SlateOS the traffic
says "Not measured" until the kernel serves the counters it is read from.

**Where:** `gui/desktop/src/netlink.rs` (the link, read from
`/sys/devices/net`), `gui/desktop/src/widgets/network.rs` (`lines`, `layout`,
`render`, `LinkSampler`), `gui/desktop/src/widgets/monitor.rs`
(`panels_of`: any of the graphs in any box), `gui/desktop/src/widgets/accessible.rs`
(`WidgetPart::NetworkLine`), `gui/desktop/src/lib.rs` (`sample_network`,
`sample_system` for either widget, `live_readings`, `MENU_ADD_NETWORK`).

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The link's facts as well as its traffic** | the traffic alone, as the kind's name ("network monitor") suggests | The address and whether the machine is connected are what a person looks for first, and they are what SlateOS can tell today; the traffic waits on the kernel's counters. | A widget about more than traffic. |
| **The traffic is the system monitor's sample** | a sampler of its own | One reading of the counters a second, whichever widget is out, so the two cannot disagree. | -- |
| **The link read every five seconds while one is out** | on an event when it changes | No such event reaches the desktop yet (`TD-C-THE-NETWORK-INDICATOR-HAS-A-SOURCE-AND-NOTHING-TO-WAKE-IT`); a widget is put out by the user and ticks as a clock does. The tray's icon, which would be there always, still waits for the event. | A cable pulled shows up to five seconds late. |
| **Most wanted first, as many lines as fit** | every line, smaller | A wide widget shows the status, the address and the traffic; the gateway and name server need the room. | The last two are not seen in one row. |
| **"Not known here" off SlateOS** | "No network adapter" | The files are SlateOS's layout; their absence elsewhere says nothing about the machine. | -- |

**Not decided here:** whether a click on the widget opens the network
settings is part of `open-questions/C-Q36.md`.
