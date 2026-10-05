### `BUG-SLEEPSLOT-HELD-UNTIL-DEADLINE` - fixed

`kernel/src/sched/mod.rs`, `sleep_until_tick_interruptible`.

`SLEEP_QUEUE` is 256 fixed slots. A slot was claimed by CAS-ing `wake_tick`
from 0, and released **only** by `process_sleep_wakeups` once `now >= deadline`.
`_interruptible` exists precisely so that another wake can return early - and on
that path the slot stayed armed for the whole remaining timeout. A
`WaitQueue::wait_until_timeout(30s)` satisfied in a millisecond held a slot for
thirty seconds. `sleep_until_tick` loops around the interruptible variant and
claimed a *fresh* slot per iteration, so one long sleep with many early wakes
could hold many slots at once.

Two follow-on harms, both seen:

1. Exhaustion is **self-amplifying**. The fallback is `while tick < deadline
   { yield_now(); }`, and every spinner hammers `SCHED`; the timer ISR retires
   expired slots under `SCHED.try_lock()`, which then fails, so slots are *not*
   retired, so the queue stays full. The old code also printed a serial line per
   failed claim - 1100+ of them - adding serial I/O to that loop. That is the
   livelock in the table above.
2. When the ISR finally reaches an orphaned slot it takes the
   `Some(task)`-but-not-`Blocked` branch and sets `pending_wake = true` on a
   task that is awake and doing something else entirely. The next
   `block_current()` that task makes returns *without blocking*. That is the
   most plausible reading of `timeout_expires returned Ok(true)`: a futex wait
   with a 5 ms timeout reported "I was woken" because a token left by an
   unrelated sleep was sitting there waiting to be spent.

**Fix.** `SleepEntry` gains a `claim` field holding a globally unique token,
and `claim` - not `wake_tick` - is now the free/busy marker. A sleeper releases
its own slot by CAS-ing *its own token* out, so a stale releaser whose slot was
already retired and re-claimed simply fails and does nothing. The ISR takes the
slot into a `RELEASING` state *before* reading `task_id`, which closes the
window where a slot could be recycled underneath the scan and the wrong task
woken. `sleep_until_tick_interruptible` now releases on both paths, and the
exhaustion warning is one-shot.
