## §211 — The QEMU boot lock queues, and a lane that cannot get a turn fails rather than booting anyway

**Date:** 2026-08-16
**Decided by:** Claude (autonomous), on lane B's request
(`requests/b-a-the-boot-lock-has-no-queue-so-a-waiting-lane-can-starve.md`)
**Lane:** A

**In short:** three copies of this project run on one PC, and only one of them
can run the emulator at a time — two at once slow each other down so much that
a healthy test looks broken. They coordinate with a "lock": whoever grabs it
first goes, everyone else waits. The problem was that grabbing it was a pure
race with no memory of who had been waiting, so a lane that finished and
immediately started again beat the waiting lane *every single time* — one lane
watched five consecutive runs of another go ahead of it across forty minutes.
And when a wait finally timed out, the script's response was to start the
emulator anyway, which is the exact thing the lock exists to prevent. Two
decisions here: **wait in a line (first come, first served)**, and **when you
still cannot get a turn, stop and say so rather than barging in**.

### Decision 1 — a ticket queue in front of the `mkdir`, not a fairer race

`mkdir` is atomic, which makes it a correct mutex and a useless scheduler: it
records who *has* the lock and nothing about who has been waiting for it. Worse
than "unfair" — it is *systematically* unfair here, because every waiter polls
on the same 5-second period, so their probes are phase-locked and the one that
entered the loop later probes later in every subsequent cycle, forever. There is
no randomness for repeated attempts to average out. That is why the observed
run was five straight losses rather than the coin-flip mix chance would predict.

The alternative considered and rejected was **anti-barge**: after releasing, a
lane declines to re-acquire for one poll interval if anyone is waiting. It is
about a fifth of the code and fixes the exact observed case. It was rejected
because it is a heuristic that *reduces the probability* of losing rather than
bounding the wait — it converts "never gets a turn" into "usually gets a turn",
and which of those you got is not visible from a log. A queue bounds it: with N
lanes you wait at most N-1 turns, and the bound is a property of the mechanism
rather than of the timing.

Cost of the queue: tickets are state that can be orphaned. A run killed by
`run-timeout.py`'s Job Object executes no exit path, so it leaves a ticket
behind — and a dead ticket at the *head* blocks every lane, which is strictly
worse than the starvation being fixed. This is paid for by sweeping tickets with
the lock's own liveness rules (proven-alive never swept, provably-dead after
60s, unjudgeable after 1200s) rather than inventing a second set of thresholds:
two rules about "is that process still there?" that can disagree is how the
next bug in this file gets written.

### Decision 2 — expiry refuses (exit 4) instead of booting anyway

This is the genuine tradeoff, and it reverses a prior default. `BOOT_LOCK_WAIT`
expiry used to proceed without the lock, on the reasoning that a bounded wait
that *proceeds* is friendlier than one that fails. That reasoning holds only if
the wait normally ends by acquiring. Under starvation it ends by doing the
forbidden thing — two emulators under TCG — an hour later, unattended, with the
resulting slowdown then attributed to the code under test. A false slow/failed
boot is a worse outcome than a refusal, because a refusal is *true* and a reader
(or a retry loop) can act on it, whereas a contention-induced phantom failure
sends someone to debug a kernel that is fine.

Against: a refusal is a non-zero exit, so any wrapper that treats non-zero as "a
kernel problem" now has a new way to be wrong. That is why it is exit **4**,
documented in the script header beside 1/2/3, rather than folded into 1 — and
why the five loop callers were taught about it explicitly rather than left to
infer it.

"Something live is ahead of us" is deliberately **two** conditions, not one: a
live lock owner, *or* a live waiter at the head of the queue. Checking only the
owner was the first implementation and the test harness caught it: when the lock
is free but a live lane holds the head ticket there is no owner to check, so an
owner-only rule boots anyway — beside the process most likely to enter the
emulator seconds later. The escalation would have survived the fix, relocated.
Where *nothing* live can be demonstrated (an ownerless lock, a queue of
processes we cannot see), expiry still proceeds, preserving the original
conservative default for genuinely unknowable states.

### Decision 3 — the refuse path deletes the serial log

Follows from decision 2 rather than standing alone, but it is the part most
likely to be undone by someone who does not see why. `boot-test.sh` truncates
the serial log *after* the lock, so on the refuse path it still holds the
previous run's output — and every soak wrapper greps that file the instant the
script returns. Deleting it makes "nothing was booted" self-evident to callers
that never heard of exit 4, including ones written later. The alternative —
relying on each caller to check the status — was rejected for the same reason
the exit-code recorder is wired into one trap rather than twelve exit sites: the
caller nobody remembered to update fails *silently and in the wrong direction*,
scoring a stale log as this run's result.

**How to reverse:** the queue is ~40 lines in the `BOOT-LOCK-REGION` and could
be deleted wholesale to return to the bare race. Decision 2 is one `if` at the
expiry branch. Both are covered by `scripts/test-boot-lock.sh`, which runs 15
cases in ~6s with no kernel build — reverse either and the tests say which
property you gave up.
