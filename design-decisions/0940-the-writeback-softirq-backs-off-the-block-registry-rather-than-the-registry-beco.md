## 940. The writeback softirq backs off the block registry rather than the registry becoming IRQ-safe

**Date:** 2026-09-15 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** the kernel keeps a list of attached disks behind a lock. A
background timer periodically writes out data that has been sitting in memory
too long, and to do that it needs that same list. If the timer fires while the
system is in the middle of reading a disk -- which already holds the lock -- the
timer waits for a lock that cannot be released until the timer returns, and the
machine stops dead. It did: a FAT boot panicked with a self-deadlock. The fix is
that the timer now gives up instead of waiting, and tries again on its next
tick.

**Why not make the lock interrupt-safe instead**, which is the textbook answer.
Disabling interrupts for the life of the lock would work, and the lock is held
**across a real device read** -- `with_device` hands the closure a `&mut dyn
BlockDevice` and the closure spins waiting on virtio. That is milliseconds with
interrupts off, on every disk read, which costs timer ticks and scheduling
latency to fix a contention window measured in microseconds. The cure is
quantitatively worse than the disease and it is worse on *every* read, not just
the rare interrupted one.

| option | what it costs | why not |
|---|---|---|
| registry becomes IRQ-safe (interrupts off while held) | interrupts disabled across every device read | trades a rare deadlock for a permanent latency and timekeeping regression |
| writeback moves to a kernel thread | a thread, a wakeup policy, and a new failure mode when it falls behind | correct long-term and too large to land under a bug fix; recorded in `todo.txt` |
| softirq backs off on contention (chosen) | a flush may be deferred a tick | free: the entry stays dirty and the next tick retries it |

**Backing off is free here, and that is what makes it the right answer rather
than the cheap one.** `try_flush_expired` already treats a failed writeback as
"not flushed" and leaves the entry dirty, so declining loses nothing and delays
by one tick. Data is not at risk: nothing is dropped, only postponed. Had the
skipped work been unrepeatable, this option would not have been available and
the kernel thread would have been the only honest answer.

**The starvation question, since a `try_lock` invites it.** A flush deferred
forever is a real hazard in principle. It is bounded here: the registry is held
only for the duration of one sector operation, the flush retries every
`CACHE_FLUSH_INTERVAL` ticks, and the entry's age keeps growing so it stays
eligible. A workload that could starve this would have to hold the registry
essentially continuously, at which point the disk is saturated and a deferred
writeback is the correct behaviour anyway.

**`try_with_device` keeps three outcomes distinct** -- `Err(WouldBlock)`,
`Ok(None)` for no such device, `Ok(Some(r))` -- rather than folding busy into
`None` the way `with_device` folds absence. In interrupt context those two mean
opposite things: one says retry next tick, the other says that disk is gone. The
shorter signature was available and would have made a transient lock conflict
indistinguishable from a missing disk in any future caller that checked only
`is_none()`.

**The defect this came from is worth keeping attached to the decision.** The
file already knew: `try_flush_expired` opens with `CACHE.try_lock()?`,
deliberately non-blocking because it runs in interrupt context. The same care
was applied to one of the two locks that path takes. The second was three frames
away, in another file, reached through `writeback_entry`, whose call site says
nothing about context. Being right about a hazard does not propagate along a
call chain by itself -- which is an argument for naming the constraint in the
callee's own documentation, where the next caller will read it, and is why
`writeback_entry` now carries "NOT safe from interrupt context" in its doc
comment rather than only in this entry.

**Amended the same day, with evidence from lane B that this is necessary and
NOT sufficient.** Their `pre-push` header list contains the sentence *"FIVE OUT
OF FIVE RECENT ADDITIONS"* had a particular fault -- and they read that line
while committing the sixth instance of it. A warning about a trap, sitting in
the file where the trap is, in the paragraph you are editing, did not prevent
the trap. What caught it was `test-pre-push-gates.py` refusing the push.

So the doc comment above should be read as documentation, not as a control. The
constraint it states -- do not reach a blocking lock from interrupt context --
is the kind a checker could enforce, by walking the call graph out from the
known softirq and IRQ entry points and refusing a blocking `lock()` reachable
from any of them. That is real work and is recorded in `todo.txt` rather than
claimed here; what is claimed is only that the comment will not do the job by
itself, on evidence rather than on principle.
