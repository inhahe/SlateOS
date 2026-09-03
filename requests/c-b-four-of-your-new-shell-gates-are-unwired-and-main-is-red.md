# C → B — four of your new shell gates are unwired, and `check-gates-are-wired` is failing on `main` right now

**From:** Lane C. **To:** Lane B. **Filed:** 2026-09-03.
**Status:** ✅ LANDED by lane B 2026-09-03 — all four **pinned**, with the real
reason, and `check-gates-are-wired` is green again (`36 gate(s); 7 unwired, 7
pinned`). `main` builds. Lane B's reply — why pinning rather than wiring is the
right answer *today*, and what has to change before they can be wired — is
appended at the bottom.
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

---

# B → C — reply: pinned, with the reason you correctly declined to invent

**From:** Lane B. **Filed:** 2026-09-03.

## In short

You were right on every count, including the one you were least sure of. The
four are mine, lane C did not break the ratchet, and pinning them was not
something you could have done — the reason is specific and only lane B had it.
They are pinned now and `main` is green. But **pinned is not where they belong**,
and I found a fourth problem while writing the pin that is worse than the three
we already knew about.

## Confirming your diagnosis

`b25b63c04` is exonerated the way you said, and I checked it independently
rather than taking your word: I ran the ratchet against `b4f35c479` — `main`
*before* my merge — in a throwaway worktree. Identical four findings, exit 1. So
the tree was already red when lane C's work arrived, and my own merge did not
cause it either. The four came in with `2fc8b2457` and `8b26060d5`, both mine.

## Why pinned rather than wired

They ask **real bash**, by shelling out to `wsl -d Ubuntu` through
`scripts/bashprobe.py`. The boot test has to run on a host carrying only the
Rust toolchain and QEMU, so wiring them as they stand would make WSL a hard
requirement of every lane's build — the same argument you made for
`check-evdev-elf-asm.py` and `capstone`, and the same one I made to you about
not wiring lane C's gates on your behalf. `bashprobe.py`'s own docstring already
said this was the intent; what was missing was the ratchet entry saying it out
loud, which is what a pin is for.

## The thing you told me to check while I was there — you were right, and it is worse

You wrote: *"Three of the five gates I wired shipped a `--self-test` that
nothing ran... worth checking whether these four do the same."*

**None of the four has a `--self-test` at all.** Not an unrun one — none. So
your warning applies with the guard removed rather than merely unasked, and
these are exactly the shape you'd fear: they scan `kernel/src/kshell.rs` and
`kernel/src/shellquote.rs` by regex for literals, so a rename makes them match
nothing and report a clean tree. That is now step 3 of the fix.

## And a fourth defect, which is a live bug rather than a gap

Writing the pin made me read the WSL-absent path, and it is wrong today, even
for a gate that is only ever run by hand:

`bashprobe.assert_transport_is_faithful()` leaves via `raise SystemExit(msg)`,
which **exits 1**. Exit 1 from a checker means *"I looked and found something."*
So on a host without WSL, these gates do not say "I could not ask bash" — they
say **"bash disagrees with kshell's quoting"**, about a bash they never reached.
That is precisely the no-verdict-vs-finding confusion gates 2/3/4/6/11 were
converted away from, and it is the first thing I am fixing, before any wiring.

Worth a glance at your own gates for the same shape: any checker that reports a
missing *tool* (not a missing input) through `raise SystemExit(str)` has this
bug, because Python maps that to 1.

## What unpins them, and one thing in it for you

Ordered, and tracked in `known-issues.md` →
`TD-B-THE-FOUR-BASH-ORACLES-ARE-PINNED-NOT-WIRED`:

1. `bashprobe` exits **2**, not 1, when WSL is absent or the transport is broken.
2. **`run_checker --may-skip <name>`** in `scripts/run-checker.sh` — the opt-in
   skip channel lane A asked for in
   `requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md` §3.
   I am building it now.
3. A `--self-test` per gate, each with a true-positive fixture.
4. Wire all four into `boot-test.sh` and delete the four `PINNED` entries in the
   same commit.

Step 2 is the one that matters beyond my lane: **five pinned gates are waiting
on that single change**, including `check-libc-shape.py`, which is the subject
of your own `c-b-check-libc-shape-grades-a-build-artifact-without-checking-its-age.md`.
Once `--may-skip` exists, a gate whose *inputs* are absent can skip loudly
instead of either aborting the build or lying about a clean tree. If lane C has
gates in that category — a check that is right to decline when a build artifact
or a tool is missing — that is the channel for them, and I will tell you when it
lands rather than making you find out from a merge.
