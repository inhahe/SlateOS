## §256 — A timer handle names the CPU that holds it, and that pins an invariant

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** the kernel keeps one list of pending alarms per processor core.
Cancelling an alarm used to mean searching every core's list, one after another,
with interrupts switched off for the whole search. That turned out to be slow
enough to stop the clock interrupt itself from being delivered, which hung the
machine. The fix is to write down, in the ticket you get when you set an alarm,
*which* core's list it went on — so cancelling looks in one place. The cost is
that this only works while alarms never move between lists, so that has to
become a rule rather than an accident.

### The situation

`hrtimer::cancel` took a bare id and had to find the entry:

```rust
crate::cpu::without_interrupts(|| {
    let mut state = CPU_TIMERS[current_cpu_index()].lock();
    if let Some(pos) = state.timers.iter().position(|t| t.id == handle.0) { ... }
    // Try other CPUs (timer might have been scheduled from a different CPU
    // if the task migrated).  This is rare but correct.
    for i in 0..live_cpus { ... }
});
```

The comment is wrong in a specific and instructive way: it conflates the *task*
migrating with the *timer entry* migrating. The task does migrate. The entry
does not — `schedule_absolute` inserts into `CPU_TIMERS[current_cpu_index()]`,
and only that CPU's `process_expired()` ever removes it, including the re-insert
that re-arms a repeating timer (same id, same list). So the cross-CPU walk could
never find anything the first lookup had missed, except by finding the *current*
CPU's list after the task moved — which is the same list under a different
index, not a different list.

That made it pure cost, and the cost was not small. A miss is the *common* case
(`cancel` runs on the success path of every wait-with-timeout, where the timer
has usually already fired), and a miss walked every live CPU's list to the end,
taking a lock per list, with interrupts disabled throughout. Once
`sleep_ns_interruptible` started cancelling correctly — which is the fix in
§255's neighbourhood — that landed on the kernel's hottest wait path, and
`process_expired()` runs *only* from the APIC timer ISR. Missed ticks mean
timers do not fire, which means waits never end. Three boots hung
(`known-issues.md` → `BUG-HRTIMER-CANCEL-SCANS-EVERY-CPU`).

### The decision

`HrTimerHandle` changes from `(u64)` to `{ id: u64, cpu: usize }`.
`cancel` locks `CPU_TIMERS[handle.cpu]` and scans it, once.

*What changes:* a cancel takes one lock instead of up to `cpu_count()`, and the
interrupts-off window shrinks from "scan every core's list" to "scan one list".
Nothing observable changes when a cancel succeeds.

A handle for a request refused at the hard ceiling carries `cpu == usize::MAX`,
so `CPU_TIMERS.get(handle.cpu)` returns `None` and cancelling is a no-op. This
is better than the alternative of returning some plausible CPU index: a refused
timer is on no list, and a search that can never succeed is exactly the pattern
this change exists to delete.

### What it costs, and the invariant it pins

The handle doubles in size, 8 bytes to 16. Irrelevant — handles are held one per
blocked waiter, never in bulk.

The real cost is that this **promotes "a timer entry never changes CPU list"
from an accident of the implementation to a load-bearing invariant.** Today
nothing violates it, and the doc comment on `HrTimerHandle` says so explicitly
with the reasoning. But a future change that migrated timers with their task —
which is a reasonable thing to want, so that a task's timeouts fire on the core
it is running on — would silently break cancellation: `cancel` would look at the
old list, find nothing, and return `false` while leaving the entry armed on the
new one. That is precisely the leak class §255 was fixing.

The alternative that does not pin the invariant is to keep the search and make
it cheap some other way — e.g. a global id→cpu map, or a per-entry "cancelled"
flag with lazy removal. Both are more machinery than the problem deserves right
now, and the lazy-removal one is what the eventual min-heap rewrite in
`todo.txt` will bring anyway. So: take the invariant, document it at the type,
and let the rewrite revisit it.

Filed in `todo.txt` under the existing `hrtimer: replace the sorted Vec` entry:
whoever does that rewrite must decide migration and cancellation together.
