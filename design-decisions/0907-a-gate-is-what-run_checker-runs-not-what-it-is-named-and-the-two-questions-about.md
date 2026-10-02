## §907 — A gate is what `run_checker` runs, not what it is named — and the two questions about a gate need two different definitions

**Date:** 2026-09-03. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** Two of the checking programs in `scripts/` exist to grade the
*other* checking programs. One asks "is this gate actually wired into a build,
and is its self-test actually run?"; the other asks "if this gate found
something, could it even fail?". Both decided what counted as a gate by looking
at the **filename**. Nine real gates are spelled differently — `scan-unwrap.py`,
`rustscan.py`, and six run only by the pre-push hook — so both meta-gates skipped
them entirely and reported "ok". The fix is to define a gate by what a caller
actually runs. This entry records why the two questions nevertheless still need
two *different* definitions, which is the part that is easy to get wrong twice.

### The failure, stated generally

**A gate that discovers nothing reports no failures, and that reads exactly like
a pass.** This shape turned up three times in one work session, at three
different levels — a gate's subject set, a meta-gate's corpus, and an analyser's
expression coverage. The meta-gate form is the nastiest, because a corpus that
*omits* a file reports "ok" in the same words as one that includes it and finds
nothing wrong. No wording, no exit code and no log line distinguishes them. Only
counting the corpus does.

The concrete symptom was a sentence `check-gates-are-wired.py` printed with total
confidence:

```
0 self-test(s) shipped but unrun
```

True of the subset it looked at. Printed as though it were true of the tree.

### The mistake was made twice in one expression

`analyse()` collected call sites out of `scripts/boot-test.sh` and
`scripts/hooks/pre-push`, reading the *actual* `run_checker` invocations — and
then filtered the result through `if _GATE.fullmatch(n)`, a regex over the
filename. Having done the hard and correct thing, it discarded the answer for
anything that did not look the way a gate is supposed to look. The file's own
docstring lists "matching filenames" as method 4, a known-bad approach, in the
course of explaining why it does not use it.

### Decision: define "a gate" as "a script some caller passes to `run_checker`"

`scripts = list(literal or named)` — no name filter. The corpus is the union,
over `CALLERS` (`scripts/boot-test.sh`, `scripts/hooks/pre-push`), of every
script run through `run_checker`; plus, for the wiring question only, the
filesystem glob.

Measured effect:

| | Before | After |
|---|---|---|
| `boot-test.sh` — gates run / self-tests run | 36 / 23 | **38 / 25** |
| `pre-push` — gates run / self-tests run | 2 / 2 | **8 / 7** |
| `check-gates-can-refuse.py` corpus | 38 | **47** |

The nine that had been exempt purely by spelling: `scan-unwrap.py`,
`scan-orphan-modules.py`, `rustscan.py`, and — all six from the pre-push hook —
`argv-utf8.py`, `getopt-ambiguity-check.py`, `host-errmsg.py`,
`multicall-aliases.py`, `quote-names.py`, `raced-globals.py`.

### The subtle part: the two questions cannot share one definition

Collapsing them is what produced the bug, and "just use the call sites
everywhere" would produce its mirror image.

| Question | Must be answered from | Because |
|---|---|---|
| *Is this gate wired at all?* | the **filesystem** — i.e. names | an unwired gate has **no call site to read**. Defining the corpus as "what callers run" makes the answer vacuously yes, always. |
| *Is its self-test run? Can it refuse?* | the **call sites** — i.e. what is invoked | a gate's spelling is a habit; the build obeys the call, not the convention |

So `check-gates-are-wired.py` keeps a name-shaped constant — renamed `_GATE_NAME`
precisely so the next reader cannot mistake it for a definition of "gate" — and
uses it *only* to enumerate candidates on disk. Everything downstream of that
uses call sites. The self-test arm's subject became `sorted(set(gates) | wired)`:
the union, because a script that is run but does not match the name is exactly
the case that was being missed.

### Decision: when the corpus cannot be determined, decline — do not fall back to the glob

`check-gates-can-refuse.py` now imports the sibling parser rather than copying
it, and if `scripts/boot-test.sh` or the pre-push hook is missing it returns a
`why_not` and `main()` exits **2**.

The rejected alternative was to fall back to the directory glob, on the grounds
that it "still checks something". That is the identical defect one level up: a
strictly smaller corpus, reported in identical words. Exit 2 is this tree's
single code for "I did not reach a verdict" (§905), and this is precisely that
situation.

One implementation note worth keeping, because the self-test caught it: the
sibling parser must be located via `Path(__file__).resolve().parent` and **not**
the module-global `SCRIPTS`, which the self-test deliberately repoints at a
temporary directory. Using `SCRIPTS` made the gate silently analyse a fixture
directory instead of the real tree. That the self-test caught it is the argument
for its asserting the contract by *calling* `main()` rather than by reading the
source.

### Reversing this

Restore `if _GATE_NAME.fullmatch(n)` in `analyse()` and the counts return to
36/23 and 2/2. There is no reason to; but the signal that the *definition* is
wrong would be a script that `run_checker` runs which is genuinely not a gate — at
which point the fix is an explicit exemption carrying a reason, in the style of
the existing unwired list, and never a return to grading by filename.
