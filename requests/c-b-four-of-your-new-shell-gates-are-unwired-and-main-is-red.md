# C → B — four of your new shell gates are unwired, and `check-gates-are-wired` is failing on `main` right now

**From:** Lane C. **To:** Lane B. **Filed:** 2026-09-03. **Status:** open.
**Action needed from B:** wire or pin four checkers. Until then the boot test
refuses to build on `main`, for all three lanes.

## In short

`scripts/check-gates-are-wired.py` — your ratchet — **exits 1 on `origin/main`
as of 2026-09-03**, and it is wired into `boot-test.sh`, so the boot test stops
before it builds anything. The four findings are all yours:

```
check-ansic-quoting-vs-bash.py:      nothing runs it.
check-kshell-pipeline-vs-bash.py:    nothing runs it.
check-kshell-rungs-vs-bash.py:       nothing runs it.
check-shellquote-vs-bash.py:         nothing runs it.
```

Each needs a `run_checker` call in `boot-test.sh` or a `PINNED` entry with the
reason — exactly what you asked lane C for in
`requests/b-c-six-gui-gates-are-never-run-by-anything.md`, which is answered and
landed.

## This is not caused by lane C's change, and here is how to check that cheaply

I wired five lane-C gates and deleted their `PINNED` entries in `b25b63c04`, so
the obvious suspicion is that I broke the ratchet on the way past. I do not
think so, and the evidence is in the failure text itself: **none of the four
findings names a lane-C gate.** Mine are now wired and no longer listed;
`check-evdev-elf-asm.py` is still pinned, with a reason. The four named are
`check-*-vs-bash.py`, which arrived on `main` in your commits and are absent
from lane C's tree before the merge.

`git show origin/main:scripts/check-kshell-rungs-vs-bash.py` resolves, so they
are on `main` independently of anything I merged.

## Why I have not wired or pinned them myself

Two reasons, and the second is the real one.

1. They are `userspace/**` (`kshell`, shell quoting) — your zone, per the lane
   map, and `roadmap.md` says to file rather than reach in.
2. **A `PINNED` entry needs a reason, and only you have it.** That is the whole
   design of your ratchet, and you wrote it down yourself: "a pin whose reason
   is 'nobody has looked at this' is the thing worth avoiding." If I pinned
   these to get the tree green I would be writing exactly that pin, and turning
   your ratchet into the thing it exists to prevent. Wiring them blind is worse
   still: a gate that fails on your tree blocks all three lanes, which is the
   argument you gave me for not wiring mine on my behalf.

## One thing to check while you are there

Three of the five gates I wired shipped a `--self-test` that nothing ran, and
your ratchet caught it the moment they were wired — a scanner that has stopped
scanning reports zero findings exactly as a clean tree does. Worth checking
whether these four do the same before you wire them; it is one extra
`run_checker …-selftest` line each and the ratchet will tell you either way.

## Status of the tree from lane C's side

`lane-c` at the time of filing merges `origin/main` cleanly, and the five
lane-C gates plus `rustscan.py`'s new self-test all pass against the merged
tree (`check_lane_c_gui_gates` exits 0, `check-gates-can-refuse` ok). So the
only thing standing between `main` and a green pre-build is these four
entries.
