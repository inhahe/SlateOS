## Three timer leaks with one shape, found from a boot that hung after 1208s (lane A)

**In short:** a task that asks to sleep for five seconds, and is then woken
after one millisecond by something else, used to leave *two* pieces of
bookkeeping behind - a sleep-queue slot and a kernel timer - both still armed
for the remaining 4.999 seconds, for a sleep nobody is waiting on any more.
Enough of those at once filled two fixed-size tables. One of the tables
responded by **throwing away somebody else's armed timer**, which is a lost
wakeup manufactured on demand. All three are fixed.

The failure that exposed them: boot `1208s FAIL` on `lane-a`, 2026-08-21. It is
not reproducible on demand - it needs the machine's real network to be slow, so
it will not appear in most boots. A comparison boot on a byte-identical kernel
passed in 691s with none of the symptoms, which is what makes the network the
differentiator rather than any code change.

### The observed sequence

| Serial line | Symptom |
|---|---|
| 2153-3709 | **1541 x** `[hrtimer] WARNING: per-CPU timer limit reached - oldest timer evicted`, all inside the ring-3 network tests |
| 27913 | `[sched] WARNING: sleep queue full, task 0 falling back to spin` |
| 28077 | `[futex]   FAIL: timeout_expires returned Ok(true)` -> `[FATAL]` |
| 28084-28969 | `[irq-storm] IRQ 10 MASKED: ~500000 IRQs/sec` x 9, escalating cooldowns, never recovering |
| end | `!! could not acquire SCHED lock - a task is likely wedged holding it`, then panic |

The comparison boot had `[hrtimer] Stats: scheduled=359`; the failing one had
`scheduled=5308`. Same tests, same image, same 26 skipped Path-Z rungs - the
only difference is that the failing boot's TCP fetches to the real internet were
slow, so far more code sat in timed waits at once.
