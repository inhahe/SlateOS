## `A-IRQSTAT-SELF-TEST-COMPARED-TWO-SNAPSHOTS-OF-A-COUNTER-THAT-NEVER-STOPS` (lane A, 2026-08-26)

**Status: FIXED 2026-08-26** — `kernel/src/fs/irqstat.rs`. Reached `origin/main`
in `8c860ade0`, so all three lanes were exposed to it between then and the fix.

**In short:** `/proc/irqstat` counts interrupts. Its boot self-test checked that
the per-vector table adds up to the totals line — a correct thing to check — but
it read the table and the totals at two different moments, and interrupts keep
arriving in between. One timer tick landing in that gap made the two disagree by
exactly 1, the assertion failed, and **the kernel panicked and the boot test
went red on a tree with nothing wrong with it.**

**The failure, verbatim:**

```
  irqstat::self_test() — running tests...
    [4/6] live timer count (39170): OK
    [5/6] tick cross-check (39171 >= 39171): OK
  !!! KERNEL PANIC !!!
  panicked at kernel\src\fs\irqstat.rs:354:5:
  assertion `left == right` failed
    left: 39171
   right: 39170
```

39171 − 39170 = one timer interrupt. Rung 3 had done `let t = totals();`; rung 6,
three rungs and three serial lines later, did `let lines = irq_lines();` — and
each of those calls takes its own `idt::vector_counts()` snapshot.

**Why it was rare rather than constant.** The window is only the few
microseconds those three rungs take; the timer fires about every 1 ms. So it
lost the race roughly once in a few hundred boots — which is the worst possible
frequency, because it is far too rare to be reproduced on demand and far too
common to never be seen. It was hit on the first boot test after the module
landed, and the tree it failed on was green.

**The fix — remove the race, don't widen the tolerance.** `totals_from(&[u64])`
and `lines_from(&[u64])` are now pure over a snapshot the caller supplies;
`totals()` and `irq_lines()` are one-line wrappers that grab their own, which
stays right for a `/proc` reader answering a question at the moment it is asked.
The self-test takes **one** `vector_counts()` and derives both views from it, so
rung 6 is no longer comparing two readings that agree only if nothing happened —
it is comparing two derivations of the same reading, which is what the property
was always about. Rungs 3 and 4 share the snapshot too.

The alternative — relaxing `==` to `>=`, as rung 5 legitimately does — was
rejected. Rung 5 compares two genuinely *different* counters (`apic::TICK_COUNT`
is BSP-only, `dispatch_vector` counts every CPU) where a subset relation is the
real property. Rung 6's property is equality; weakening it to an inequality
would have kept the rung green while letting a line table that double-counted a
vector pass.

**The uncomfortable part, recorded because it is the lesson.** Rung 5's own
comment reasons the race out correctly and in detail — *"Read the ticks FIRST: a
tick landing between the two reads can only then make the count look larger,
never smaller, so this cannot flake."* The same author, in the same function,
one rung later, compared two live samples with `==`. Knowing that a counter
moves is not the same as noticing every place you have assumed it did not. The
structural fix is the one applied here: make it *impossible* to compare two
snapshots by giving the comparison a signature that takes only one.

Same shape as `A-NUMASTAT-CPU-SETS-ARE-A-BOOT-SNAPSHOT-NOT-A-HOTPLUG-VIEW`,
fixed hours earlier the same day: an assertion against a freshly-read value
where the correct operand was the value the code had actually used.
