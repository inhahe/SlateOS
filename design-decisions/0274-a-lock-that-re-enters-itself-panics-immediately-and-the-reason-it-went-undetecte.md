## §274 — A lock that re-enters itself panics immediately, and the reason it went undetected is that opting out of ordering checks silently opted out of recursion detection

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** A kernel lock is a token only one task may hold at a time; a task
that asks for one it is already holding will wait for itself forever, and the
machine freezes with no output at all. That just happened four times over —
`STATE.lock().n = STATE.lock().n + 1`, which reads like an ordinary "add one to
a counter" but takes the lock twice. The kernel had three separate mechanisms
that were each supposed to catch this, and all three missed it. This entry
records the two calls made in response: the lock now **panics** the instant it
detects a task re-entering a lock it already holds, rather than warning or
waiting; and the reason all three nets had holes gets written down, because the
cause is a pattern that will recur.

**Why all three detectors missed it.** They were:

1. **lockdep's recursion check.** It exists and is correct. But it lives behind
   `lockdep::lock_acquire`, and `PreemptSpinMutex` — the lock type all four
   deadlock sites used — never calls it. §70 deliberately made that lock skip
   lockdep, on the reasoning that lock-*ordering* checks add no value for a leaf
   lock that is never held across another acquire. That reasoning was sound for
   ordering. It was never examined for recursion, because recursion detection
   happened to be implemented in the same function. **Opting out of one check
   silently opted out of a different, unrelated one.**
2. **The 30-second spin-stall report.** It never fired, for reasons still open
   (see `known-issues.md`); the leading suspect is a fallback iteration count
   calibrated against a TSC frequency that reads as zero under emulation.
3. **A test that covered the path.** `test_incompressible_skip` ran the exact
   code containing two of the four deadlocks, and accepted either outcome —
   "The important thing is it doesn't panic". A test that cannot fail does not
   execute the code it names in any sense that matters.

**Decision: panic, not warn.** The check (`fail_if_recursive` in
`kernel/src/sync.rs`) fires on the contended path when the recorded owner is the
current task, and it panics.

*Alternative considered — log loudly and keep spinning.* Its appeal is that a
false positive would be survivable: a bad detector would print noise rather than
kill a boot. That appeal is illusory here. The acquire it fires on **can never
succeed** — the only task that could release the lock is the one now blocked
waiting for it. Continuing to spin does not buy a chance of recovery; it buys a
silent hang, which is exactly the failure mode being fixed. A panic converts an
unbounded freeze with no output into a stack trace naming the lock, the task and
the address, and that is the entire value of the change.

*What makes the false-positive risk acceptable* is that the predicate is not
heuristic. `owner` is set only by a successful acquire and cleared by the guard
before the physical unlock; a mismatch means the same task id genuinely holds
it. Three facts were verified before wiring it in: all four `Mutex`
constructors initialise `owner` to `OWNER_NONE`; both guard types clear `owner`
*before* releasing the lock word, so no window exists where a released lock
still names an owner; and `register_ap_idle` gives each CPU's idle task a
distinct id, so two idle tasks on two CPUs cannot alias.

**Decision: a static checker for the cross-function form, not more boot cycles.**
The four sites fixed were the single-statement form, which a grep can find. The
form that a grep cannot find is a guard held across a call that re-takes the
same lock — no single line mentions the lock twice.
`scripts/check-recursive-locks.py` resolves calls within one file and reports
guards live across a call whose transitive callees re-acquire. It found 2 in
799 files, both genuine, one of them reachable by typing `boot` at the kernel
shell.

*Why within-file and not whole-program.* A cross-module analysis needs import
and trait-dispatch resolution, and would arrive with a false-positive rate that
gets it ignored. The module-private `static STATE: Mutex<..>` pattern is where
the risk actually lives in this tree, and it is entirely visible inside one
file. The cost is false negatives by construction, which is the right side to
err on for a check whose findings are read by a human before anything changes.
It never reports `try_lock`, which returns `None` rather than spinning and so
cannot deadlock.

**The generalisable lesson.** When a subsystem is given an opt-out from a
safety check, the opt-out is scoped to whatever the check's *implementation*
happens to bundle together, not to what the opt-out's rationale actually
covered. §70's rationale addressed ordering; the code it disabled also did
recursion. Any future "this lock skips X" should enumerate what X does, not
just what X is called.
