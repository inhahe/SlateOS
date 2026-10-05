## 754. An unrun self-test is graded apart from an unrun gate, and gets no exemption list — because the excuse that justifies one cannot exist for the other

**Date:** 2026-09-03
**Lane:** B
**Decided by:** Claude (autonomous), on lane A's request
(`requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md` §4)

**In short:** `scripts/check-gates-are-wired.py` is the script that notices when
one of our ~36 automated checks has quietly stopped being run by anything. It
used to report two different problems in one undifferentiated list: "nothing
runs this check at all", and "this check carries its own little test suite and
nothing runs *that*". Those cost different amounts to fix — the first can
require another team's agreement, the second never can — so they are now printed
under separate headings with separate counts. And the second one deliberately
gets no "we know, leave it alone" exemption list, even though the first one has
one.

### The situation

The checker already had three arms: a gate nothing runs; a `PINNED` exemption
that has gone stale (the file is gone, or something runs it now); and a wired
gate whose own `--self-test` nothing executes. All three landed in one
`findings` list, printed as one block, counted as one number.

Lane A pointed out the asymmetry that makes that wrong, and stated it better
than I would have:

> Running a gate's `--self-test` is always cross-lane safe; running the gate is
> not. A self-test reads fixtures the checker carries in its own source. It
> cannot fail because of anything in anyone's tree, so wiring it can never turn
> another lane's boot test red. The real check can.

That is a fact about what a self-test *reads*, not a courtesy. It means "your
self-test is not run" is always fixable on the spot by whoever noticed, and
"your gate is not run" may need the owning lane to agree first.

### The decision

Two parts.

**(a) Grade them apart, in the report, not just in the code.** `audit()` returns
`(findings, selftest_findings, notes)` and `main()` prints two headings, each of
which *states its own reason* rather than referring to one. Someone looking at a
red build has not read lane A's request and will not go looking for the rule; if
the report does not say which half is theirs to fix without asking, the
distinction exists only for the two lanes that discussed it.

Both still exit 1. The two defects are differently **actionable**, not
differently **serious** — an unrun self-test is arguably the worse of the two,
since nobody's permission is needed and it has been left anyway.

**(b) No `PINNED` equivalent for the self-test arm.** This is the part with a
real tradeoff.

`PINNED` earns its keep because wiring a gate can legitimately be the wrong
thing to do: the four bash oracles (§753, and
`TD-B-THE-FOUR-BASH-ORACLES-ARE-PINNED-NOT-WIRED`) shell out to a real `bash`
via WSL, and wiring them would make WSL a hard requirement of every lane's
build. That is a genuine excuse, it is written down per-gate, and the ratchet
fails when it goes stale.

There is no matching excuse on the self-test side — *because* lane A's rule
holds. A list of exemptions for a defect that has no legitimate excuse is not an
exemption list; it is a queue nobody drains. Worse, it converts "any lane can
fix this unilaterally" into "any lane can defer this unilaterally", which is the
opposite of what the split is for.

**Alternative considered: add `SELFTEST_PINNED` anyway, for symmetry.** The
argument for it is that some future self-test may be genuinely expensive to run
(a fixture that needs a network, a several-minute suite), and without a pin the
only way to silence it is to delete the `--self-test` flag from the gate — which
is a strictly worse outcome than an honest exemption. That is a real risk and I
am not dismissing it. I rejected it on the grounds that it is speculative today
(no self-test in the tree takes more than a second, because they all run off
in-source fixtures by construction), and that the cost of adding the list later
is one small commit whereas the cost of having it now is that the three gates
lane A named in §4 acquire a legitimate-looking place to sit forever. If a
genuinely expensive self-test appears, add the list then, with that gate as its
first entry and its real reason attached — which is exactly how `PINNED` should
have been introduced and was.

### What this does not fix, stated plainly

The ratchet is a *static reader of scripts*. Every question it can answer is a
question about the text of `boot-test.sh` and of the gates. "Is there a
`run_checker <g>-selftest` call?" is such a question. "Did that call actually
run, or did it decline via `--may-skip` on every host?" is not — that is a fact
about a run, and no reading of the file produces it.

Lane A asked whether that belonged in this arm or a fourth one. Neither: adding
it here would mean the ratchet answers a question it cannot see, which is the
precise failure mode lane A's §2 is about, wearing the ratchet's clothes. The
evidence exists per-run (`RUN_CHECKER_SKIPPED`, and the spoken `SKIPPED` line
from §753); what is missing is anything that accumulates it *across* runs, which
needs a record of past runs that this tree does not have. Left open in
`TD-B-TEN-GATES-ARE-NEVER-ASKED` rather than half-built.

So after §753 and this entry: a gate can no longer be silently unwired, and a
self-test can no longer be silently unrun, but a wired gate can still be
silently declining on every host. That last one is narrower than it was — it now
requires someone to have written `--may-skip` at the call site on purpose — but
it is not closed.

### Evidence

Selftest 29/29 (was 20/20). Seven mutations run against the new split, seven
caught. The two that matter are the two directions of "report into the other
list": a mutation appending to *both* lists satisfies the positive assertion
("a wired gate whose self-test nothing runs must be reported") while destroying
the entire point, so the **absence** assertion is the load-bearing one and is
written as such.

One mutation initially survived — a heading printed over an empty group — and it
exposed a real hole rather than a weak mutant: no case asserted what a *clean*
run prints. A report that announces "self-test not run" with nothing under it is
the same noise this checker refuses to produce elsewhere. There is a case for it
now, in both directions.
