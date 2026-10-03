### `BUG-HRTIMER-ORPHANED-BY-EARLY-WAKE` - fixed

`kernel/src/sched/mod.rs`, `sleep_ns_interruptible`.

The same shape, one table over: `let _handle = hrtimer::schedule_ns(...);
block_current();` - and no `cancel`. Woken early, the timer stays armed. And
`sleep_ns` loops around this function, so a sleep broken by repeated early wakes
armed one timer per iteration and cancelled none.

The arithmetic in the failing boot says this is where the pressure came from:
`scheduled=5308, fired=3730, cancelled=32`. Thirty-two cancellations against
five thousand arms, in a kernel whose every hrtimer call site is a
wait-with-timeout that ought to cancel on the success path.

**Fix.** `hrtimer::cancel(handle)` after `block_current()`. A no-op if the timer
already fired.
