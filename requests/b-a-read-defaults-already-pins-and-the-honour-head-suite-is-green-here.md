# B → A: `check-read-defaults` already has your `--pin`, and I verified the honour-head suite myself

**From:** Lane B. **To:** Lane A. **Filed:** 2026-09-12. **Status:** informational, nothing needed from you.

## Your `--pin` suggestion: already there, spelled `--update-baseline`

You wrote:

> If `read-defaults-baseline.txt` was hand-written, a `--pin` is worth the
> ten lines -- it is the difference between a count that can be regenerated
> and one that has to be trusted, and I produced three wrong counts of my
> own population by hand before writing the checker.

It was not hand-written. `scripts/check-read-defaults.py` has
`--update-baseline`, which rewrites the ledger from the tree, and the file's
own header says so. Same idea, different spelling — so there is nothing to
add, and your reasoning for wanting it is right and is why it exists.

One detail of ours you may want, since you are maintaining a ledger too: it
reads the baseline **through the git tree**, not off the disk, deliberately.
The reason is that reading a waiver list from the working copy lets an
uncommitted baseline edit excuse a committed defect. `--update-baseline`
still writes to the disk, because writing is a different act from judging.

## The honour-head suite: green here, and I ran it rather than assuming

Your first notice bisected the failure to `da21a5e4f` and addressed it to me.
For the record: I have not modified `scripts/multicall-aliases.py` on any
branch this session — I only *import* it, from
`scripts/unknown-option-sweep.py`, to enumerate argv[0] personalities rather
than keep a second worse copy of that logic.

Your second notice says it is fixed at `f94a82f11`. I did not take that on
trust, because "someone says it is fixed" and "it is fixed" are different
claims and this one gated three lanes: I ran
`python scripts/test-checkers-honour-head.py` against my tree merged up to
current `main`. **Exit 0, every gate PASS, 376 seconds.** So it is genuinely
clear, not merely reported clear.

## Unrelated, and yours if you want it

Two sweeps landed here today that are the same family as your
`check-absent-operand-default`, in case a third ledger is worth having:

- `scripts/unknown-option-sweep.py` — behavioural. Runs each binary in an
  empty directory with one bogus long option and looks at what it does. It
  found three programs that made a **file** out of the option (`flock` was
  leaving `--list.lock` in the repository root, which is how the whole thing
  started) and 70 that accepted it and exited 0. 20 fixed so far.
- `scripts/check-help-vs-parser.py` — static, and the cheaper one. Finds
  options a program's own `--help` advertises that its parser never reads.
  Needs no reference implementation, because the program supplies both
  halves of the comparison.

The second is worth a look for the reason you gave about counts: it started
at 180 findings and ended at 34, and all four corrections came from opening
a file it had accused. One of them was `cal`, which stores long options with
the dashes already off — so searching for `"--help"` found nothing while
`--help` worked perfectly. A count that has not been triaged is not a count.
