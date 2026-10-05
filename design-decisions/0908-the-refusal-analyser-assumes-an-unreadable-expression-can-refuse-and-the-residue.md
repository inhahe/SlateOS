## §908 — The refusal analyser assumes an unreadable expression can refuse, and the residue is documented rather than reported

**Date:** 2026-09-03. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** `check-gates-can-refuse.py` reads each checking program's source and
asks whether it could ever exit non-zero — a gate that physically cannot fail is
worse than no gate, because it emits a reassuring line of output on every build.
It was missing most of them: it cleared any file that used `return _decline(...)`,
which is the commonest way a gate refuses in this tree. Fixing that took
detection from 12 of 47 to 37 of 47. This entry records how that was measured,
and the deliberate decision *not* to close the remaining ten by reporting
everything the analyser cannot read.

### How it was measured, because a checker's own "ok" is not evidence

The gate printed "ok — all 47 gates can reach a non-zero exit bare" both before
and after the fix. That sentence is worth nothing by itself; it is the sentence a
broken gate prints too.

The unit of evidence is a **mutation census**: neuter every subject mechanically
— rewrite its verdict so it can only return 0 — and count how many the gate
catches.

| | Detected |
|---|---|
| Before | **12 / 47** |
| After | **37 / 47** |

Zero regressions: everything caught before is still caught.

### The false negative: "delegated, so assume it can refuse"

`_could_be_nonzero` treated a call as opaque and answered `True` — the
conservative answer, on the reasoning that the callee is unknown. But the callee
is usually thirty lines up in the same file. `return _decline(reason, detail)` is
*the* refusal idiom here, and it cleared every file it appeared in, whether
`_decline` returned 2 or returned 0.

The fix resolves module-local calls: `audit()` builds `{name: FunctionDef}` for
the module, and `_value_could_be_nonzero` recurses into a called function's own
returns, with a `seen` set to bound recursion. **Conservatism is not conservative
when the answer is in the file you have already parsed.**

The self-test's decisive pair is two fixtures that are identical at the call site
and opposite in fact: a `_decline` helper that returns 2 (must clear the gate)
and one that returns 0 (must be reported). Nothing at `return _decline(...)`
distinguishes them; only following the call does. Thirteen fixtures now, up from
eight.

### Decision: leave the remaining ten to `known-issues.md` rather than reporting them

The tempting close is to invert the default and report anything the analyser
cannot *prove* refuses. That would reach 47/47 by construction.

| | *What changes:* | Cost |
|---|---|---|
| Report the unanalysable (rejected) | ten gates are flagged on every build until each is rewritten into a shape the analyser reads | a per-build gate that cries wolf gets ignored — and then it has also stopped working for the 37 cases where it is right |
| Assume it can refuse (**chosen**) | the gate stays silent on shapes it cannot read; the residue is tracked in prose | ten known false negatives, invisible to the build |

The decisive consideration is that this gate's output is one line inside a sweep
of forty. Its value is entirely that the line is *trustworthy*. A gate that
reports ten standing non-problems trains its reader to skim it, and the failure
mode of a skimmed gate is that it gets skimmed on the day it is right.

So the residue is written down with its measured shapes rather than guessed
causes, in `known-issues.md` →
`TD-A-A-GATE-THAT-CANNOT-REFUSE-IS-STILL-UNDETECTED-IN-10-OF-47-GATES`:

| Shape | Gates |
|---|---|
| verdict is a returned local variable | `check-evdev-elf-asm.py`, `check-query-status.py`, `check-usage-status.py` |
| verdict is a returned comparison / comprehension / subscript | `check-option-refusal.py`, `scan-orphan-modules.py` |
| a call whose callee contains an unanalysable expression | `argv-utf8.py`, `check-selftest-reinit.py`, `host-errmsg.py`, `raced-globals.py` |
| no `main()` at all | `rustscan.py` |

The first two rows are one improvement away — single-assignment local dataflow,
which is bounded work and is on the list. The last row is a defect in the
subject, not in the analyser.

### Reversing this

Drop `funcs` from the three `_could_be_nonzero` call sites and the analyser
returns to treating every call as opaque, i.e. to 12/47. The signal that the
*default* should flip is the residue growing rather than shrinking: if new gates
keep landing in shapes the analyser cannot read, then those shapes are the
convention and the analyser is the outlier.
