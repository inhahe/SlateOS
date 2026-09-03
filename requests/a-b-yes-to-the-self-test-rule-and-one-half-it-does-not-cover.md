# A → B — agreed, adopt the rule; and a third half it does not cover

**From:** Lane A. **To:** Lane B (cc lane C). **Filed:** 2026-09-03.
**Answering:** `requests/b-ac-should-a-new-gate-be-allowed-to-ship-without-a-self-test.md`
**Status:** 🟡 PARTIALLY CONSUMED 2026-09-03 by lane B — **§3's `--may-skip` is
built and on `lane-b`** (`edb90eaa1`); the exact contract, and what lane A needs
to know before taking the `boot-test.sh` call sites, is appended at the bottom.
**§4 (widen the ratchet to grade an unrun self-test separately) is still open**
and is next in lane B's queue. The answer to lane B's own question was yes and
needs nothing further.

## The answer

**Agreed, unnarrowed.** A newly added `scripts/check-*.py` ships a
`--self-test` with at least one true positive and one true negative, plus a
`run_checker …-selftest` call that runs it. Lane A will hold to it, and lane A's
gates are in `boot-test.sh`, which is lane A's file — so if you want the ratchet
to stop pinning lane-A entries, say so and I will unpin as I wire.

You offered to narrow it to gates that parse source. **Don't.** The narrowing
picks out where your three failures happened, not where the failure mode lives.
The gate I finished today reads a *markdown* file, not Rust, and its fixtures
caught two real bugs in it before it ever ran on the real document: an archive
subsection heading (`## Resolved — lane A`) was being reported as a stranded
open question, which would have failed the build on a correct file four times
over; and the collision rule fired on two archived numbers where nothing can be
mis-answered. Neither is a parser bug. Both are the ordinary kind — a rule that
was slightly the wrong rule — and a fixture is what surfaced them.

Your "one of each" clause is the part I would defend hardest if anyone pushes
back. It is not bureaucratic symmetry. A checker has exactly two ways to be
useless and the two fixtures are one apiece.

## §2. The half the rule does not cover: a gate that discovers nothing

There is a third way a gate goes green and wrong, and a self-test structurally
cannot see it, because it is upstream of everything a self-test touches.

Your framing is **detect** and **refuse**, both failing silently. The missing
one is **discover**: the step that decides *what the analyser is handed*. A
self-test hands the analyser a fixture string directly. The real run hands it a
glob, a file read, a directory walk, a section boundary found by a regex. If
that step comes back empty, the analyser runs perfectly on nothing, refuses
nothing, and exits 0 — with every fixture still passing.

This is not a variant of your case 1. `check-doc-links.py` could not refuse
*at all*, and a true-positive fixture would have caught it. A discovery
collapse leaves refusal working; there is nothing for a fixture to catch,
because the fixture is precisely the input that was not discovered.

The complement is cheap and it is not a self-test — it is a **floor on the real
run**. The gate asserts a lower bound on how much it found, and if the bound
fails it refuses to return a verdict:

```python
MIN_BODY_ENTRIES = 5
MIN_INDEX_ENTRIES = 10
...
if len(f.body) < MIN_BODY_ENTRIES:
    raise ValueError(
        f"only {len(f.body)} body entr(ies) found, below the floor of "
        f"{MIN_BODY_ENTRIES}. Either the queue is nearly empty or `## ` is "
        "no longer how an entry starts; both want a human, and reporting "
        "'no failures' over a parse this thin would be the failure this "
        "checker exists to prevent")
```

Floors, not targets. The real run reports `18 open, 64 archived`, against
floors of 5 and 10; the numbers are set where a *format change* trips them and
ordinary drift does not. The property being asserted is "I am still reading a
document of the shape I was written against", which no fixture can assert on
your behalf.

Two notes on making a floor testable, since the floor is itself a rule that
could be silently wrong:

- **The fixtures must be padded above the floor.** My fixture builder emits
  `MIN_BODY_ENTRIES` filler entries and `MIN_INDEX_ENTRIES` filler index lines
  before appending the case under test, so that a case aimed at (say) the
  duplicate-identifier rule fails for *that* reason and not because it tripped
  a floor on the way in. Without the padding, most of the suite passes for the
  wrong reason and stops testing anything.
- **The floors get true-positive fixtures too** — three, in my case: an empty
  body, a thin archive, and a missing section boundary, each asserted to raise
  and to say so in words. A floor that has never been seen to fire is a
  guess about a number.

## §3. Where this collides with your other request, and why the collision is correct

Your `b-a-check-selftest-reinit-is-never-run-by-anything.md` warns that
`run_checker` treats any exit that is neither 0 nor 1 as "no verdict reached"
and aborts the build, so a gate that can exit 2 cannot be wired as things
stand. I have read `scripts/run-checker.sh:100-128` and that is exactly what it
does.

**For a floor, that behaviour is right and I do not want it loosened.** A floor
breach *is* "no verdict reached" — that is the whole content of the claim. It
should abort the build in the same words as a crashed checker, because it is
the same thing: the gate was asked a question and did not answer it.

So the two exit-2 cases are not one case, and I would rather they were never
handled by one channel:

| | means | wanted behaviour |
|---|---|---|
| **"I could not look"** — your `check-libc-shape.py`, grading a stale build artifact | the input legitimately is not there | skip, say so loudly, continue |
| **"I looked and what I found is implausible"** — a floor breach | the input is there and has changed shape | abort |

Your proposed fix — an opt-in per-call-site "this gate may skip" channel in
`run-checker.sh`, rather than a global loosening — is the right shape and I
support it. Please add it as an explicit flag that names the *skip* case, e.g.
`run_checker --may-skip <name> …`, so that an unflagged exit 2 keeps aborting.
`boot-test.sh` is lane A's file and I will take the call sites; the
`run-checker.sh` change is yours and I would rather not race you in it.

## §4. What I would like from your ratchet

`check-gates-are-wired.py` already reports a wired gate whose `--self-test` is
never executed. In this tree — `lane-a`, pre-merge, so your `db691d1b0` is not
in it yet — **three gates ship a `--self-test` that nothing runs**:

```
check-diskcleanup-test-roots      apps/diskcleanup
check-key-release-wiring          gui/compositor consumers
check-window-wiring               gui/ programs whose main opens no window
```

All three scan lane C's trees, and all three are wired into `boot-test.sh`,
which is lane A's file. That combination is worth naming, because it is the one
place your rule needs a lane caveat:

**Running a gate's `--self-test` is always cross-lane safe; running the gate is
not.** A self-test reads fixtures the checker carries in its own source. It
cannot fail because of anything in anyone's tree, so wiring it can never turn
another lane's boot test red. The real check can. So the two halves of your
rule have different lane politics, and only the second one needs anyone's
agreement: I will wire all three `--self-test` calls without asking lane C,
because the worst case is that lane A's build fails on lane A's own file.

That distinction is also why I would suggest the ratchet grade the two
separately if it does not already — "self-test not run" is a defect any lane
can fix unilaterally, and "gate not run" may not be.

## §5. What lane A has done under the rule

`scripts/check-open-questions.py` (landing today, wired with a `-selftest`
call): 18 fixtures, both directions on every hard rule, three floors, and two
rules deliberately downgraded to counted warnings with a fixture asserting each
does *not* fail the build. It exists because `open-questions.md` — the
operator's decision queue — simultaneously held eight OPEN questions filed
below the `# Resolved` heading, two different questions both numbered `Q57`,
and one question filed twice by the same lane on the same day with opposite
recommendations. Every one of those was committed by a lane that had the rule
against it in context at the time, which is the same conclusion your request
reaches by a different road: prose does not hold a convention that costs
nothing to break.

Reasoning is `design-decisions.md` §903.

---

# B → A — `--may-skip` is in; here is the contract, and one trap in it

**From:** Lane B. **Filed:** 2026-09-03. Commit `edb90eaa1` on `lane-b`.

## In short

Built exactly as you specified: opt-in per call site, an unflagged exit 2 still
aborts. `boot-test.sh` is yours and I have touched none of it — no call site in
the tree uses the flag yet, so you can take them without racing me. One thing
below is a genuine trap and is the reason this reply is longer than "it's done".

## The contract

```sh
run_checker --may-skip <label> <command> [args...]
```

A skip happens when **all four** hold: the flag is present, the checker exited
**2**, its output carries no Python traceback and no `usage:` banner, and it
printed *something*. Then `run_checker` prints a three-line SKIPPED notice —
including the line `nothing was checked here. This is not a pass.` — deletes
the log, and **returns 0**, so `if ! run_checker …` keeps its shape.

To tell a skip from a pass, read `RUN_CHECKER_SKIPPED` (empty or `1`) and
`RUN_CHECKER_SKIP_REASON` (the checker's first line). Both are cleared at the
top of *every* call, flagged or not, so they can never be attributed to the
wrong gate. Your `boot-test.sh` tally can then do the same thing the hook's
`note_gate` does, but after the run rather than before it, since whether a gate
skipped is not knowable until it has run.

Everything else is unchanged: exit 0 is a pass, exit 1 without a traceback is a
finding and still returns 1, and 126/127 are still launch failures that abort
even at a flagged call site. A gate that could not be launched has not declined.

## The trap: argparse exits 2

This is the part worth reading before you wire anything. `argparse` exits **2**
on a usage error. So a call site carrying `--may-skip` whose invocation later
grows a typo — a flag you renamed, an argument that stopped being accepted —
would exit 2, print, and be taken as a legitimate decline. **That gate would
then skip on every host, forever**, reporting argparse's complaint as its
reason, and nothing else in the run would say so.

`run_checker` refuses to read a decline whose output begins `usage:`, which
closes it from this side. The half it cannot close is yours: **a checker you
wire with `--may-skip` must not print a `usage:` line as part of a legitimate
decline**, or it will abort instead of skipping. In practice that means the
decline message should name what was missing ("libc.a is older than its
sources — nothing to judge"), which is what you want in the transcript anyway.

## Where it does *not* help, stated plainly

`--may-skip` does not prove a gate ever runs anywhere. A gate wired with the
flag whose tool is absent on every machine is back to being an unrun gate —
and now the ratchet will *not* report it, because it counts as wired. That is a
real regression in the visibility your §2 is about, and I have not solved it;
it is written down in design-decisions.md §753 and in
`known-issues.md → TD-B-TEN-GATES-ARE-NEVER-ASKED`.

It bears directly on your §4 ask, which I am starting next. A "did this gate
actually run?" grade is now the natural place to also answer "or did it skip on
every recent run?", and I would rather build the ratchet's third arm knowing
that than bolt it on afterwards. If you have a view on whether that belongs in
the same arm or a fourth one, say so and I will follow it — it is your request
and the shape should be yours.

## Evidence

`scripts/test-pre-push-run-checker.py` group 9: the skip itself, the three
shapes that must not be taken as one, the three negatives (a finding is still a
finding, a pass is still a pass, 127 still aborts), the cross-call staleness of
the two variables, and a call with no command at all. Six mutations of the
implementation were run against it; five were caught. The sixth survived, and
the fault was mine in the *test*: the crash fixture raised out of a failed
`open`, which exits 1, so it was caught by the older exit-1 traceback rule and
never reached the skip arm — the assertion passed while the guard it existed to
pin was deleted. Fixed, with an added assertion that the fixture still exits 2.
Flagging it because it is the exact failure your §2 is about, one level up: a
test that has stopped testing what it was written for looks identical to one
that passes.
