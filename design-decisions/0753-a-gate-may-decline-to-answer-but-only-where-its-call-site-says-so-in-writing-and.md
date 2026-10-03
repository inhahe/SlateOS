## 753. A gate may decline to answer, but only where its call site says so in writing — and a decline must be spoken, not merely returned

**Date:** 2026-09-03
**Lane:** B
**Decided by:** Claude (autonomous), on lane A's request and to lane A's stated constraint

**In short:** Some checks genuinely cannot answer on some machines — one needs
a compiled library that may not have been built yet, four need a Linux
subsystem the build host is not required to have. Until now a check that said
"I cannot answer" stopped the build, so rather than stopping everyone's builds
those five checks were simply *never run at all*, on any machine, which is
worse: an unasked check enforces nothing. They now have a way to bow out — but
only at a call site that was written to allow it, and only if they say out loud
what they could not do. A check that bows out silently still stops the build,
because a silent skip is indistinguishable from a check that quietly broke.

### The situation

`run_checker` (`scripts/run-checker.sh`) grades every checker's exit status: 0
is clean, 1 is a finding, and **anything else is "no verdict" and aborts the
run**. That rule is the reason a crashed checker can no longer be reported as a
finding against the operator's code, and it is load-bearing.

But "no verdict" is not one cause. `check-libc-shape.py` returns 2 when
`libc.a` is older than its sources — it is not judging the tree, it is saying
the artifact it grades is stale. The four `check-*-vs-bash.py` oracles ask real
bash through `wsl -d Ubuntu`, and on a host without WSL they cannot ask at all.
For all five, aborting the build is the wrong answer, so all five sat in
`check-gates-are-wired.py`'s `PINNED` dict as deliberately-unwired. Five gates
that never run, to avoid five gates that abort — the ratchet was honest about
it, which is how the cost stayed visible.

### The decision

`run_checker [--may-skip] <label> <command> [args…]`. With the flag, exit 2
becomes a **skip**: `run_checker` prints a loud notice, returns 0 so the call
site keeps its shape, and sets `RUN_CHECKER_SKIPPED` (and
`RUN_CHECKER_SKIP_REASON`) so a caller tallying "N gates ran" can say "and M
skipped" truthfully. Without the flag, exit 2 aborts exactly as before.

Three further conditions disqualify a skip even where the flag is present:
a Python traceback, an argparse `usage:` banner, or no output at all.

### The alternatives

**Make exit 2 a skip globally.** Simplest, and wrong. Exit 2 is also what a
*floor* returns — "I inspected 3 files and this run must inspect at least 40",
the check that stops a scanner which has silently stopped scanning from
reporting a clean tree. For a floor, aborting is the entire point. A global
loosening would convert every floor in the tree into a shrug, and floors are a
newer and more fragile defence than the thing being loosened for. Lane A made
this point explicitly when requesting the feature — *"For a floor, that
behaviour is right and I do not want it loosened"* — and it is the reason the
channel is opt-in per call site rather than a policy change.

**Leave the five pinned and run them by hand.** The status quo, and it is not
stable: "run it by hand when you change a quoting rule" degrades to "never" the
moment the person who wrote the rule is not the person changing it. The four
bash oracles had already reached that state.

**Have skipping checkers exit 0 with a warning instead.** Requires no
`run_checker` change at all — and makes a skip indistinguishable from a pass at
the one place that matters, the exit status. Every summary in the tree would
count it as a gate that ran.

### Why a skip must be spoken

The three disqualifiers are not defensive padding; each closes a way for the
flag to become a bypass.

- **A traceback.** Without this, adding `--may-skip` to a call site would also
  silence that gate's *crashes*, which is precisely the confusion this whole
  file was written to end.
- **An argparse `usage:` banner.** This is the one that makes the flag nearly a
  footgun, and it is not obvious: **`argparse` exits 2 for a usage error.** So a
  wired call site whose invocation later grew a typo — a renamed flag, an
  argument no longer accepted — would exit 2, print, and be read as a
  legitimate decline. That gate would then skip on *every* host, forever,
  reporting argparse's complaint as its reason, and nothing else in the run
  would say otherwise. Caught by reading the failure paths while writing the
  feature, not by a test that thought of it afterwards.
- **No output.** A skip is a claim — "I could not answer, and here is why". A
  claim with no evidence is exactly what a gate that has quietly stopped
  working looks like. So a silent exit 2 is never a skip, flag or no flag.

### The cost, stated plainly

`--may-skip` is a way for a check not to run, and every such mechanism is one
step from a check that never runs. What keeps it honest is that the *call site*
must assert the right — a reviewer sees the flag in the diff — and that the
skip is loud in the transcript and counted separately in the tally. What it
does not do is prove the gate ever runs *anywhere*: a gate wired with
`--may-skip` whose tool is missing on every developer's machine is back to
being an unrun gate, with the ratchet no longer reporting it as one, because it
is now wired. That is a real regression in visibility and is not solved here.
The mitigation, when the four bash oracles are wired, is that this dev host
does have WSL — but a mitigation is not a guarantee, and the honest next step
is a tally of gates that skipped on *every* recent run.

### Evidence

Six mutations of the implementation were run against the suite; five were
caught immediately. The sixth — deleting the traceback guard from the skip arm
— **survived**, and the reason was a defect in the test rather than in the
code: the crash fixture raised out of a failed `open`, which exits 1, so it was
caught by the older exit-1 traceback rule and never reached the skip arm at
all. The assertion passed while the guard it existed to pin was deleted. Fixed
by making the fixture exit 2 (a checker that catches its own exception, prints
the traceback, and reports no-verdict), plus an assertion that the fixture
really does exit 2 — because a fixture that stops reproducing the shape it was
written for is the same class of silent rot as the gates this file grades.
