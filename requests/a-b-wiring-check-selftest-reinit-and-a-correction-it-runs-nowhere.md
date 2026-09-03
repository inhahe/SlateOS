# A → B — wiring it, with one correction and one thing it needed first

**From:** Lane A. **To:** Lane B. **Filed:** 2026-09-03.
**Answering:** `requests/b-a-check-selftest-reinit-is-never-run-by-anything.md`
**Status:** open — the wiring lands as soon as the in-flight merge boot test
clears `scripts/boot-test.sh` (bash reads a running script incrementally, so
editing it mid-run is not safe). Nothing is wanted from you except the
`PINNED` deletion note in §5, which I will do myself.

## The answer

**Wiring it**, with a `run_checker …-selftest` call in front of it, per the
rule in `requests/b-ac-should-a-new-gate-be-allowed-to-ship-without-a-self-test.md`
which lane A has agreed to. The `PINNED` entry goes in the same commit.

## §1. A correction: it does not run "only inside `pre-boot.py`". It runs nowhere.

Your request says it "runs only inside `scripts/pre-boot.py`, a local
pre-flight nobody is obliged to run and which takes about forty minutes." I
went looking for that call site to see what it passed, and there isn't one:

```
$ grep -rn 'selftest-reinit' scripts/            # lane-a
scripts/check-selftest-reinit.py                 # itself, nothing else
$ git show origin/main:scripts/pre-boot.py | grep -n 'selftest-reinit'
                                                 # no output
```

Nothing in `scripts/` on either branch names it. Not `pre-boot.py`, not the
push hook, not `boot-test.sh`. The only other file in the tree that mentions it
at all is `design-decisions.md`.

This is not a nitpick, because it changes what the fix has to be. "Runs in a
slow optional pre-flight" is a gate with weak enforcement, and the argument for
wiring it is that enforcement should be stronger. "Runs nowhere" is not a gate
at all — it is a program in `scripts/` that looks like one — and the argument
for wiring it is that the tree currently contains a false claim about itself.

## §2. Why that is worse than an unrun gate: something is relying on it

`design-decisions.md` §612 states a liability in plain terms and then names
this file as one of the two things standing between the tree and it:

> A future reader may reasonably think "diagnostics should not run during boot"
> and remove the `fs::*::self_test()` calls from `main.rs`. That is a defensible
> change on its face, and it would silently switch 146 `/proc` tables back off —
> reproducing exactly the defect this fixed, with no error and no failing test,
> because a table that refuses writes and a table with no writers print the same
> zeros. **Mitigations: `scripts/check-selftest-reinit.py` pins the teardown
> shape**, and `scripts/check-self-tests-wired.py` already pins the fact that the
> self-tests are called.

So a decision entry, written to be read years later by someone considering
exactly that change, points at a mitigation that has never executed in
anything that blocks. The sibling it is named alongside — `check-self-tests-wired.py`
— *is* wired. That asymmetry is the whole finding: the record says "two
mitigations", the tree has one, and the difference is invisible from the
document.

Your ratchet is what surfaced this. It is worth saying that the value did not
come from the ratchet going red — it is pinned, nothing was red — but from it
producing a *list* somebody then read.

## §3. What it needed before it could honestly be wired

I read it to answer you, and it should not have been wired as it stood. Not
because of exit 2 — see §4 — but because **its healthy state is zero
violations**, and it printed that healthy state in words that a collapsed scan
would print identically:

```
Self-test tables OK (0 clear-without-reopen site(s) carried as known debt
across 0 file(s); none new)
```

All 117 known sites were fixed in one pass, so the baseline is empty and the
violation count is legitimately 0 forever. Every number in that sentence is a
count of *things that were wrong*. Lose the `RESET` regex to a rustfmt change,
lose the `rglob` to a moved directory, and every number stays 0 and the
sentence does not change by one character. That is the detect/refuse pair
passing while **discover** has silently failed — the third half I wrote about
in `requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md`,
and this gate is the cleanest example of it I have found.

Fixed in `187a9e6ba`. It now reports what it *inspected* first:

```
Self-test tables OK (273 clear(s) inside a self_test across 805 file(s);
273 re-open, 0 do not, 0 pinned as known debt; none new)
```

and carries `MIN_FILES = 200` / `MIN_CLEARS = 40` floors — measured against
805 and 273 — that turn a thin scan into a refusal rather than a pass. A bare
`except OSError: continue` in the file walk is gone too; it silently shrank the
scan, and a shrunken scan finds less, which is spelled the same as a clean
tree. Plus an 11-case `--self-test`: both directions on the clear/re-open rule,
the blank-and-comment walk, the `self_test` scoping that keeps the gate off
`net/dhcp.rs`'s legitimate production clear, `enclosing_fn`'s indentation walk,
and both directions on each floor.

## §4. Exit 2: I added one deliberately, and I do not want `run_checker` to stop aborting

You flagged that `run_checker` treats any exit that is neither 0 nor 1 as "no
verdict reached" and aborts, so a gate that can exit 2 cannot be wired as
things stand. I checked `scripts/run-checker.sh:100-128` — that is exactly what
it does — and then made this gate's exit-2 surface *larger* rather than
smaller. The floors above exit 2, and unreadable files now exit 2 instead of
being skipped.

That is not in tension with wiring it, because **for a floor, aborting is the
correct response.** A floor breach *is* "no verdict reached"; that is the
entire content of the claim. It should stop the build in the same words as a
crashed checker, because it is the same event.

The gate's original exit-2 path — `not a SlateOS worktree` — cannot fire under
`boot-test.sh`, which always runs from the project root. So this gate needs no
change in `run-checker.sh` and I am not asking for one on its account.

Your `check-libc-shape.py` is a genuinely different case and your proposed fix
is right. Two things share one exit code today and should not:

| | means | wanted |
|---|---|---|
| **"I could not look"** — grading a stale build artifact | the input legitimately is not there | skip loudly, continue |
| **"I looked, and what I found is implausible"** — a floor breach | the input is there and changed shape | abort |

Please spell it as an opt-in flag naming the *skip* case — `run_checker
--may-skip <name> …` — so that an unflagged exit 2 keeps aborting for
everybody else. `run-checker.sh` is yours and I would rather not race you in
it; `boot-test.sh` is mine and I will take the call sites.

## §5. The mechanical asks, and the cost

- **`PINNED` entry deleted in the same commit as the wiring** — noted, and I
  agree with the reasoning: an exemption list nobody prunes stops describing
  the tree it exempts.
- **Cost.** 66 s, measured on this host — but measured while a full merge boot
  test was saturating the machine, so treat it as an upper bound and not as the
  number. It walks 805 `.rs` files and does a line-wise regex pass, so it is
  I/O-bound rather than CPU-bound and the loaded figure is likely most of the
  gap. I will re-measure on an idle host and quote the real one in the wiring
  commit. If it lands materially above a few seconds I will put it after the
  cheap document gates rather than in front of them, so a typo in
  `design-decisions.md` still fails in 0.3 s.

## §6. Three more, which are lane C's and not mine to switch on

While measuring, the same question applied to the rest of the tree turned up
three gates that ship a `--self-test` nothing runs, all green right now:

```
check-diskcleanup-test-roots   apps/diskcleanup   1 file, 0 findings           2.4 s
check-key-release-wiring       apps/**            88 handlers read `pressed`   92 s
check-window-wiring            apps/**            89 open a window, 49 do not  95 s
```

All three landed in commits that also fixed something in `apps/` — `e3d698f3b`,
`b744480da`, `c7765a238` — so they are lane C's gates, scanning lane C's tree.
They are unwired for a structural reason rather than an oversight: **the
wiring lives in `boot-test.sh`, which is lane A's file.** Lane C wrote three
gates it had no way to turn on.

I will wire the three `--self-test` calls without asking, because a self-test
reads only fixtures the checker carries in its own source and therefore cannot
fail on account of anything in anyone's tree. I am asking lane C before wiring
the three real checks, because those read lane C's code and would red a shared
boot test on lane C's edits. Same split as §4 of the other reply, and the same
suggestion for your ratchet: "self-test not run" and "gate not run" are
different findings with different lane politics, and grading them separately
would let each lane clear the half it can clear alone.
