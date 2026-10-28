## TD-A-LINUXDRV-JIFFIES-STAND-STILL-IN-A-BARE-SPIN (lane A, 2026-10-28) — OPEN

**Status:** OPEN

**What:** In a Linux driver process (`linuxdrv/`, design-decisions 1578,
1580), `jiffies` is a plain variable the layer brings up to the monotonic
clock at chosen points: at every switch between Linux's tasks, before a
timed sleep reckons its end, after every udelay/ndelay/mdelay, and every
16th `cpu_relax()` (the shadow `<asm/vdso/processor.h>`). On Linux the timer
interrupt moves it whatever the CPU is doing. So a loop that waits for
`jiffies` and calls none of those -- `while (time_before(jiffies, end)) { if
(readl(reg) & READY) break; }`, or one that spins with `barrier()` or
`READ_ONCE()` alone -- never sees time pass, and spins forever where Linux's
would time out.

**Where:** `linuxdrv/emul/lx_emul.c` (`lx_emul_tick`, `lx_emul_jiffies_now`,
`slx_cpu_relax`), `linuxdrv/emul/lx_sched.c`, `linuxdrv/shadow/processor.h`.

**How to reproduce:** in a driver process, `unsigned long end = jiffies + 2;
while (time_before(jiffies, end)) barrier();` -- it never returns (the
hello driver's test does the same spin deliberately to make `jiffies` stale,
bounded by the clock instead).

**Proper fix:** a `jiffies` the kernel keeps current, as Linux's own is: a
page the kernel writes at its timer tick, mapped read-only into the driver
process where the link puts `jiffies_64` (`linuxdrv/link.ld`), holding
Linux's count at the layer's HZ (250) from `INITIAL_JIFFIES` -- a vvar-like
page, which SlateOS does not have yet. It would cover every reader, and
remove the cpu_relax shadow and the layer's scattered updates. It is a
kernel interface (a mapping the timer path writes), so it wants a design
decision of its own: one page per driver process, or one shared page with
the offset applied by the layer's link. Raised by the batch-25 review (Opus,
2026-10-28); no driver in the layer yet waits this way, and checkpatch-clean
Linux code puts `cpu_relax()` in such loops.
