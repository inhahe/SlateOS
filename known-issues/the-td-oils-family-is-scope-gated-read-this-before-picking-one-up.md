## ⛔ The `TD-OILS-*` family is scope-gated — read this before picking one up

**design-decisions.md §305 (operator, 2026-08-14) froze osh's bash-fidelity
scope.** There are ~325 open `TD-OILS-*` entries in this file. **They are not a
backlog to burn down**, and the general "fix bugs immediately" rule above does
**not** apply to them by default.

GNU bash 5.2 itself cross-compiles and runs on SlateOS (since 2026-07-22 —
`scripts/bash-spike/`, `kernel/src/proc/spawn.rs::self_test_bash_on_slateos_libc`),
so byte-for-byte osh↔bash parity stopped being a goal. **Fix an osh divergence
only if:**

- something we actually ship or run hits it; **or**
- it is a crash, hang, data-loss, security, or wrong-exit-status-that-propagates
  bug — i.e. a bug on its own terms, independent of bash; **or**
- it is a regression against an already-green corpus case.

**Do not fix, and do not add a corpus case for:** diagnostic wording or the exact
substring a message echoes; artifacts of bash being a 40-year-old C program
(`OPTIND=4294967297` wrapping through an `int`); constructs reachable only by
adversarial input whose only observable difference is the error text.

When a divergence is real but out of scope, annotate its entry
`SCOPE: out of frozen scope (§305)` and leave it unfixed. If something genuinely
needs exact bash, **run bash**. Read §305 in full before doing any osh parity
work — it also records *why* this cap exists, which is a 25-day misdirection
worth not repeating.
