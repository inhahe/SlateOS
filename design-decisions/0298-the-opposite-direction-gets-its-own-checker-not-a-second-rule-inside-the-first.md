## §298 — The opposite direction gets its own checker, not a second rule inside the first

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** the shell has a build-time check that a command which prints an
error message must also tell the caller it failed. Applied to a command that
was *answering a question* rather than complaining, that rule produces the
opposite bug — a correct answer reported as a failure — and it did, twice, in
August. This decision adds a second checker for that opposite direction and
keeps it in a separate file rather than folding it into the first, so the two
rules can disagree out loud instead of silently.

**The two rules.** `check-usage-status.py`: *if a block prints a usage message
because it could not do what it was asked, it must set a non-zero status.*
`check-query-status.py`: *if a block can only be reached by the user asking —
no argument was given — and answers by printing program state, it must not.*
They are not complements of one another; they are two rules over overlapping
sets, and the overlap is where the bugs live.

**Why not one script.** A single classifier would have to hold "this needs a
status" and "this must not have one" together and decide which applies. It
would therefore have to *resolve* the ambiguous case, and it would resolve it
in whichever direction the code happened to be written — silently, with no
output, because agreeing with the tree is what a passing check looks like. Two
scripts cannot do that. A site the author has got wrong trips one of them; a
site that is genuinely ambiguous trips *both*, and there is no way to make both
green without writing down which reading is intended and why.

That is not hypothetical: it is what happened to `quota` an hour after the
second checker was written. Taking the failure status off bare `quota` (the
query reading) made it trip the usage-status gate, which is how its `ALLOWED`
entry came to exist with a reason attached instead of a status being flipped
back and forth by whichever sweep ran last.

| Option | Against |
|---|---|
| One script, two rules, internal precedence | Resolves the ambiguous case by fiat and prints nothing when it does. The precedence order becomes an undocumented policy. |
| One script, report sites matching *either* | The two messages mean opposite things; a combined report is a list the reader has to re-classify by hand, every run. |
| **Two scripts, both run, both must be green** | Two files and two `ALLOWED` lists to keep. Accepted: the duplication is small and the ambiguity is surfaced rather than swallowed. |
| No second checker; rely on review | Already tried. `elog echo` and `fc algo` shipped the bug for a month and were found by a targeted search, not by reading. |

**The checker is verified against a revision that had the bug.** Run on
`9251e5a3d^` it reports `cmd_fcompress` and `cmd_elog`, the two sites that
actually shipped; run on the tree today it reports only the new find. The
invocation is in the script's docstring, because a checker nobody has watched
fail is a checker nobody knows works — the same reason `check-usage-status.py`
takes an optional path argument.

**One implementation note worth keeping.** Braces are counted a character at a
time, not a line at a time. `} else {` closes and opens on one line, so a
line-granular depth count nets to zero and the walk sails past the brace that
actually delimits the block — which makes a sibling `else` branch look like the
guarded one. That single error accounted for 15 of the survey's first 18
candidates, all of them `cmd_ulimit`-shaped: a genuine query branch next door to
a genuine error branch, reported as one block.

**And what it exposed, which is the larger point.** Fixing `quota`'s query half
put the reader one screen away from its catch-all arm, which printed
`"Unknown subcommand '{}'. Use: …"` with no status at all — the *first* rule's
bug, in a command the first rule's gate had checked and passed. It passed
because the gate finds a diagnostic by looking for the word `Usage:`, and that
arm says `Use:`. Widening the trigger reports 49 more such sites. A gate's
trigger is part of its rule; a trigger derived from the wording of the last bug
is a syntactic sweep wearing a semantic hat, which is §296's lesson turning up
one level below where §296 left it. Tracked as
`A-KSHELL-DIAGNOSTICS-NOT-WORDED-USAGE-ESCAPE-THE-GATE`.
