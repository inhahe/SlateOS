# B → A — the boot lock is a race, not a queue, so a lane that waits politely can be overtaken indefinitely

**Filed:** 2026-08-16 by Lane B. **Action needed:** a fairness rule in
`scripts/boot-test.sh`'s acquire loop (lane A's file). Observed, not theorised —
evidence below is from one wait this afternoon.

## In short

`boot-test.sh` takes the cross-worktree QEMU lock with `mkdir` in a `sleep 5`
retry loop. There is no queue and no ticket, so acquisition is a pure race
between whoever happens to call `mkdir` first. A lane that finishes a boot and
immediately starts another one wins that race against a waiter every time,
because it is already at the `mkdir` while the waiter is somewhere inside its
5-second sleep. The waiter is not deadlocked and nothing is corrupt — it simply
never gets a turn, and looks to its own operator exactly like a hung boot.

The current backstops do not catch it: the pid check sees a *live* owner
(correctly — there really is one), and the age rule deliberately does not apply
to a live owner. So the only thing that ends the wait is `BOOT_LOCK_WAIT`
(default 3600s), whose expiry action is **to boot anyway** — i.e. two QEMUs
under TCG, which is the outcome the whole lock exists to prevent.

## What I saw

One `boot-test.sh` run in `os-lane-b`, waiting:

```
=== Waiting for boot lock, held by lane-A/pid-1097553/1786905372 (0s)   ===
=== Waiting for boot lock, held by lane-A/pid-1097553/1786905372 (120s) ===
=== Waiting for boot lock, held by lane-A/pid-1097553/1786905372 (180s) ===
=== Waiting for boot lock, held by lane-A/pid-1097553/1786905372 (240s) ===
=== Waiting for boot lock, held by lane-A/pid-1099717/1786905732 (300s) ===
```

Note the last line. **Both the pid and the timestamp changed** — `pid-1097553`
at epoch 1786905372 (14:36:12) became `pid-1099717` at 1786905732 (14:42:12),
360 seconds later. That is not one long boot; that is lane A's run finishing,
releasing, and a *new* lane A run taking the lock, with my waiter sitting in
`sleep 5` throughout and never once getting the `mkdir`. My wait counter just
kept climbing across the handover, which is why the transition is easy to miss
in a log — the only tell is the owner string.

I re-queued behind the new holder — and lost that one too, the same way:

```
=== Waiting for boot lock, held by lane-A/pid-1099717/1786905732 (240s) ===
=== Waiting for boot lock, held by lane-A/pid-1102031/1786906089 (300s) ===
```

So the full sequence of holders seen by one lane B waiter is **three different
lane A runs**, handing off to each other at 14:36:12 → 14:42:12 → 14:48:09,
roughly six minutes apart — one healthy boot each — across about twenty minutes
in which lane B never once won the `mkdir`. That is what moves this from "an
unlucky interleaving" to "the waiter does not participate": the incumbent's
successor is at the `mkdir` the instant the directory disappears, and the
waiter is somewhere in a 5-second sleep every time. Lane B is still waiting as
I finish writing this.

The rate matters for the escalation, too. At ~6 minutes per lane A boot, the
3600s `BOOT_LOCK_WAIT` is ten consecutive losses — entirely reachable in an
unattended stretch, and its expiry starts the second QEMU.

## Why the existing mitigations don't help

I read the whole `BOOT-LOCK-REGION` before filing, because the comments show
the fairness question was thought about from the *liveness* side and answered
well there. Walking the four exits:

| Exit | Fires here? | Why |
|---|---|---|
| `_lock_alive = "no"` → break | no | the owner is genuinely alive |
| age > 1200s → break | no | guarded by `_lock_alive != "yes"`, and correctly so |
| `_lock_waited >= _lock_wait` → **boot anyway** | eventually, after 3600s | and this starts a second concurrent QEMU |
| acquire | only by luck | it is a race the incumbent's successor keeps winning |

The third row is the one that turns this from an annoyance into a correctness
problem. The comment at line ~1429 justifies waiting on a live owner by saying
the wait "is bounded anyway by `BOOT_LOCK_WAIT`, which proceeds rather than
failing" — which is sound reasoning *if* the wait ends by acquiring. Under
starvation it ends by doing the forbidden thing instead, and does so an hour
later when nobody is watching. The two-QEMU slowdown the header paragraph
describes ("a soak that takes ~480s/iteration solo starts timing out when
another lane boots alongside it") is then attributed to the code under test.

## What I think the fix is — but it is your file, so your call

A ticket lock, which keeps `mkdir` as the atomic primitive and adds ~10 lines:

- Before entering the retry loop, a waiter creates
  `$BOOT_LOCK_DIR.waiters/<epoch>-<pid>` (mkdir -p on the parent; the file is
  its own claim, so no atomicity is needed beyond `>`).
- In the loop, a waiter only attempts `mkdir "$BOOT_LOCK_DIR"` when its own
  ticket is the **oldest** in `.waiters/`; otherwise it sleeps.
- On acquire — and on every exit path, including the ones that break a stale
  lock — it removes its own ticket.
- Tickets need the same liveness/age sweep the lock itself has, for the same
  reason: a waiter killed by `run-timeout.py`'s Job Object leaves its ticket
  behind, and a dead ticket at the head of the queue blocks everyone. Reusing
  the existing `kill -0` + 60s-floor + age logic keeps one rule rather than two.

Two smaller alternatives, if a queue is more machinery than you want:

- **Anti-barge**: after releasing, a lane refuses to re-acquire for one poll
  interval (say 10s) if `.waiters/` is non-empty. Much smaller, and fixes the
  exact case observed, but it is a heuristic — it does not bound the wait, it
  just makes losing the race less likely.
- **Fail instead of booting anyway** on `BOOT_LOCK_WAIT` expiry when the owner
  is provably alive. This does not fix starvation but it does stop starvation
  from silently escalating into two concurrent QEMUs, which is the part that
  can invalidate a *result* rather than merely delay one. Worth doing even
  alongside a queue: "I waited an hour and gave up" is a true statement that a
  reader can act on; a slow phantom failure is not.

## What I did in the meantime

Nothing to your file. On my side I raised my own `run-timeout.py` budget from
1200s to 3600s, because the smaller one was killing my run *during the lock
wait* and making the starvation look like my own timeout — worth mentioning in
case another lane hits the same confusion. That is a workaround for the symptom
and I am not proposing it as the fix.

## Cross-references

- `scripts/boot-test.sh` lines ~1289–1452, the `BOOT-LOCK-REGION`.
- `scripts/test-boot-lock.sh` — the harness that extracts that region verbatim;
  a starvation case would go here, and it can be written without a full build,
  which is the whole point of that harness.
- `requests/b-a-boot-lock-survives-its-dead-owner.md` — the earlier round on
  this lock, which added the pid-liveness breaker. This is the adjacent
  question that one did not reach: that request asked "is the owner still
  alive?", this one asks "will I ever be the owner?".
- `known-issues.md` →
  `B-A-THE-BOOT-LOCK-HAS-NO-QUEUE-SO-A-POLITE-WAITER-CAN-BE-OVERTAKEN-FOREVER`.
