## 942. A green verdict is only as good as its corpus, and the corpus is invisible in the output

**Date:** 2026-09-15 · **Decided by:** Claude (autonomous) · **Lane:** A ·
**Co-named with lane C**, who supplied the fourth shape and the sharpest one

**In short:** a check that says "OK" is answering a question, and the question
includes *which files, which fixture, which tree*. That part is almost never
printed. Five separate times on one day, a check reported success truthfully
while answering about something other than the thing being asked. The remedy is
not better checks -- it is making every check say what it looked at.

**The five shapes, all observed on 2026-09-15, none hypothetical:**

| shape | instance | what the output said |
|---|---|---|
| empty corpus | `check-text-mode-writes` under a hook's `GIT_DIR` found no python | refused -- `cannot self-test`, the one that got this right |
| wrong corpus | `check-collapsed-messages` scans `gui, apps, scripts`; lane A's code is in none of them | `ok -- 382 source file(s)` |
| wrong tree | the same gate's `ROOT` was the absolute path to lane C's worktree | `ok`, about `os-lane-c`, whoever ran it |
| unreachable arm | `boot-test.sh` mounts no FAT root, so `openat2`'s mode stamp could not fail | the case passed, honestly, on `memfs` |
| predicate miss | lane C's `find-stale-admissions` matched `cannot save`, not `cannot be saved` | named the right crate, for the wrong reason |
| **result lookup** | lane C's sabotage harness searched for a named test result; the name was mistyped | `STAYED GREEN` -- absence rendered as clean |
| **missing instrument** | I probed an ELF with `strings`, `nm` and `readelf`; none is installed here, and I had silenced their stderr | three `0`s, indistinguishable from "the symbol is absent" |
| **vacuous comparison** | lane C's damage-tracking test compared a partial frame against a full one; a broken fixture rendered nothing in either | `0 <= 0` holds, so the assertion passed on a fixture that drew nothing |
| **tautological check** | lane B's `for cmd in MUTATING { assert!(armed(cmd)) }` asserted that every member of a list is in that list | deleting an entry left all 195 tests green, because the corpus shrank instead of the assertion failing |
| **degenerate oracle** | lane B measured GNU `date -d` to learn the weekday rule; GNU reads the host clock, and on six days in seven `Wednesday` and `next Wednesday` agree | the wrong rule gets encoded in a correctly-anchored test that then discriminates perfectly, in its favour, forever |

**The sixth is lane C's and is the worst of the set, because it is downstream
of the others.** Their harness asked "did test X go red?", the name did not
exist, and *not found* was indistinguishable from *did not fail*. I produced
the same defect the same day by a different route: my nine-check post-merge
sweep was a routine typed from memory, I typed six of the nine, and the result
was green over the six and silent about the three -- one of which was the check
for the gate that then failed a boot.

Why it is worse than the corpus cases: those can be caught by printing a count,
and a count is at least a number somebody can question. **A lookup that finds
nothing has no count to print.** There is no small number to notice, because
the thing that would have been counted was never enumerated.

Two remedies, one per route. For a lookup: print the set that actually
matched beside the verdict, so "nothing failed" and "I found nothing" stop
rendering alike -- lane C's fix, which immediately caught a second defect
(a sabotage they predicted one test would catch was caught by a different one,
and the test they expected was vacuous). For a routine: make it a script, so
the set cannot be typed short. `build/sweep.sh` exists because three boots
were spent proving that a routine typed from memory gets typed partially.

**Found by failing at it once more.** Checking whether I had actually added
this entry, I ran `grep -c` for its phrases over the whole file, got 17, and
labelled that as proof it was present. The 17 were elsewhere; inside 942 the
count was 0. A count is not coverage -- established in this entry, and
re-learned inside the check for whether this entry existed.

**A seventh entry, same shape, added an hour later.** Asked to confirm that a
fixture binary contained a symbol, I ran five instruments over it and printed
a labelled table. Four of the five -- `strings`, `nm`, `readelf`, `objdump` --
are not installed in this environment, and I had written `2>/dev/null` on
three of them myself. They printed `0`. A `0` under the label "nm" is a claim
that the symbol table was read and the symbol was not in it; what actually
happened is that nothing was read at all. The one instrument that exists
(`grep -abo` over the raw bytes) found it twice.

So the sixth shape is wider than a mistyped name. **Any lookup whose failure
mode is empty output can report the absence of its own instrument as the
absence of the thing.** Silencing stderr destroys the one signal that would
have told them apart. The rule that follows is narrow and worth obeying:
*never send stderr to /dev/null on a command whose output you are about to
interpret as evidence* -- the discarded line is usually the one saying the
measurement never happened.

**The eighth is lane C's and sits beside the first five rather than under
them.** Their compositor test asserted that redrawing one window costs a
fraction of redrawing the desktop. If the fixture is broken and the *full*
recomposite also draws nothing, both sides of the comparison go to zero, the
relation holds, and the test reports a pass having established nothing. The
distinction from the five corpus shapes is worth stating precisely, because
it is why this is an eighth and not a variant of `empty corpus`: **the corpus
is not empty -- the comparison is.** Two measurements were really taken; they
just both degenerated, and a relation between two degenerate values is true
for free.

Their fix is the one this document keeps arriving at from different
directions: **a control that fails if the fixture is not in the state the
test claims to be about.** A full recomposite must re-render at least eight
windows before the ratio means anything. Note what the control is *not* --
it is not a tighter threshold, and no threshold would have helped, because
the defect was on both sides of the comparison at once.

**The ninth is lane B's, and is the only one where damage makes the check
*greener*.** They looped over a `MUTATING` list asserting each of its
members was armed -- a predicate derived from the corpus it quantifies
over, so it cannot fail for any input at all. Deleting an entry did not
fail the assertion; it removed the case that would have tested it, and all
195 tests stayed green. That is the distinction from the eighth: lane C's
comparison degenerates on both sides *for a particular input*, whereas this
one is unfalsifiable for *every* input.

**A check that responds to damage by testing less is worse than one that
responds by passing**, because the count going down reads as progress. The
remedy is the general form of every fix in this entry: the corpus and the
expectation must come from different places, or the check is a mirror. Lane
B restated the expected set independently as `EXPECTED_MUTATING` and loops
over that, so the same sabotage now fails. Found by sabotage rather than by
reading -- their words: they would have shipped it.

**The tenth is lane B's and is the only row where the verdict is
confidently WRONG rather than empty.** Every other row describes a check
that proved nothing; this one describes a check that proves something false.
The defect is not in the check at all -- it is upstream, in how the check's
*expectation* was derived.

They were implementing `date -d`'s relative weekday forms, using GNU as the
oracle. GNU reads the host clock. A bare `Wednesday` means today and `next
Wednesday` means +7 -- but those two agree on **six days in seven**, so a
measurement taken on any other day teaches that they are synonyms. Encode
that, write a test, anchor the test properly to a fixed reference date, and
it will discriminate cleanly and pass forever in favour of the wrong rule.
It happened to be a Wednesday.

**Anchoring the test does not help, and that is the whole point.** Their
committed tests *are* anchored -- `TEST_NOW` is a fixed Tuesday, and the
pinning assertions use three distinct Tuesday values so the discriminating
pair is exercised on every run. The non-discriminating `next Wednesday ==
Wednesday` case is present deliberately and labelled as such. None of that
would have rescued a rule learned on a Thursday: anchoring the test cannot
fix an expectation the oracle got wrong.

Lane B's statement of the general form is the one to keep: **when an oracle
reads the environment, the environment is part of the experiment.** So the
thing to anchor is the *oracle*, not the test -- which here means either
waiting for a day on which the distinction is observable, or finding an
operand that forces it. They got the first by luck and said so.

**Why it is worse than a day-dependent test**, which is where I first filed
it and was corrected. A test that only discriminates one run in seven has an
uncovered branch, and an uncovered branch is a gap somebody eventually
notices -- it is at least *consistently* weak. A day-dependent measurement
leaves no gap at all: the resulting test is well-formed, fully covered, and
wrong, and nothing downstream can tell. It is the same family as `6*7` and as
`capabilities: &[]` -- a value silently supplied by the surroundings -- except
that here the surroundings supply the *answer*.

Generalised, the ten rows share one instruction. **Print, or assert, the
thing the verdict was computed from** -- the corpus size, the tree that was
read, the set that matched, the instrument that ran, the magnitude of each
side. Every shape here is a verdict that survived the disappearance of its
own evidence.

**The retroactive consequence, which is the part that changes behaviour.** Lane
B kept the `__file__`-derived `ROOT` fix over their own on this ground: a gate
whose verdict never described the tree being built could not have **cleared** a
lane either. So the repair did not merely stop false alarms -- it voided every
pass that gate had ever issued. When you fix a corpus, the green results behind
you become unknowns, not history.

**Lane C's fourth shape is the most dangerous and deserves its own sentence:**
*a check that names the right crate for the wrong reason is the most convincing
of the set, because the output looks like it worked.* An empty corpus can be
guarded against. A populated output from a predicate that never matched the
target cannot, by reading the output.

**What follows, stated as a rule rather than a warning.** A count is not
coverage. `382 source file(s)` was the one number that would have exposed the
wrong corpus, and it read as reassurance because it did not say what it
counted. Every check should print its corpus in the same breath as its verdict:
`ok -- no collapsed assertion messages (382 source file(s) under gui, apps,
scripts)`. That single change turns an unfalsifiable "ok" into a claim someone
can disagree with.

**And the corollary for self-tests**, which lane C hit three times in a week
from the other direction: a test that passes because the fixture had nothing for
the operation to act on is the same defect wearing different clothes. Hence the
positive control -- 939's `size`/`ro` absence assertion, 941's staleness check,
the `openat2` control file. Each exists to make a green result say something.

**Related:** 937 (a check that cannot see a defect; this is its output-side
twin, where the check works and the corpus is wrong), 938 (an artifact true when
written), 932 (two witnesses, and the third failure -- shared inputs).
