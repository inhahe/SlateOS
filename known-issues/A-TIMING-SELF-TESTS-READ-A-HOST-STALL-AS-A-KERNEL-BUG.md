### A-TIMING-SELF-TESTS-READ-A-HOST-STALL-AS-A-KERNEL-BUG -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN (lane A) -- fixed on lane-a-wip 2026-10-07, awaiting a boot on
`main`.

**In short:** three of the kernel's start-up self-tests timed how long
something took inside the virtual machine and failed the boot when it took too
long. In the emulator the boot tests use, the virtual machine's clock is the
host's real clock, so when the host is too busy to run the emulator for a few
seconds, the virtual clock jumps those seconds at once and the test sees a
"slow" kernel that did nothing wrong. Lane C lost two boots of unchanged
kernels this way on 2026-10-05; every lane's boot was exposed whenever another
lane was compiling.

**Where:**

| test | file | what it compared |
|---|---|---|
| eventfd signaled-before-expiry | `kernel/src/ipc/eventfd.rs` `test_timeout_signaled` | a reader with a 5 s timeout against a writer that slept 5 ms first: a stall over 5 s expired both, and the reader could run first |
| multi-object wait, phases 1-7 and 9-11 | `kernel/src/ipc/multiwait.rs` `self_test` | each wait's elapsed time against a ceiling (100 ms to 1 s), and phase 4 against its own 2 s timeout |
| socket wait backoff | `kernel/src/net/socket.rs` (the `wait_until` self-test) | twelve backoff sleeps against a 2 s ceiling |

**Observed** (lane C, both on kernels byte-identical to ones that passed):

    [eventfd]   FAIL: timeout_signaled: got 18446744073709551615     (boot of f8addc6e7, at 630 s)
    [multiwait] FAIL: timerfd expiry returned n=1 after 2003606600ns (boot of 993280872, 77 s in)

The eventfd one is the July bug `known-issues-resolved/B-EVENTFD-TOTEST-SHORTTIMEOUT.md`
again: its fix raised the reader's timeout from 500 ms to 5 s, which made the
window smaller but kept it a guest-clock bound.

**Why a longer bound is not the fix.** No ceiling outlasts every stall, and the
ceiling is the only thing in these tests that detects the defect it guards
against (a lost wake, or a park whose cap was dropped, shows up only as a wait
that ran to its timeout). Raising it trades the detection away. The precedent
is `known-issues-resolved/A-a-timing-self-test-panicked-the-kernel-over-a-988ms-sleep-...`
(`sched::sleep_ns`, 2026-09-18).

**The fix, by test:**

- **eventfd:** nothing is judged against guest time alone. The write happens
  only once the reader is seen parked (registered and `Blocked`), so the test
  exercises the path it names. A lost wakeup is judged by state: `sched::wake`
  makes a `Blocked` task `Ready` before `write()` returns, so a reader still
  `Blocked` after the write was never woken -- deterministic, no clock. A
  `TimedOut` fails only when the write provably completed before the reader's
  deadline (timestamps, with 1 ms allowed for clock skew between CPUs);
  otherwise the attempt proves nothing and is retried, up to 5 times. Waits for
  the other task count scheduling rounds, which a stall cannot use up.
- **multiwait:** each timed phase runs up to 3 times and fails only if late on
  every attempt -- a stall does not repeat on demand, a lost wake does. A result
  load cannot produce fails on the attempt that shows it: a wait back early, or
  empty before its own timeout, or ready on the wrong object, or ready without
  its helper having acted. Every OK line now says which attempt passed.
  Each phase also joins its helper task before reading the helper's
  `MW_WROTE` count, which closes a second, unrelated flake: the helper counted
  *after* its write, so a wait the write released could read the count as 0
  and report "writer ran: 0".
- **socket backoff:** an overshoot of the 2 s ceiling is measured again, up to
  3 times; finishing in under 50 ms still fails at once.

**How this class was swept:** every `>`/`>=` against a time-valued name
(`elapsed`, `took`, `waited`, `slept`, `*_ns`, `*_ms`), a nanosecond-scale
literal, or a `*_NS`/`*_MS`/`*_TICKS` constant, within eight lines of a
`FAIL` print or `fail(` call, across `kernel/src`. These three were the
remainder after `sched::sleep_ns`.

**What would still make one of them fail under load:** the same stall on every
attempt -- 5 in a row for eventfd, 3 for the others. That is not a host
stall's shape, and if it happens the attempt lines printed before the FAIL say
how late each one was.
