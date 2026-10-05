## TD-A-FOUR-MODULES-NAMED-AFTER-DEFERRED-WORK-PRIMITIVES-DEFER-NOTHING (lane A, 2026-09-14)

**In short:** the kernel has modules called `softirq`, `timerq`, `wqstat` and
`kthread`. In Linux those names mean "run this later" and "run this on a kernel
thread". Here all four are **counters for `/proc`** and nothing more -- they
record that work happened somewhere else. Code that calls
`wqstat::enqueue(id)` believing it has scheduled something gets a `/proc`
counter that increments and no work performed, ever.

| module | looks like | actually is |
|---|---|---|
| `kernel/src/fs/softirq.rs` | Linux softirqs | statistics (says so in its own doc) |
| `kernel/src/fs/timerq.rs` | a timer queue | statistics (says so in its own doc) |
| `kernel/src/fs/wqstat.rs` | a workqueue | accounting: `enqueue`/`activate`/`complete` bump fields |
| `kernel/src/fs/kthread.rs` | kthread spawning | a registry: `register`/`set_state`/`record_cpu_time` |

Measured, not inferred: across all four there are **zero** occurrences of
`sched::spawn`, a `loop {`, `schedule()` or `yield_now`. They cannot run
anything, and their location under `fs/` is the tell -- they are there because
they are `/proc` file providers, not because they are scheduling primitives.

**Why this is a trap and not just a naming quibble.** It fails in the
**substitution** mode of 937: a caller that mistakes `wqstat` for a workqueue
does not get a compile error or a silent no-op it might notice. It gets
*positive confirmation* -- `/proc` shows the enqueue count rising, which is
exactly what a working queue would show. The observable signal a developer would
reach for to verify the mechanism is the one thing that still works.

**The real mechanism is `sched::spawn`** (`kernel/src/sched/mod.rs:1517`, plus
`spawn_with_affinity` and `spawn_suspended`). Deferred work means spawning a task
that owns a queue and sleeps when it is empty; there is no generic
enqueue-a-closure facility in this kernel yet.

**Where this bites next, concretely.** A-Q10 is answered and its second half is
"do the read-back and checksum after the write has returned to the caller"
(design-decisions, A-Q10). That needs a genuine worker. Reaching for the
plausibly-named module would produce a versioning path that enqueues to a
counter and records no versions, while `/proc` reported a healthy queue.

**The proper fix**, for whoever needs it first: build one real single-worker
queue on `sched::spawn` -- a `VecDeque` behind the existing `PreemptSpinMutex`
and an `Event`/waitqueue to park on when empty -- and give it a name that cannot
be confused with the four above. Then either rename these to `*_stat` or add a
`//!` line to each saying it counts and does not run. Renaming is the better
half: a doc comment is only read by someone already suspicious.
