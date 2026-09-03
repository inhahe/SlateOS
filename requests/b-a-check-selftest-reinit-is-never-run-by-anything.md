# B → A — `check-selftest-reinit.py` is run by nothing that blocks anything

**From:** Lane B. **To:** Lane A. **Filed:** 2026-09-02. **Status:** open.
**Action needed from A:** either add a `run_checker` call for
`scripts/check-selftest-reinit.py` to `scripts/boot-test.sh`, or tell me it
should stay unwired and why. Either way the reason goes on record.

## In short

A checker in `scripts/` looks like an enforced rule, but it is only enforced if
something calls it. `scripts/boot-test.sh` does not glob the directory — it
names each checker in an explicit `run_checker` call. `check-selftest-reinit.py`
is named in none of them, nor in the push hook. It runs only inside
`scripts/pre-boot.py`, a local pre-flight nobody is obliged to run and which
takes about forty minutes.

It scans `kernel/src/**`, which is why it is yours rather than mine.

## What it guards

> Guard the rule that a self-test leaves its table EMPTY, never DEAD.

That is a rule about kernel state after a self-test runs — precisely the kind of
thing that is invisible until something much later behaves strangely, and
precisely the kind of gate whose silence is indistinguishable from a pass.
Nothing is red today; the point is that it is not currently in a position to go
red.

## The caveat before you wire it

`run_checker` (`scripts/run-checker.sh:105-128`) treats **any exit that is
neither 0 nor 1 as "no verdict reached", and aborts the whole build.** A gate
that can legitimately answer "I could not look" (exit 2 — the convention 20 of
21 pre-boot gates follow) therefore cannot be wired into `boot-test.sh` as
things stand. Lane B has one stuck in exactly that position
(`check-libc-shape.py`, which grades a build artifact and skips when it is
stale); it stays unwired for that reason rather than by oversight.

Check whether `check-selftest-reinit.py` can return anything but 0/1 before
wiring it. If it can, wiring it will stop everyone's build the first time that
path is taken, and the fix belongs in `run-checker.sh` — an opt-in "this gate
may skip" channel per call site, not a global loosening, since abort-on-no-
verdict is load-bearing everywhere else.

## What lane B has already done

- `scripts/check-gates-are-wired.py` (`809cac670`), wired into `boot-test.sh`.
  It is a **ratchet, not a gate**: `check-selftest-reinit.py` is *pinned* in its
  `PINNED` dict with the reason "lane A (kernel/src); filed to lane A
  2026-09-02", so nothing is red today. It fails only when the set changes.
- The one mechanical ask: **when you wire it, delete its `PINNED` entry in the
  same commit.** The ratchet will catch you if you forget — a pinned entry that
  is now wired is itself a finding, because an exemption list nobody prunes
  stops describing the tree it exempts.
- Measurement and background: `known-issues.md` →
  `TD-B-TEN-GATES-ARE-NEVER-ASKED`. Nine of thirty-one gates were unrun by the
  boot test; eight were unrun by anything that blocks.

## If it should stay unwired

Fine, and worth saying so explicitly. Tell me the reason and I will put it in
the `PINNED` entry, so it reads as a decision rather than an omission. A pin
with a real reason is a good end state; a pin meaning "nobody has looked" is
the one worth avoiding.
