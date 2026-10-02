### `BUG-HRTIMER-CANCEL-SCANS-EVERY-CPU` - fixed, but **not** the hang

`kernel/src/hrtimer.rs`, `cancel()`.

**Read the last paragraph of this subsection before believing the middle of
it.** The IRQ-off-cost story below is a plausible mechanism that turned out not
to be the one operating here; it is kept because the defect it describes is
real and the fix is worth having.

A `HrTimerHandle` was a bare id, so `cancel` had to search for the entry. It
tried the current CPU, and on a miss dropped that lock and locked+scanned every
other live CPU's list - the whole thing inside `without_interrupts`. A miss is
the *common* case, because `cancel` runs on the success path of every
wait-with-timeout, where the timer has usually already fired.

Before the leak fix that cost nothing, because `cancel` was almost never called
(`cancelled=32` against `scheduled=5308` in the failing boot). Adding the
correct `cancel` moved it onto the hottest wait path in the kernel: `kmutex`,
semaphore, condvar, `kchannel` and `once_event` all route through
`wait_timeout_ns` -> `sleep_ns_interruptible`. `process_expired()` runs **only**
from the APIC timer ISR, so frequent long IRQ-off windows coalesce or lose
ticks, and timers then do not fire at all. Hence: the cancel path could stop the
very timers it was cancelling.

**The cross-CPU walk was not merely expensive, it was unnecessary.** A timer
entry never migrates. `schedule_absolute` inserts into
`CPU_TIMERS[current_cpu_index()]`, and only that CPU's `process_expired()`
removes it - including the re-insert of a repeating timer, which keeps both the
same id and the same list. The comment that justified the walk ("timer might
have been scheduled from a different CPU if the task migrated") conflated the
*task* migrating with the *entry* migrating; the entry does not move.

**Fix.** `HrTimerHandle` becomes `{ id, cpu }`, carrying the list the entry
landed on. `cancel` takes exactly one lock and does exactly one scan. A handle
returned by a request that was refused at the hard ceiling carries
`cpu == usize::MAX`, so cancelling it is a no-op instead of a search that could
never succeed.

**This did not fix the hang.** The next boot with it applied stalled at the
*identical* serial line (2178, `[sched] Task 108 exiting`). Keep the change - it
removes a cross-CPU lock walk from the kernel's hottest wait path and pins a
real invariant - but it is an optimisation, not the bug. See the next section
for what was actually wrong.
