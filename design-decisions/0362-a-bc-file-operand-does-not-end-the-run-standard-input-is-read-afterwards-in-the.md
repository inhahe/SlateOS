## §362 — A `bc` file operand does not end the run: standard input is read afterwards, in the same interpreter

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Where it bites:** `userspace/coreutils/src/bin/bc.rs`, `main`.

**In short:** When you run `bc script.bc`, does bc stop after the script, or
does it then start reading what you type? Ours used to stop. GNU does not —
it runs the script and then carries on reading, using the same variables and
functions the script defined. Measured, not recalled:
`printf '9+9\n' | bc a.bc` where `a.bc` holds `1+1` prints `2` and then `18`.
Ours now matches.

**Why this is a decision and not just a bug fix.** The two behaviours are both
defensible, and the one we had is what a person would design from scratch:
"you named a file, I ran it, we are done." GNU's is the one that makes
`bc mylib.bc` a *useful* command — it loads a library of functions and hands
you a calculator that knows them. Given the choice, matching the reference
implementation wins, because the whole point of shipping something called `bc`
is that scripts and habits carry over.

**The cost, stated plainly.** `bc script.bc` from a terminal now leaves you at
a prompt rather than returning to the shell. That surprises anyone who expects
the old behaviour, and it is exactly what GNU does. A script that wants the
old behaviour writes `bc script.bc </dev/null`, which is also what it would
write on a GNU host.

**Our one deviation, and why.** `-e EXPR` — which GNU bc does **not** have at
all; `bc -e 2+2` upstream answers `invalid option -- 'e'` — does end the run.
`bc -e '2+2'` dropping into an interactive session is nobody's behaviour, and
since the flag is ours we are free to define it. So the rule is: standard
input is read unless an explicit expression was given. Both halves are
covered by tests in `bc.rs` that name the GNU command establishing them.
