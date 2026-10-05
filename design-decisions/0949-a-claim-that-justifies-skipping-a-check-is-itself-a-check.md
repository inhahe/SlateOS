## 949. A claim that justifies skipping a check is itself a check

**Date:** 2026-09-17 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; the premise behind 489 opt-outs

**In short:** one of the two kinds of lock in this kernel skips the
deadlock detector, and the reason it is allowed to is that nothing is ever
locked *inside* it. That reason is written in a comment and is true of 489
separate places only if every author of every one of them got it right. The
one time it was wrong, the machine froze for twenty minutes and printed
nothing about a lock. This makes the reason checkable.

### The claim, and what rests on it

70 chose `PreemptSpinMutex` for **true leaf** locks -- "locks that never
nest another lock inside their critical section, so lockdep ordering checks
add no value" -- and that sentence is the entire justification for the type
not registering with lockdep. 944's rule applies directly: a claim that
justifies skipping a control is a control, and controls have to be
executable. This one was asserted per call site across 489 instances in ~370
files and checked by nothing.

### It has already been paid for once, in the same shape

`sync.rs`'s own post-mortem, worth quoting because it names the mechanism
better than I would: `fs::encrypt` shipped
`STATE.lock().x = STATE.lock().x.saturating_add(1)` on a `PreemptSpinMutex`,
the first boot that called it froze for 20 minutes and printed **nothing**
about a lock, and the reason lockdep missed it was that the type

> is documented as the no-tracking sibling for leaf locks ... Ordering checks
> genuinely add nothing for a leaf lock -- but *recursion* detection is not
> an ordering check, and opting out of one silently opted out of the other.

`fail_if_recursive` closed that hole, and only that hole: it compares a lock
against *itself*. Nesting a **different** lock inside a leaf critical section
is still invisible to every detector -- lockdep cannot see the type at all,
and the recursion check is looking at one lock.

### Report-only, on today's evidence

Ships as a counter and at most eight reports, not as a gate. The
lock-context check (948) produced five findings on its first real boot and
**all five were false**, three of them on locks whose `try_lock` path is the
documented fix for the very hazard being checked. A second new lock-
instrument gating a boot on its first run would be an unforced error.

### Two choices that are not obvious

**The report is filtered by `in_hardirq()`; the depth counter is not.**
`PreemptSpinMutex` disables preemption but not interrupts, so a task can
hold leaf A when an IRQ lands, and leaf B taken by that handler shares only
a CPU with A, not a critical section. Reporting that is a false positive. But
filtering the *enter* while leaving the *exit* would let the handler's
release decrement the interrupted task's depth and corrupt it, so both sides
of the counter stay unfiltered and only the report is suppressed.

**The outer lock's name is carried in two per-CPU words.** A report that
names only the inner lock is not actionable, which the same day's `?`
investigation settled: `sync::Mutex::new` defaults the name to `b"?"` and 563
instances answer to it. Two words rather than a formatted string because this
is the hottest acquire path in the kernel, and stored only on the 0 -> 1
transition.

### The gap, stated here and at the code

`in_hardirq()` is set in `dispatch_vector`, which handles the IRQ vectors. CPU
**exceptions** reach `isr_stub_*` -> `handle_*` without passing through it, so
a fault handler taking one of these while a task holds one still reports.
`proc/exception.rs` uses the type, so that is reachable rather than
theoretical -- and demand paging inside a critical section is ordinary kernel
behaviour, not a defect. Any report whose site is a fault path has to be read
that way. Filtering exceptions too would mean coupling this counter to every
exception stub, which a first measurement does not earn.
