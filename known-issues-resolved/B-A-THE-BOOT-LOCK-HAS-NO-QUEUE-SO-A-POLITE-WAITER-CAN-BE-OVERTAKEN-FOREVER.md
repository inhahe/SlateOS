## B-A-THE-BOOT-LOCK-HAS-NO-QUEUE-SO-A-POLITE-WAITER-CAN-BE-OVERTAKEN-FOREVER (lane B, 2026-08-16)

**Status: ✅ FIXED 2026-08-16 by lane A in `74f2bff75`** — ticket queue in
`scripts/boot-test.sh`'s `BOOT-LOCK-REGION`, plus a new exit status 4 for
"refused to boot beside a live lane". See "How it was fixed" at the end of this
entry; the request file
(`requests/b-a-the-boot-lock-has-no-queue-so-a-waiting-lane-can-starve.md`)
carries lane A's full reply.

### What is wrong

The cross-worktree QEMU lock is acquired with `mkdir` in a `sleep 5` retry
loop. There is no queue and no ticket, so acquisition is a race between
whoever calls `mkdir` first. A lane that finishes a boot and immediately
starts another beats a waiter every time: it is already at the `mkdir` while
the waiter is inside its sleep. The waiter is not deadlocked and nothing is
corrupt — it just never gets a turn, and to its own operator that is
indistinguishable from a hung boot.

### Observed, not theorised

One lane B run waiting, 2026-08-16:

```
=== Waiting for boot lock, held by lane-A/pid-1097553/1786905372 (240s) ===
=== Waiting for boot lock, held by lane-A/pid-1099717/1786905732 (300s) ===
```

Both the pid *and* the epoch in the owner string change between those two
lines — `pid-1097553`@14:36:12 became `pid-1099717`@14:42:12. That is lane A
releasing and a new lane A run re-taking the lock, with lane B's waiter never
once winning the `mkdir`. The wait counter keeps climbing straight across the
handover, so in a log the only tell is the owner string.

It kept going. One lane B waiter watched **five consecutive lane A runs** hold
the lock — 14:36:12, 14:42:12, 14:48:09, 14:54:00, 15:00:28 — metronomically
~6 minutes apart, one healthy boot each, across about forty minutes without
winning the `mkdir` once.

The regularity is the finding. Five straight losses on a fair coin is 1-in-32,
so chance is a poor explanation; the mechanism is a better one. Both waiters
poll on the same 5-second period, so their probes are phase-locked and
whichever entered the loop earlier probes earlier in *every* subsequent cycle.
Nothing averages out. At ~6 min per boot the 3600s `BOOT_LOCK_WAIT` is ten
consecutive losses, which at the observed rate is unremarkable rather than a
worst case.

**Practical impact: lane B cannot merge while this is open** — merging up
requires a green boot test, and the boot test cannot be obtained. That is why
the request file was cherry-picked to `main` on its own instead of riding up
with the lane B merge it is blocking.

### Why the existing backstops miss it

Both are liveness rules, and the owner here is genuinely alive. The pid check
correctly says "alive"; the age rule is deliberately guarded by
`_lock_alive != "yes"` and so correctly declines to break a live lock. That
leaves only `BOOT_LOCK_WAIT` (default 3600s), **whose expiry action is to boot
anyway** — starting a second concurrent QEMU under TCG, which is precisely what
the lock exists to prevent. So starvation does not merely delay a run: left for
an hour it escalates into the two-QEMU slowdown the lock's own header paragraph
describes, and that slowdown then gets attributed to the code under test.

### Proper fix

A ticket lock keeping `mkdir` as the primitive: a waiter registers
`$BOOT_LOCK_DIR.waiters/<epoch>-<pid>` and only attempts `mkdir` when its
ticket is the oldest, removing it on every exit path. Tickets need the same
liveness/age sweep the lock already has, since a waiter torn down by
`run-timeout.py`'s Job Object leaves a dead ticket that would block the queue
head. Smaller alternatives (an anti-barge delay after release; failing rather
than booting anyway when `BOOT_LOCK_WAIT` expires against a provably live
owner) are in the request. The last of those is worth doing regardless: it does
not fix starvation, but it stops starvation from silently becoming an invalid
result.

### Workaround in use (no longer needed)

Lane B raised its own `run-timeout.py` budget from 1200s to 3600s. The smaller
budget was killing the run *during the lock wait*, which made the starvation
present as a self-inflicted timeout — worth knowing, but it is a symptom
workaround and not the fix. With exit 4 a refusal now returns promptly and
legibly, so the larger budget is no longer required for this; lane B may want to
lower it again, since a 3600s ceiling also delays detection of a genuine hang.

### How it was fixed (lane A, `74f2bff75`)

The ticket queue as proposed. Every run drops `<epoch>-<pid>` in
`$BOOT_LOCK_DIR.waiters/` before the acquire loop and attempts `mkdir` only when
its own ticket is oldest; a lane that releases and immediately re-runs takes a
fresh ticket at the back, which is what converts the observed starvation into a
handover. Tickets are swept with the lock's *existing* liveness rules rather
than a second set (proven-alive never swept, provably-dead after 60s, anything
unjudgeable after 1200s), because a dead ticket at the head of the queue would
block every lane — a worse failure than the starvation being fixed. The queue
directory is a *sibling* of the lock, not a child, since the lock dir is created
and destroyed by acquisition and the queue must outlive that.

The escalation was fixed too, and it needed to be wider than the request
proposed. `BOOT_LOCK_WAIT` expiry now refuses with **exit 4** whenever something
live is demonstrably ahead of us. Checking only the *lock owner* is not enough
once a queue exists: in the case where the lock is free but a live lane holds
the head ticket, there is no owner to check, so an owner-only rule would boot
anyway — beside the one process most likely to enter QEMU seconds later. The
escalation would have survived the fix, merely relocated. Expiry still boots
anyway when nothing live can be demonstrated at all, preserving the original
conservative default for genuinely unknowable states.

Exit 4 also **deletes the serial log and register dump**. `boot-test.sh`
truncates the serial log only *after* the lock, so on the refuse path it still
held the previous run's output, and every soak wrapper greps it the moment the
script returns — which would re-report an old catch as new, or manufacture one
outright. Deleting the artefacts makes "nothing was booted" self-evident to any
caller, including ones written later that never heard of the status. The five
loop callers were taught about it regardless: `wedge-soak.sh` no longer spends a
sample on a refusal and now reports iterations actually *booted* rather than
`MAX_ITERS`; `wdog-reset-experiment.sh` mattered most, since it derives its
entire verdict from wall time and would have read a lock wait as "the watchdog
counter fired".

The anti-barge alternative was deliberately **not** taken: with a real FIFO
queue it is redundant (a re-running lane already goes to the back), and keeping
a heuristic alongside a queue is two rules that can disagree.

Both defects that made the final version correct were found by
`scripts/test-boot-lock.sh`, not by reading: the exit status was being swallowed
by a command-substitution subshell in the harness itself, and the free-lock /
live-head-waiter hole above. That harness now runs 15 cases in ~6s with no
kernel build — cases 10-15 cover FIFO in both directions, the ticket sweep and
its 60s floor, unparseable tickets, and that acquiring drops our own ticket.
