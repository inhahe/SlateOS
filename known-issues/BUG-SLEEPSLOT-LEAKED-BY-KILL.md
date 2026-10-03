### `BUG-SLEEPSLOT-LEAKED-BY-KILL` - fixed

**In short:** a task that is asleep holds one of 256 "wake me at time T"
tickets. It gives the ticket back on the line right after it wakes up. If
something *kills* it while it is asleep, it never reaches that line, so the
ticket sits there until time T arrives - which for the long timeouts this
kernel uses is nearly an hour. Enough of those and no one can sleep any more:
the next sleeper falls back to spinning, which pins a whole CPU.

`kernel/src/sched/mod.rs`, `kill_task` / `sleep_until_tick_interruptible`.

The sibling of `BUG-SLEEPSLOT-HELD-UNTIL-DEADLINE` above, and it survived that
fix. That fix gave the *sleeper* a way to release its own slot early
(`release_sleep_slot(slot)` on the line after `block_current()` returns). It
does nothing for a sleeper that never returns from `block_current()` at all,
which is exactly what `kill_task` produces: a `Blocked` task is "simply marked
Dead", and dead tasks do not run cleanup code.

`process_sleep_wakeups` does eventually retire the slot - it frees an expired
slot "regardless of the task's state" - but only at the deadline. That is the
whole harm: the deadline is the thing the killed task was waiting *out*.

**How it was found.** Not from a failure. The wedge dump added while chasing
`BUG-BLOCKED-TASK-RESUMED-IN-PLACE` prints the sleep queue, and it showed two
slots held by tasks 206 and 207 - both long dead - with `wake_tick` about
374000 against a `now` of about 34000. At 100 Hz that is a wake scheduled ~57
minutes into a boot that lasts six. Those two slots were gone for the whole
run. `bench.rs` kills its helper tasks (`kill_task(helper_id)` in half a dozen
benchmarks), which is where they came from; every benchmark that leaves a
helper asleep costs a slot for the rest of the boot.

Two slots out of 256 is not a failure, which is why nothing had noticed. But it
is monotonic - slots only ever leak, never come back - so the failure mode is a
long-running system, and it lands as the *self-amplifying* exhaustion livelock
documented in the entry above rather than as anything that names sleep slots.

**Fix.** `release_sleep_slots_for_task(task_id)`, called from `kill_task` once
the `SCHED` lock is dropped. It scans the queue and frees the slots the dying
task owns, using the same take-into-`RELEASING`-before-reading-`task_id`
discipline as `process_sleep_wakeups` - and restoring the token untouched when
the slot turns out to belong to someone else, so a concurrent scan cannot lose
an unrelated sleep. It prints a line when it frees anything, because a kill
that has to clean up after a sleeper is worth seeing in a log.

`kill_task` is the single choke point: a grep of the tree finds no other writer
of `TaskState::Dead` outside `sched/mod.rs`, and `task_exit`'s self-termination
path cannot be holding a slot (a task in `block_current` is not the task calling
`exit`).

Covered by a new case in `sched::self_test`: spawn a task that sleeps for
10,000,000 ticks, confirm the occupied-slot count went up by one, kill it, and
require the count to be back where it started - `[sched]   kill_task(sleeping)
releases the sleep slot: OK`.
