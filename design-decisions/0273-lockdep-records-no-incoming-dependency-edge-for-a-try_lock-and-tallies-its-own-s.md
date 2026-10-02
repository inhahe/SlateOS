## 273. lockdep records no incoming dependency edge for a `try_lock`, and tallies its own self-test's violations separately

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel has a lock-order checker that warns when two locks are
taken in opposite orders by different code paths, because that can wedge the
machine — each side sits waiting for the lock the other holds. It was crying
wolf twice over. Once because it counted a "just try, and give up if it's
busy" acquisition as one that could get stuck (it can't — giving up is the
whole point). Once because its own boot-time self-test deliberately provokes
three warnings to prove the detector works, and those three were being added to
the tally of real ones. Both are now fixed, so "zero warnings" finally means
"healthy" instead of "impossible".

Both were found the same way, and that is the part worth remembering: by giving
`syshealth` an exit status. Its check 5 — "no lockdep violations" — had been
printing `[FAIL] Lockdep: 3 violation(s) detected` on *every boot since the
validator landed*, and nobody noticed, because the command reported success
whatever it printed. A checker whose verdict is not in its status is a checker
nobody reads.

### The `try_lock` half

A deadlock needs both sides stuck. `Mutex::try_lock` is never stuck: if the
lock is taken it returns `None` and the caller unwinds. So the edge
*held → try_locked*, recorded because we happened to be holding something when
a `try_lock` succeeded, describes a cycle that cannot close.

Concretely: `sysctl::get` holds `sysctl-reg` across
`fs::cache::try_flush_expired`'s `try_lock` of `CACHE`, while some other path
takes `CACHE` then `sysctl-reg`. If that other path blocks on `sysctl-reg`, our
path's `try_lock` simply fails, returns `None`, and releases `sysctl-reg`. The
other path proceeds. There is no deadlock, and there never was.

*What changes:* the warning stops appearing; `syshealth` check 5 can pass.

**Alternatives considered:**

- **Leave it and lower `syshealth`'s threshold** (e.g. "fail above 3"). Rejected
  outright: it hard-codes a magic number that changes the moment the self-test
  gains a case, and it papers over a false positive rather than removing it.
- **Suppress the *outgoing* edges too** — treat a try-acquired lock as not held
  at all. Wrong, and the more dangerous mistake because it is silent: a
  `try_lock` that *succeeded* is genuinely held, so a blocking acquire nested
  inside its critical section can deadlock in the completely ordinary way.
  Linux keeps the lock in `curr->held_locks` for exactly this reason. The
  self-test asserts this half explicitly.
- **Report the inversion but do not count it.** Keeps the noise, loses the
  signal — a warning nobody may act on trains readers to skip warnings.

The precedent is direct: Linux's lockdep skips the whole dependency-add and
deadlock-check block for a trylock (`validate_chain()`:
`if (!hlock->trylock && hlock->check && …)`) while still pushing the lock onto
the held stack. We now do the same, via a `lockdep::Acquire::{Blocking, Try}`
argument passed at the two `sync::Mutex` call sites.

### The self-test half

`lockdep::self_test` inverts A→B, closes an A→B→C→A cycle, and re-acquires a
held lock — three violations, on purpose, to prove the detector fires. They
landed in the same counter as real ones, so `violation_count()` was
permanently ≥ 3.

The fix is a second tally (`SELF_TEST_VIOLATIONS`) plus an `IN_SELF_TEST` flag
that routes reports to it, rather than the obvious alternative:

- **Reset the counter when the self-test finishes.** Rejected on two grounds.
  It would erase a *genuine* violation that happened to land inside the
  self-test's window — unlikely on a single-threaded early boot, but a
  validator that quietly discards evidence is the wrong thing to build. And it
  leaves the self-test's own assertions with nothing to assert against; with
  the split, the test checks both that it provoked what it meant to *and* that
  it added nothing to the real tally.

*What changes:* `violation_count()` and `diag`'s `Lockdep: violations=` read 0
on a healthy boot instead of 3; the self-test's `Stats:` line reports the two
numbers apart (`N deliberate violations, M real`).

**Consequence to watch:** these two changes both make lockdep report *less*.
That is the right direction for a validator whose reports were unactionable,
but it means a future regression could hide behind them. The mitigation is the
new self-test rung, which asserts both directions of the trylock rule, and the
`syshealth` status itself — which is now, for the first time, a signal that a
boot script could actually branch on.
