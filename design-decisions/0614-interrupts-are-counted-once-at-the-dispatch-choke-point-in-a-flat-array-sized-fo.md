## 614. Interrupts are counted once at the dispatch choke point, in a flat array sized for the whole 256-vector space

**Date:** 2026-08-26
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** the kernel keeps a running tally of how many times each interrupt
has fired. Until now it only tallied CPU *faults* (page faults and the like) and
silently ignored real hardware interrupts -- so the "timer interrupts" counter
read zero on a machine whose timer had fired millions of times. The fix has to
decide two things: where to do the counting, and how big the table should be.
Both were decided in favour of the version that cannot quietly go wrong again,
at a cost of one atomic add per interrupt and 2 KiB of memory.

**The two decisions.**

*Where.* Every hardware vector already funnels through one function,
`idt::dispatch_vector`, which switches on the vector number and calls the right
handler. The count goes there -- once, before the switch -- rather than in each
of the five handlers it dispatches to.

The alternative (a call in `isr_timer`, `handle_device_irq`, the two IPI
handlers and `isr_spurious`) is what the tracking entry originally proposed, and
it is correct exactly as long as every future author of a new arm remembers to
add the call. That is precisely the property that had already failed: vectors
33-56 were installed without anyone noticing they went uncounted. A convention
that has already been broken once should not be re-adopted as the fix for having
broken it.

Putting it before the switch rather than inside the arms also buys the one case
no per-handler count can ever see: an interrupt arriving on a vector that has a
stub installed but no handler (`_ => {}`). That is the single most diagnostic
event in the whole table, and the per-handler design structurally cannot count
it.

*How big.* The array went from 48 entries to 256 -- the entire vector space --
rather than to 57, or to "48 plus a few". 48 was itself a reasoned number once
(32 exceptions + 16 device IRQs) and it was wrong within one feature: the IOAPIC
has 24 inputs, not 16, and the two IPIs and the spurious vector live up at 251,
252 and 255. Any bound short of 256 is another guess of the same kind, and the
guesses are not free -- `count_vector` uses `.get()`, so an out-of-range vector
is *dropped in silence*, which is the failure mode that hid this bug for its
entire life.

**What it costs.**

| cost | size | assessment |
|---|---|---|
| one relaxed `fetch_add` per hardware interrupt | a few ns | noise beside the ISR body; the exception paths already paid it |
| `.bss` for `VECTOR_COUNTS` + `RATE_SNAPSHOT_COUNTS` | 2 KiB each | irrelevant against a 16 KiB page |
| `vector_counts()` / `InterruptRates` returned by value | ~2 KiB of stack | 3% of the 64 KiB `TASK_STACK_SIZE`, and only on hand-run diagnostic paths, never in an ISR |

**The real tradeoff is the cache line, not the cycles.** `VECTOR_COUNTS` is a
flat global, so slot 32 is one cache line that every CPU's timer ISR now
contends for at the tick rate. On a 4-core machine at 1 kHz that is 4000
contended atomic adds a second -- measurable in a microbenchmark, invisible in a
profile. It was accepted because the alternative is a per-CPU array, and that
is a different change with a different justification: it makes the counter
faster *and* makes per-CPU attribution possible (which `fs::irqstat::per_cpu()`
wants and still has no source for), but it touches an interrupt hot path and
deserves its own benchmark rather than being smuggled in beside a correctness
fix. Recorded as deferred, not as rejected.

**A correctness fix that breaks a display, in the same commit.** Counting
hardware vectors makes `kstat`'s "interrupts" and `kcounters`' `total_interrupts`
true for the first time -- and makes `kshell`'s `Exceptions: total=` false, since
it summed the whole array. It was never *right*; it was unfalsifiable, correct
only for as long as the array it summed happened to contain nothing but
exceptions. Left alone, its `> 100` HIGH health indicator would have latched
within the first second of uptime and never cleared. Narrowing it to vectors
0-31 is part of this change and not a follow-up: a commit that is defensible on
its own terms while leaving a permanently-red health indicator behind it is
worse than no commit.

**Where it bites.** `kernel/src/idt.rs` (`VECTOR_STATS_SIZE`, `dispatch_vector`,
the new `vector_name`); `kernel/src/kshell.rs` (`cmd_health`, `cmd_exceptions`,
`cmd_exclog`, `cmd_irqrate`); `kernel/src/kcounters.rs`; `kernel/src/kstat.rs`.
Tracked as `A-IDT-COUNTS-ONLY-EXCEPTIONS-AND-CALLS-THE-TOTAL-INTERRUPTS`.
