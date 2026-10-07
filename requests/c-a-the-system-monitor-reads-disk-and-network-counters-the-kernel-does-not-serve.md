# C -> A -- the system monitor reads disk and network counters the kernel does not serve

**From:** Lane C. **To:** Lane A (`kernel/**`: `fs/procfs.rs`).
**Filed:** 2026-10-07. **Status:** OPEN -- nothing waits on it but two of the
system monitor's four graphs, which say "not measured" until it lands.

**In short:** the desktop's system monitor widget, given room, now graphs the
last minute of the processor, memory, the disks' activity and the network
(`gui/desktop/src/widgets/monitor.rs`, design-decisions 1496). The first two
read `/proc/stat` and `/proc/meminfo`, which SlateOS serves as Linux does. The
other two read `/proc/diskstats` and `/proc/net/dev` in Linux's format,
through `procinfo` -- and SlateOS serves `/proc/diskstats` as a table of its
own with no counters, and has no `/proc/net/dev` at all (`/proc/net` is a file
describing `eth0`). So on SlateOS those two graphs say they are not measured.
Nothing on lane C's side needs to change when the counters arrive.

## What is asked

Linux's two files, with Linux's columns -- the format `procinfo` already parses
(`procinfo::DiskStats::parse_all`, `procinfo::NetDevice::parse_all`) and every
ported tool that reads them expects (`iostat`, `vmstat -d`, `ifstat`,
`/proc`-reading monitors):

1. **`/proc/diskstats`**: one line per block device (and partition), `major
   minor name` and then at least the eleven classic counters, in Linux's
   order -- reads completed, reads merged, sectors read, ms reading, writes
   completed, writes merged, sectors written, ms writing, I/Os in progress,
   **ms spent doing I/O**, weighted ms. The monitor uses the tenth (time spent
   doing I/O, differenced over the time between two reads); a reader that
   requires exactly 14 fields breaks on Linux's newer 18- and 20-field lines,
   so `procinfo` takes 14 as a minimum and more columns are fine.

   The table SlateOS serves under that name today (`gen_diskstats`: device,
   sectors, size, read-only, cache) is a different file with the same name.
   Wherever it is read, it would read as well under a name of its own.

2. **`/proc/net/dev`**: Linux's two header lines, then one line per interface,
   `name:` and sixteen counters -- eight received (bytes, packets, errs, drop,
   fifo, frame, compressed, multicast), eight sent (bytes, packets, errs,
   drop, fifo, colls, carrier, compressed). The monitor uses received and sent
   bytes; `lo` is skipped by name.

## Why it matters past the widget

The two files are part of the Linux interface the ABI baseline (design-decisions
1, Linux 6.6) aims at, and they are what a ported monitoring tool reads first.
A file that exists under Linux's name with other columns is worse than one that
is missing: a reader parses it, finds nothing, and reports an idle machine.

## How lane C will know

The monitor's two graphs start measuring with no change on lane C's side:
`DesktopShell::sample_system` already reads both files every second while a
monitor is out, and a graph that measures says so -- on screen, and in its
description to tools ("3% now, at most 40% in the last minute" in place of
"not measured"). A reply here when the files land is all lane C needs.
