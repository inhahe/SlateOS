## 641. `fs::perfmon` projects `kstat`'s history instead of keeping its own, and the fields no source can fill are deleted rather than zeroed

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane A
**Lane:** A

**In short:** the kernel has a "performance monitor" — the thing that would draw
the CPU and memory graphs in a Task Manager. It reported **zero samples, on
every boot, forever**, because the four functions that were supposed to feed it
were never called by anything. Meanwhile a *different* part of the kernel
(`kstat`) had been recording CPU and memory once a second since boot, into a
buffer nobody was showing. The fix is to delete the empty store and have the
monitor read the buffer that was already full. The judgment call worth recording
is the second half: several of the monitor's fields — CPU temperature, core
frequency, a user-vs-system time split, disk and network history — had **no
source anywhere in the kernel**, so they are removed outright rather than kept
and filled with zeros.

### The situation

`kernel/src/fs/perfmon.rs` owned four `Vec` histories and four recorders,
`record_cpu` / `record_mem` / `record_disk` / `record_net`. Nothing outside the
module called any of them; the only caller was the module's own self-test, which
passed fixture values and asserted them back. So `/proc/perfmon` printed

```
CPU samples: 0
Mem samples: 0
Disk samples:0
Net samples: 0
```

on a machine that had been up for an hour, and the `perfmon cpu` shell arm
answered "No CPU samples". This is category 3 of
`known-issues.md` → `A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH`: not a
missing feature but an **unfed table**, which is worse, because a zero that
nothing can raise is indistinguishable from a measured zero.

`kernel/src/kstat.rs` had, the whole time, been sampling free frames, total
frames, heap bytes, pressure score, runnable and live task counts, per-CPU
utilisation, context switches and interrupts into a 60-entry ring, once a
second, from the timer softirq.

### The decision

**Project at read time; store only policy.** `cpu_history()`, `mem_history()`,
`cpu_latest()`, `mem_latest()` and `active_alerts()` all compute from
`kstat::recent()` when called. The module's `STATE` now holds two numbers — the
CPU and memory alert thresholds — because a threshold is a *user's choice* and
not a measurement, and choices are the only thing this module has any business
owning.

This is the same shape as §-less precedents `fs::pagecache` and `fs::netdev`
(both logged in that `known-issues.md` entry's burn-down): where a subsystem
already keeps the numbers, join the two halves at read time rather than by
calling an accounting function on the producer's hot path.

**Alternatives rejected:**

| Option | Why not |
|---|---|
| Wire `record_*` into a second sampler | The constraint that shapes the real sampler — softirq context, so no allocation and no blocking lock — makes a second one strictly worse than one. Two samplers of the same event are two numbers that can disagree, with nothing to say which is right. |
| Call `record_cpu` from `kstat::sample()` | Puts this module's `PreemptSpinMutex` and a `Vec` push inside the timer softirq. Both are forbidden there for the reason `kstat` documents at length: a blocking acquire of a lock the interrupted code holds can never succeed. |
| Keep the histories, feed them from a kernel thread | A third copy of a ring that already exists, sampled at a third instant, so `kstat` and `perfmon` would disagree about the same second. |
| Leave it and file it | It has been filed since 2026-08-26. The cost of leaving it is that `/proc/perfmon` keeps publishing zeros that read as measurements. |

### The part that is a genuine trade: deleting fields

`CpuSample` lost `system_pct`, `user_pct`, `freq_mhz`, `temp_mc`,
`process_count` and `thread_count`. `MemSample` lost `cached_bytes`,
`swap_used_bytes` and `page_faults`. `DiskSample` and `NetSample` are gone
entirely, with `disk_history()` and `net_history()`.

**For deleting:** every one of these had no source. The scheduler publishes
`(total_ticks, idle_ticks)` per CPU and nothing finer, so a user/system split
cannot be computed. There is no frequency driver and no thermal driver. Disk and
network counters live behind spin locks the softirq cannot take. A field that
can only ever be zero is not an incomplete feature; it is a **fabrication
surface** — the next person to touch it fills it with something plausible, and
now the graph has a line on it that is not measurement. This is exactly the
`irqstat` argument recorded in that same burn-down, where deleting the mutators
was right *because they were the wrong shape*, not merely uncalled.

**Against deleting:** the field names were a record of an intent — someone meant
this monitor to show temperature one day — and deleting them loses that. A
future thermal driver now has to re-add `temp_mc` rather than find it waiting.

**Why deleting won:** the intent is preserved better in prose than in a struct
field, so the module doc now says explicitly what is absent and *why it cannot
be filled from here*, which is more than the field ever said. And the asymmetry
of harm is decisive: an absent field costs one small edit when a source appears;
a permanently-zero field costs a wrong number on a screen for as long as nobody
notices, which is how long it had already been.

Disk and network get a pointer rather than silence: `/proc/perfmon` now ends
with a line saying to read `/proc/diskstat` and `/proc/netdev`, which carry the
real cumulative counters. What does not exist is a *time series* of them.

### Two settings became read-only, which is a user-visible change

`perfmon interval <ms>` and `set_max_samples` are gone. They stored a value,
`get_config()` read it back, and nothing sampled any faster — because this
module had no sampler. **A knob that moves and changes nothing is worse than a
fixed value**, because it answers "did that work?" with yes. The interval and
depth are now reported from `kstat::sample_interval_ms()` and
`kstat::history_depth()`, labelled in `perfmon config` as not settable here.

Replacing them, `perfmon cpu-alert <pct>` and `perfmon mem-alert <pct>` are new,
which makes the two threshold setters reachable — they were among the twelve
unreachable mutators this module contributed.

### Alerts lost their identity and their dismiss verb

The old design appended an `Alert` row per over-threshold sample, each with an
`id` and a `dismissed` flag, and offered `perfmon dismiss <id|all>`. A CPU
pinned at 100% therefore produced a growing list of identical rows, and
dismissing them all made a still-overloaded machine report itself healthy.

Alerts are now derived from the newest sample each time they are asked for, so
they have no identity and cannot be dismissed: **a condition that is still true
cannot be dismissed, and one that has passed disappears without being told to.**
The cost is that a spike which has already ended leaves no trace — the old
design could in principle have shown you a burst from a minute ago. It never
did, because nothing recorded one; and the honest place for that feature is a
scan of the projected history, which is now possible and was not before.

### Effect on the burn-down metric

`scripts/check-unreachable-mutators.py`: **515 → 503** unreachable mutators, 220
→ 219 modules, 1941 → 1928 mutators. Read that with the same care the
`irqstat` row asks for — most of the drop is deletion of functions that should
not exist, not callers being wired up. Two of the twelve (`set_cpu_alert`,
`set_mem_alert`) are genuine wirings.

One incidental finding, recorded because it will recur: the scanner matches on
**call text**, so passing `perfmon::set_cpu_alert` as a function item to a
shared helper left both setters counted as callerless. Wrapping in a closure
fixes the metric but trips `clippy::redundant_closure`; the resolution was to
split the helper into a parse half and a report half so each shell arm calls its
setter directly. That is better code anyway, but the reason it was written that
way is the scanner, and a later reader tidying it back into a callback would
silently re-break the measurement.

### How to reverse

The projection is four short functions (`project_cpu`, `project_mem`, `window`,
`tick_to_ns`) in `kernel/src/fs/perfmon.rs`. Restoring stored histories means
re-adding the `Vec`s and finding a caller for the recorders — which is the thing
that was never done in the first place, and the reason to check before trying is
that the softirq cannot be that caller.
