## B-GATE-11-DIED-ON-ITS-OWN-ARGUMENT-LIST-AND-BLOCKED-EVERY-PUSH (lane B)

**Status: FIXED 2026-09-02** (lane B). Found by it happening: a push of 84
commits was refused, and not by a finding.

**In short:** the push hook scopes its dead-doc-link check to the crates the
push touches, and it named those crates by listing them on the command line.
A command line has a maximum length. A push carrying 2,568 changed `.rs`
files names 2,568 directories — 64,862 bytes, against the 32,767-character
limit Windows enforces — so the shell answered "Argument list too long", the
checker exited 126 without opening a single file, and the push was refused.
The refusal was correct behaviour and that is the point: an exit code that is
not a verdict must never be read as a pass. The effect was a lane that could
not push at all, for a reason that had nothing to do with the code it was
pushing.

**What made it invisible until it fired.** The bug is not in the checker and
not in the hook's logic; it is in the seam between them — how the scope is
handed over — and it only exists above a scale nothing had reached. The
hook's shape suite (`test-pre-push-gates.py`) checks structure, and this was
structurally fine. The checker's `--selftest` checks its parsing rules, and
its parsing was fine. Neither could have seen it. What made the scale
reachable was the 2,286-crate honesty sweep (`935c200a5`), which multiplied
the size of a normal lane-B push by two orders of magnitude in one commit.

**The fix (this commit).** The list goes in a file, which has no such ceiling,
and `check-doc-links.py` grows `--paths-from FILE` (`-` for stdin) to read it.

Batching the arguments was the obvious alternative and is worse. The verdict
would still be right — crates are scanned independently and the findings are
a union — but every batch repeats the `rglob("Cargo.toml")` crate discovery
over four whole roots, so forty batches is minutes of work spent avoiding a
limit that a file simply does not have.

An empty `--paths-from` is a hard error (exit 2), not a pass and not a
whole-tree sweep. Both silent readings were available and both are bad: "scan
everything" widens the gate onto files the pusher cannot fix, which is how a
gate comes to be bypassed by habit, and "scan nothing" means a bug that
empties the list switches the check off leaving no trace.

**Regression coverage.** `scripts/test-pre-push-doclinks-gate.py` (new) builds
a fixture with enough crates to exceed the limit and asserts the gate reaches
a verdict — in **both** directions on that same fixture, because "the big
clean push was allowed" is also exactly what a gate that silently skipped
would produce. The second push buries one dead link in the middle of the list
and must be refused. It also covers the small-input equivalence of
`--paths-from` and positional arguments, so the flag cannot quietly come to
mean something different from what it replaced.
`test-pre-push-gates.py` gains the cheap shape assertion that the scope is
read from a file, which catches a reintroduction without needing the
several-hundred-crate fixture.

**Gates that do *not* have this problem, checked rather than assumed.** Gate
5's `$bins` is bounded by `userspace/coreutils/src/bin/` — about 124 short
names, which cannot approach the limit however wide the push. Gate 7 already
writes its file list to a file and already batches its rustfmt calls at 64.
Every other gate passes a fixed number of arguments.

**Related, not fixed here:** `scripts/run-checker.sh` reports a non-zero exit
by suggesting the usual local cause ("a second rustc-heavy job running
concurrently and exhausting memory"). That advice is right for the failures
it was written for and was actively misleading here — exit 126 with
"Argument list too long" is not a resource problem, and following the advice
would have meant re-running an 16-minute push to reach the same wall. Filed
as its own item below.
