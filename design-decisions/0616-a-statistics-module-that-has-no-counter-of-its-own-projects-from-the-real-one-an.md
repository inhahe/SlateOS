## 616. A statistics module that has no counter of its own projects from the real one — and its unreachable mutators are deleted, not kept

**Date:** 2026-08-26
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** `/proc/irqstat` is the file you read to see how many times each
hardware interrupt has fired. It had its own private table of those numbers,
which nothing in the kernel ever wrote to — so it reported either invented
figures or zeros, on a machine taking thousands of interrupts a second. The
choice was between wiring the interrupt handler to write into that table, or
throwing the table away and computing the file from the counter the kernel
already keeps. The second was chosen, and — unusually — the now-unused functions
were deleted rather than left in place as evidence of a gap.

**Decision 1: project, do not add a second counter.**

`idt::dispatch_vector` already bumps one relaxed atomic per interrupt at the one
point every hardware vector passes through (§614). `fs::irqstat` now derives
every number it prints from that array, on demand, and owns no state at all.

The alternative — call `irqstat::record(cpu, irq)` from the interrupt path — is
the obvious repair and is wrong twice over:

* **It makes two counters of one event.** Two counters can disagree, and when
  they do there is nothing to say which is right. One missed call site, one
  early return, one new vector whose author does not know this module exists,
  and `/proc/irqstat` and `counters` report different interrupt totals. A
  projection cannot drift, because there is only ever one number.
* **It costs a lock in an ISR.** This module kept its table behind a spin lock.
  The existing count is one relaxed `fetch_add`; the alternative is a lock
  acquisition on the interrupt path, which is the "long operation under
  IRQs-disabled" anti-pattern in miniature.

This is the third projection of this shape (`pagecache`, `netdev`), and the
argument has been the same each time: where the subsystem already keeps the
number for free, join the two halves at *read* time.

**Decision 2: delete the unreachable mutators, against the standing rule.**

`known-issues.md`'s `A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH` says
plainly: do not fix an unreachable mutator by deleting it, because the function
is usually right and the missing caller is the defect, and deleting it removes
the evidence that a number is unfed. That rule is good and was followed for
`pagecache` and `netdev`, whose `record_*` functions remain — reserved for a
per-device source that could plausibly exist later.

It was not followed here, because the premise does not hold. These functions
were the wrong *shape*, not merely uncalled:

| removed | why no caller could ever be right |
|---|---|
| `record(cpu, irq)` | presumes a per-CPU, per-line table; `VECTOR_COUNTS` is one flat global array, so the `cpu` argument has no source |
| `record_latency` | nothing samples ISR latency anywhere in the kernel |
| `mark_spurious(cpu, irq)` | per-line spurious counts have no source; the APIC spurious vector is a single global count |
| `IrqLine::affinity_mask`, `CpuIrqState` | pure fabrication surface — the fields the fictional seed data used to fill |

Keeping them would not have preserved evidence of an unfed number — the number
is now fed, from the real counter. It would have preserved an *invitation to the
wrong fix*, sitting one call away from an ISR, which is exactly decision 1's
failure mode. The rule's purpose is to stop a gap being papered over; here
deleting is what closes the gap, and keeping is what would leave it open.

The `IrqType` enum went the same way, for the same reason.
`Timer`/`Keyboard`/`Cascade`/`Serial`/`Disk`/`Network`/`Usb`/`Gpu` is a claim
about which device sits behind an IOAPIC input. The kernel does not know that —
it is a runtime property of ACPI routing and driver binding — so the enum could
only ever have been populated by guessing. It is now
`Timer`/`Device`/`Ipi`/`Spurious`/`Unassigned`, derived from the vector number,
which the kernel *does* know because it assigned it.

**What is still missing, and is reported as missing rather than as zero.**
Per-CPU attribution and ISR latency have no source. The module emits neither,
and `/proc/irqstat` carries the line
`per_cpu: unavailable (interrupt counters are not per-CPU)` instead of a table
of `cpu0: total=0` rows. A zero is not a blank — it is a claim that the machine
measured zero, and "every AP is idle" is a far more misleading thing to publish
than "we do not know". Making the counters per-CPU is a real option that would
also cut cross-CPU cache-line contention, but it changes an interrupt hot path
and belongs to its own change with its own benchmark.

**The cost accepted.** `irq_lines()` and `totals()` each walk 256 array slots
per call instead of reading a cached table. Both are called only from `/proc`
reads and a hand-run shell command, never from an interrupt path, so this is
free in every case that matters.

**Where it bites.** `kernel/src/fs/irqstat.rs` (rewritten);
`kernel/src/fs/procfs.rs` (`gen_irqstat`); `kernel/src/kshell.rs`
(`cmd_irqstat` — the `register` subcommand is gone with the table it wrote to);
`kernel/src/main.rs` (boot battery note). Verified by a 6-test boot self-test
that asserts against *live* data — including that the timer vector's count is
non-zero and is at least `apic::tick_count()` — which is the assertion a
fixture-based test structurally cannot make, and the one that detects the
projection having become disconnected from its counter.
