## 1496. The system monitor widget takes the resource monitor's graphs, and the resource monitor goes

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous), under the operator's rule in §1423 &middot; **Lane:** C

**In short:** the desktop carried two system monitors. One, the widget you
add from the desktop's menu, showed three bars -- processor, memory, disk
space. The other, `gui/desktop/src/resmon.rs`, drew graphs of the last
minute and had never been put on the screen by anything. The operator's
rule for dead code with a live twin (§1423) is that the twin takes what only
the dead copy could do, and then the copy goes. So the widget, given room --
"Size" on its menu now offers two rows -- draws four graphs of the last
minute (processor, memory, the disks' activity, the network), each saying
what it reads now and the most it read in the minute, with the disk-space bar
at its foot; and `resmon.rs` is deleted. At its smaller sizes it is the three
bars it was.

**Where:** `gui/desktop/src/widgets/monitor.rs` (`Graph`, `Graphs`, `Sampler`,
`disk_busy_fraction`, `network_rate`, `panels`, `render`),
`gui/desktop/src/widgets.rs` (`monitor_layout`, `render_meter`,
`WidgetKind::sizes`, `LiveReadings::graphs`), `gui/desktop/src/widgets/accessible.rs`
(`WidgetPart::Graph`), `gui/desktop/src/lib.rs` (`sample_system`,
`live_readings`); the counters asked of lane A in
`requests/c-a-the-system-monitor-reads-disk-and-network-counters-the-kernel-does-not-serve.md`.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The graphs at two rows of the grid; the bars below that** | a mode switch on the widget, as `resmon.rs` had (compact and expanded) | Size is already how a widget is asked to show more -- a widget's menu offers "Size" since 2026-10-06, and a calendar, a note and a photo frame each draw more given more; a second control for the same question is one more thing to find. | A user who wants graphs in one row cannot have them -- four graphs in one row's height are too short to read a shape from. |
| **Four graphs: the four `resmon.rs` drew** | its six: it also kept GPU use and temperature | Those two were stored and never drawn -- an ability nobody could see -- and nothing on SlateOS or a Linux host reports either in a form this could read. | When something does, they are a graph each to add. |
| **Disk activity is the busiest disk's** | the average over disks | A graph of how hard the disks are working should read one disk flat out as flat out; an average with an idle second disk halves it. A partition is never busier than its disk, so the partitions `/proc/diskstats` lists change nothing. | A machine with one busy disk and nine idle ones looks as busy as one with ten busy disks. |
| **The network plotted on its minute's peak** | on a fixed scale | A rate has no whole: on a fixed scale a quiet link is a line along the floor and a fast one runs off the top. | The height of the line is not comparable from one minute to the next; the heading's numbers are what to compare. |
| **A reading nothing measured empties its graph** | the line kept where it stopped | A line that stops reads as one that held. | A source that drops out for one sample loses the minute behind it. |
| **The disks and network read in Linux's formats, through `procinfo`** | SlateOS's own `/proc/diskstats` table and `/proc/net` file | They are Linux's interface, which the ABI baseline (§1) aims at and every ported tool reads; reading SlateOS's own tables would tie the widget to files that should become Linux's. | On SlateOS today those two graphs say "not measured" until lane A serves the counters (the request above). |
| **Sampling only while a monitor is out, and no sooner than half a second after the last** | at every widget's due tick, as before | With no monitor out there is nothing to read `/proc` for; and a clock and a photo frame falling due a moment apart would crowd the graph's minute and measure the processor over too short an interval. | The first sample after a monitor is added measures nothing for a second, as at start-up. |

**What it cost.** `resmon.rs`'s 35 palette defects (`scripts/reintro-palette.py`)
went with it. The judgements they held that still have a drawing -- each graph
pinned to the role it names, none the accent, the guide lines as furniture, a
heading's label dimmer than its reading, everything under the widget's veil --
are tests in `widgets/monitor_tests.rs` and defects against `monitor.rs`.
