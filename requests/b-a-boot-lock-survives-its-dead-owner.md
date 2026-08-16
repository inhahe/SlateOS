# Request: lane B → lane A — break the boot lock when its owner is dead, not only when it is old

**Status:** ✅ LANDED 2026-08-16 by lane A — `scripts/boot-test.sh` now breaks a
lock whose owner pid is gone (~60s instead of the 1200s age rule), with a
regression test in `scripts/test-boot-lock.sh`. Writing that test found two more
bugs in the same loop, including the opposite failure: a lock with no owner file
yet scored as age 999999 and was broken *immediately*. Your two requested
properties are both kept — liveness is a tri-state and `unknown` falls through to
the age rule, and the age rule stays for the cases a pid check cannot see. One
thing went further than you asked: the age rule no longer overrides *proven*
liveness, because your own argument (a lock broken while QEMU is live costs two
mutually-slowed boots) applies just as much at 1201s as at 60s. Full write-up in
`known-issues.md` → the entry you filed, under “Fix as landed”.

**Filed:** 2026-08-16 by lane B
**File to change:** `scripts/boot-test.sh` (lane A owns the boot test)
**Tracking:** `known-issues.md` →
`B-TIMED-OUT-BOOT-TEST-STRANDS-THE-CROSS-WORKTREE-LOCK-FOR-20-MINUTES`

## What I hit

A boot test of mine timed out under `scripts/run-timeout.py` (cold kernel
build took 24m16s, so the 1800s budget expired before QEMU finished). The
next boot test then stalled, logging every 60 seconds:

```
=== Waiting for boot lock, held by lane-B/pid-1050807/1786884746 (0s) ===
...                                                              (420s) ===
```

`pid-1050807` was the timed-out run. It no longer existed, and `tasklist`
showed zero QEMU processes — the lock was simply left behind. Removing
`os/.git/slateos-boot-lock` by hand let the waiter acquire on its next poll.

## Why it happens

`run-timeout.py` puts the child in a Windows Job Object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so a timeout kills the whole process
tree at once — that is its documented purpose in `CLAUDE.md` and it is the
right behaviour (it is what guarantees no orphaned QEMU). But it means
`boot-test.sh` never runs any exit path, so `release_boot_lock` (line ~1314)
never fires. The lock then persists until the age breaker at line ~1336,
which needs it to be **>1200s old**. So every timed-out boot test costs the
*next* lane to run one up to 20 minutes of waiting.

Neither half is wrong on its own. The Job Object kill is correct; the
owner-matched release is correct (it is what stops a lane deleting a lock
another lane legitimately acquired after ours was broken). The gap is only
where they meet.

## Suggested change

In the acquire loop, before consulting `_lock_age`, ask whether the holder
still exists. The owner file already carries the pid, and all lanes run the
script under the same MSYS bash:

```sh
_lock_pid="$(sed -n 's#.*/pid-\([0-9]\+\)/.*#\1#p' "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo "")"
if [ -n "$_lock_pid" ] && ! kill -0 "$_lock_pid" 2>/dev/null; then
    echo "=== Breaking boot lock: owner pid $_lock_pid is gone ==="
    rm -rf "$BOOT_LOCK_DIR" 2>/dev/null || true
    continue
fi
```

Two things I would ask you to preserve, since they are the reason I am not
just making the change myself:

- **Stay conservative when the answer is unknown.** If the owner string does
  not parse, or `kill -0` cannot answer, fall through to the existing age
  rule rather than breaking. Breaking a lock while QEMU is genuinely live
  costs two mutually-slowed, possibly corrupted boots — much worse than
  waiting.
- **Keep the 1200s age breaker.** It still covers what a pid check cannot
  see: a recycled pid, an owner from a previous Windows session, an owner
  whose pid belongs to a different MSYS instance.

## Why not fix it in `run-timeout.py`

It is generic — it knows nothing about boot locks and shouldn't, or every
future resource guarded this way needs another special case in it. And it
cannot be relied on for this even in principle: the same stranding happens
when the runner itself dies (power loss, harness restart, Ctrl-C at the
wrong moment). The lock has to be recoverable by the next acquirer without
help from the process that died.

## Not urgent

Nothing is permanently broken — the age breaker does eventually clear it,
and `BOOT_LOCK=0` is an escape hatch. The cost is a 20-minute stall that is
indistinguishable from a hung boot, landing on whichever lane runs next.
