## TD-C-APPS-SYSINFO-WAITS-ON-A-FILESYSTEM-TREE-THAT-DOES-NOT-EXIST -- FIXED 2026-09-15

**In short:** the graphical System Information window reads ten of its twelve
categories from `/sys/hardware/...`. The kernel serves no `/sys/hardware` at
all, and lane A has now decided that several of those nodes will never exist,
because `/proc` already answers the same questions. So those categories will
report "cannot read" forever, while the data sits in `/proc` — and the crate
built specifically to read it for this app is not among the app's
dependencies.

**Date:** 2026-09-15. **Lane:** C. **Fixed 2026-09-15** -- all three steps
below are done. Six categories now read real data: CPU, memory and storage
from `/sys/devices`, network and processes from `/proc` via `procinfo`, and
IRQs and Display from `/proc/interrupts` and `/proc/monitors` once lane B's
parsers landed.

**But the last row of the table below is wrong, and I wrote it.** See
`TD-C-SYSINFO-FILED-A-NEGATIVE-IT-NEVER-CHECKED`, which supersedes it.

**How it got here, in three correct steps.**

1. `apps/sysinfo` was given a real provider reading `/sys/hardware/*`, with an
   honest comment: *"Every query is expected to fail at present: nothing in
   `kernel/`, `services/` or `userspace/` produces `/sys/hardware/*`. That is
   the point rather than a defect here — the window says it cannot read the
   hardware, which is true, and it starts reporting real values on the day a
   producer appears."* Correct when written.
2. §850 then settled that the CPU and memory trees belong at `/sys/devices`,
   not a parallel `/sys/hardware`, and those two were moved. They work.
3. Lane A recorded on 2026-09-15 that `irqs` and `display` are **deliberately
   not served**, because `/proc/interrupts` and `/proc/monitors` already
   publish them and "a second kernel answer to one question" is what §850
   exists to prevent.

Every step is right. The consequence nobody was positioned to see is that the
app's comment — *"it starts reporting real values on the day a producer
appears"* — became a **promise that will not be kept**, for a growing number of
its categories, and the app is the only place that sentence is written.

**What the kernel actually serves** (`kernel/src/fs/sysfs.rs`):
`/sys/devices/system/cpu/**`, `/sys/devices/system/memory/{total_kb,
available_kb}`, `/sys/devices/block/<name>/*` (added 2026-09-15, dd-939),
`/sys/fs/*`, `/sys/params/*`. No `/sys/hardware` node of any kind, and
deliberately no `cpufreq/` or `net/` — "no frequency source, and
`InterfaceInfo` has no name field, so both would be invented", which is the
same discipline applied one level down.

**What `/proc` publishes that these categories want:** `cpuinfo`, `meminfo`,
`uptime`, `loadavg`, `version`, `sys/kernel/hostname`, `mounts`, `diskstats`,
`net/dev`, `interrupts`, `monitors`, and the whole `<pid>/` tree.

**`procinfo` parses nearly all of it already**, and exists for this app: its
module docs say so by name — *"the two system-information programs in this
tree — `userspace/sysinfo` (the CLI) and `apps/sysinfo` (the graphical one) —
differ entirely in the second half and not at all in this one"* — and the
request that produced it was filed by this lane. **`apps/sysinfo/Cargo.toml`
does not list it.** `apps/procexplorer` and `apps/sysmonitor` both do, as of
today.

| category | source available now | in `procinfo`? |
|---|---|---|
| System summary, CPU, memory | `/proc/{cpuinfo,meminfo,uptime,loadavg,version}` | yes |
| Storage | `/proc/mounts`, `/proc/diskstats`, `/sys/devices/block` | yes |
| Network | `/proc/net/dev` | yes |
| Processes | `/proc/<pid>/{stat,statm,cmdline}` | yes |
| IRQs | `/proc/interrupts` | **no parser** |
| Display | `/proc/monitors` | **no parser** |
| PCI, USB, sound, I/O ports, DMA, memory map, drivers, services, startup | ~~nothing publishes these~~ **WRONG -- a claim about the whole kernel made from looking at one filesystem** | see below |

**The fix, in order:**

1. Add `procinfo` to `apps/sysinfo` and serve the first four rows from it. That
   is most of the window, and it is real data today.
2. For IRQs and display, ask lane B for `ProcFs::interrupts()` and
   `ProcFs::monitors()` rather than parsing them here — `procinfo` exists
   precisely so two programs do not grow two parsers of one file, and
   `userspace/` will want the same two.
3. For the last row, keep saying it cannot be read, and **delete the
   `/sys/hardware/*` constants that back them** so the next reader is not
   waiting on a path that was decided against. The sentence about "the day a
   producer appears" has to go with them: it is a promise this project has
   declined to make.

**Why this is filed rather than done:** it is the largest remaining item in
this sweep and wants its own commits. Everything needed to start is above, and
nothing about it is blocked.
