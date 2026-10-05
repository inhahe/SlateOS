## 961. A task earns the interactive boost by sleeping, not only by blocking briefly — sleep credit alongside the burst average

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A

**In short:** the scheduler moves "interactive" tasks — ones that wake up, do
a little work and go back to waiting, like anything responding to a user —
two places up the queue, so they respond quickly. It recognised them by one
sign only: their work comes in short bursts. A program reading a large file
in small pieces shows the same sign while using almost all of the processor,
because each wait is over almost as soon as it starts. It was being moved up
the queue and starving the program waiting for it. Now a task must also have
actually spent time asleep, recently, to be moved up.

**What was decided.** Each task keeps a `sleep_credit`: raised at every wake
from a real park by the ticks it spent parked, lowered by one for each tick it
is charged while running, held within `0..=MAX_SLEEP_CREDIT` (100 ticks). The
boost now requires both the old test — the burst average under
`INTERACTIVE_THRESHOLD_TICKS` — and a credit of at least `MIN_SLEEP_CREDIT`
(one threshold). It is re-evaluated at each block, at each wake, and at each
tick, so a task that stops sleeping loses the boost the tick its credit runs
out rather than at its next block. This is the quantity Linux's O(1)
scheduler kept as `sleep_avg`.

Tick sampling makes it an unbiased measure even for work and sleep shorter
than a tick: a task on the CPU 90% of the time is charged about 90% of the
ticks, and sleeps too short to straddle a tick boundary bank nothing, so it
drains; a task that sleeps most of the time banks far more than it spends.

**Found by** `ctest-python-repl` on 2026-09-25: the interpreter child, inside
a large read, kept the boost while its parent at the same base priority was
rescued by the anti-starvation check a dozen times.

**Alternatives considered.**

| option | why not |
|---|---|
| Measure burst and sleep in nanoseconds (hrtimer) | exact rather than sampled, but needs a timestamp at every switch-out as well as every block, on the hottest path in the kernel, to accumulate a burst across preemptions; the sampled credit reaches the same verdict statistically for the cost of one decrement per tick |
| Replace the burst rule with the credit alone (as O(1) did) | the burst rule is what drops a task the moment one burst runs long, and the self-test pins that; keeping both costs one comparison |
| Fair-share scheduling by virtual runtime (CFS) | the right long-term shape for fairness, and a rewrite of the run queues; this fixes the observed starvation within the current design |

**Where this bites:** `kernel/src/sched/task.rs` (`sleep_credit`,
`MAX_SLEEP_CREDIT`, `MIN_SLEEP_CREDIT`, `interactive_verdict`, `record_block`,
`tick_burst`, `mark_ready`), `kernel/src/sched/mod.rs` (`spawn_inner` stamps
`block_tick` on a suspended spawn so admission is not credited with every tick
since boot; `test_interactive_detection`).
