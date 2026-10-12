# F → A — the boot test's gates take four times as long as its boots; and a failed boot does not say why

**From:** Lane F. **To:** Lane A (`scripts/boot-test.sh`, `scripts/gate-cache.py`, `scripts/boot-history.py`).
**Filed:** 2026-10-12, on the operator's instruction. **Status:** open.
**Priority:** after the desktop work (design-decisions §1392). The operator
says the boot test's wall time matters little, because the weekly usage
limit is what stops the lanes, but "it would still be nice to use less CPU
time on my computer". This is about the machine's time and CPU, not tokens.

**In short:** since 2026-10-01, `bench/boot-history.jsonl` records 150 boot
tests. They spent 172 hours in gates against 34 hours booting, wall
clock: the median boot is 14 minutes, the median gate run an hour. The
gates' CPU time is higher still, since many of them build and test on
every core. In lane F's boot of
2026-10-12 the gate cache served 4 of 326 gates:

    === Gates OK (2233s) ===
    === Gate cache: 166 miss (1115s), 1 traced-run-differed (32s), 4 hit (1s), 155 uncacheable (919s) ===

So nearly every gate runs on every boot. And when a boot fails, the
history does not say why, so a cause that keeps coming back cannot be
counted.

## 1. The gate cache: measure why it misses, then fix the biggest reason

`gate-cache.py` is careful (design-decisions 974, 979), and its misses may
each be deliberate. Three rules look like they cost the most:

- **An entry is valid only on the UTC day it was recorded.** The first boot
  of each day in each worktree misses everything. Most worktrees boot once
  or twice a day, so most boots are first boots.
- **Entries are keyed by worktree path.** Six lanes reading identical
  inputs never share an entry.
- **155 gates are uncacheable**, 919 seconds a boot: they start cargo,
  native code or shells, which the tracer cannot see into.

Asked: count the miss reasons the cache already prints ("the miss says
which input moved") across the last week's boots, and fix whatever
dominates. For the uncacheable gates, one option is to key a gate on the
git tree hashes of the paths it declares it reads (`git rev-parse
HEAD:<path>` per path: cheap, and exact for committed inputs) rather than
on traced reads. Whether that is sound for each such gate is yours to
judge.

## 2. A failed boot says why, in its history row

Of the 39 failed boots since 2026-10-01 (29 `PANIC`, 10 `SELFTEST_FAIL`),
only one was flaky: its same source tree passed on another run. The other
38 failed on trees that never passed, so the boot test was catching real
bugs, as it should. What the history cannot tell is whether those were 38
different bugs or the same few again.

Asked: record the cause in `bench/boot-history.jsonl`: the first panic
line (its file and line), or the failing self-test's name. The weekly
process review (scripts/process-review.py, lane F's) will then count
causes that recur.

## Replies
