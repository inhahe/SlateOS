## 1522. A boot can carry on past failed self-tests, on request, to list every failure in one run

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous), beside the operator's §914, whose default it leaves as it is · **Lane:** A

**In short:** a boot stops at the first failed integrity self-test (§914),
so a change that breaks three tests takes three boots to find them -- about
an hour each, most of it the gates (known-issues
`A-A-BOOT-SPENDS-AN-HOUR-TESTING-SCRIPTS`). Lane A spent six boots in a row
on 2026-10-01/02 finding one stale expectation each. Now a boot started with
`SLATE_CMDLINE="selftest.keep_going=1"` reports each failure exactly as
before -- `FATAL: ... self-test failed`, so the run still fails -- and goes on,
including past the failed parts of the Linux ABI translation test's 231
chained parts. Without the parameter nothing changes.

**What changed:** `selftest::keep_going()` reads the kernel command line;
`selftest::dispatch`/`dispatch_debug` print the `FATAL` line and, only when it
is set, return instead of halting; `selftest::step(part)` wraps each chained
part of `linux::self_test` -- a pass-through unless it is set.

**Why this does not reopen §914.** §914 decided what a *machine* does when an
integrity test fails: it halts, rather than run on with a broken invariant.
That stays the behaviour of every boot that does not ask otherwise -- and the
operator rejected skipping self-tests (option A) because a tested path that
differs from the shipped one is the wrong thing to have on a first hardware
boot. This skips nothing and ships nothing: it is a developer's request to see
more of a run that has already failed. A boot with it set that passes is an
ordinary pass, since nothing changes until something fails.

| | What changes | For | Against |
|---|---|---|---|
| **Carry on, on request, still reported as failure (chosen)** | one failing boot lists every failure | an hour saved per failure after the first; the run's verdict is unchanged | a test after a failed integrity test ran on a kernel whose invariants may be broken, so a later failure is a lead until a boot without the first confirms it |
| Leave it: one failure per boot | -- | nothing to explain | six boots for six stale expectations, as on 2026-10-02 |
| Demote more tests to Diagnostic | they would log and continue always | no switch | changes §914's classification -- the operator's -- for a developer convenience |

**Revisit** if failures seen only after an earlier one keep failing to
reproduce on their own: that would mean the tests lean on each other's state,
which is a bug in the tests, and the switch would be reporting noise.
