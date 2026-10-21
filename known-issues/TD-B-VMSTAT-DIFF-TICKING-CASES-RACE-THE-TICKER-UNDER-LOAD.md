## TD-B-VMSTAT-DIFF-TICKING-CASES-RACE-THE-TICKER-UNDER-LOAD (lane B, 2026-10-09) — OPEN

**Status:** OPEN

**What.** `scripts/vmstat-diff.sh`'s ticking cases (`TICK=1`, the `tick` and
`tickdebt` worlds) can fail on one side when the machine is busy. Seen
once, 2026-10-09, on `vmstat -w -a 1 4 [world=tick] [ticking]`, while a
`git push`'s gates were running in another worktree: the third report's
`bi` was 7 on our side and 0 on upstream's. Every other column agreed, and
the case agreed in the runs before and after it.

**Why.** `ticker.py` writes each state's files over the fixture at 0.5 s,
1.5 s and 2.5 s after it starts, one file at a time and in name order
(`stat` before `vmstat`). vmstat reads at 0 s, 1 s, 2 s and 3 s. A ticker
or a vmstat that falls half a second behind -- or one whose reading falls
between the ticker's writes of `stat` and `vmstat` -- sees part of one
state and part of the next, and the two sides need not see the same mix.
Nothing is wrong with either vmstat; the harness's timing is.

**Where.** `scripts/vmstat-diff.sh`: `ticker.py` and the `TICK=1` cases.

**How to reproduce.** Run the harness while something else keeps every
processor busy; the ticking cases fail now and then, on either side.

**The proper fix.** Take the timing out of it: switch states when vmstat
waits rather than on the clock -- for instance, run each side under a
`LD_PRELOAD` shim (or `ptrace`) that swaps the fixture on each `nanosleep`
vmstat makes between reports, or bind each state's directory over
`/proc` in turn from inside the namespace at those calls. Until then, a
failure of a `[ticking]` case alone, with the case agreeing on a rerun, is
this and not a regression.
