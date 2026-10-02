### `BUG-DEFERRED-WAKE-DROPPED-BEFORE-PARK` - fixed (this was the hang)

`kernel/src/sched/mod.rs`, `drain_deferred_wakes_locked()`.

**What the wedge dump showed.** Single-CPU boot, every other task `Dead`:

```
tid=0   state=Blocked pending_wake=false block_site=kernel\src\container.rs:4311  name="idle"
tid=108 state=Blocked pending_wake=false waited=4664
        block_site=kernel\src\sched\mod.rs:5828                                  name="netstack-dns"
[sched]   sleep queue: 0/256 slots occupied
[hrtimer] totals: scheduled=19467 fired=10 cancelled=19457 refused=0
[hrtimer] cpu0: 0 pending
```

`container.rs:4311` is `wait_process()`, so tid 0 is waiting for the netstack
daemon. `mod.rs:5828` is the `block_current()` inside `sleep_ns_interruptible`.
Tid 108 is parked on a sleep whose **timer no longer exists** (`0 pending`, and
the totals account for every timer ever created: `19467 = 10 + 19457 + 0`), with
`pending_wake=false`. Nothing anywhere could ever run again. Note the totals are
*healthy*: a 19457:10 cancel-to-fire ratio is the leak fix working, since every
`wait_timeout_ns` satisfied early legitimately cancels its timer.

**The defect.** Three functions wake a task. All three must handle the case
where the target has not parked yet, because the ordinary
register-then-recheck interleaving puts it there constantly:

| | target `Blocked` | target `Running`/`Ready` |
|---|---|---|
| `wake()` | enqueue | `pending_wake = true` |
| `try_wake()` | enqueue | `pending_wake = true` |
| `drain_deferred_wakes_locked()` | enqueue | **wake discarded, slot cleared** |

The third one is not a rare path - it is the *primary* one. Its own doc comment
says so: on a single-CPU system the ISR-context `try_wake` always loses the race
for the scheduler lock against the code it interrupted, so every deferred wake
is delivered here. So every wake that arrived in the window between a task
arming its timer and reaching `block_current()` was silently dropped.

The sequence that hung the boot:

1. tid 108 calls `sleep_ns_interruptible`, arms its hrtimer, and is still
   `Running`.
2. The timer expires. `process_expired` removes the entry (this is the `fired`
   that leaves `0 pending`) and calls the callback, which calls
   `try_wake(108)` - contended, so it falls back to `defer_wake(108)`.
3. `schedule_inner` drains. tid 108 is still `Running`, so the drain discards
   the wake and clears the slot.
4. tid 108 reaches `block_current()`, sees `pending_wake == false`, and parks
   forever with no timer armed.

Step 3 is a window of a few instructions, which is why the hang was
intermittent, and why it needed the leak fixes (which multiplied timer traffic
~4x) to become frequent enough to catch.

**Fix.** `drain_deferred_wakes_locked` sets `pending_wake = true` in the
not-`Blocked` case, exactly like the other two.

### Two smaller defects fixed in the same file, found by reading around it

Neither is known to have caused an observed failure; both are the same class of
silent wake loss.

1. **The pending-flag was cleared after the scan, not before.** Both
   `drain_deferred_wakes_locked` and `process_deferred_wakes` scanned the 32
   slots and then stored `DEFERRED_WAKES_PENDING = false`. A `defer_wake`
   landing in a slot the loop had already walked past sets the flag *behind* the
   cursor - and the trailing store then erased it, stranding that slot until
   some unrelated later wake happened to set the flag again. Both now clear the
   flag before the scan, where a redundant rescan is the worst outcome.

2. **A full queue dropped the wake silently.** `defer_wake` fell off the end of
   its 32-slot search and returned, with a comment saying this "should never
   happen". A dropped wake is a hang, so it must not be invisible: it now
   retries `try_wake` once directly (the lock holder may have released it since,
   and `try_lock` keeps this ISR-safe), and if that also fails it increments
   `DEFERRED_WAKE_DROPS` and prints one `CRITICAL` line. The counter and the
   occupied slots are printed by `dump_timer_sources()`, so both dumps show
   them.

**Diagnostic gap this closed.** From the parked task's side, a dropped wake, a
stranded queue slot and a timer that never fired look identical - `Blocked`,
`pending_wake=false`, no timer. `dump_timer_sources()` now prints the sleep
queue, the deferred-wake queue (occupied slots, the pending flag, the drop
count) and the hrtimer lists together, from both the liveness watchdog and the
idle-fallback wedge dump, so the log tells them apart.
