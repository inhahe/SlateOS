## §309 — Byte-fidelity with bash has an "unless it is a defect" clause; osh does not reproduce the null array element

**Date:** 2026-08-15
**Decided by:** Operator (Claude recommended this option — open-questions.md Q40 option B)

**The question.** osh is held to byte-fidelity with bash 5.2.37. One measured
bash behaviour, reachable only through a nameref, stores a **null pointer** into
an array element (`n=(a b c); declare -n q='n[1]'; declare q`), after which the
array reads as empty while its elements are demonstrably still present. It looks
like a defect rather than a design: bash cannot describe the resulting state
with any of its own printers, no bash-level operation other than this one can
produce it, and the bind carries `ASS_FORCE` so it also silently defeats
`readonly`.

**Decision: do not reproduce it (option B).** osh keeps `Str` array elements and
the array reads normally. The divergence is waived in the corpus and the full
write-up stays in `known-issues.md` →
`TD-OILS-A-DECLARATION-WITH-NOTHING-TO-DO-BINDS-A-NULL-THROUGH-THE-REFERENCE`,
so the decision is reversible if a real script is ever found that depends on it.

**What this actually settles — the precedent, not the bug.** The narrow
cost/benefit was never close: option A wanted every array reader in `interp.rs`
rewritten around an `Option<Str>` element type, permanently, to preserve a state
no bash-level operation can otherwise produce. The reason it needed the operator
is that it establishes **whether byte-fidelity has an "unless it's a bug" clause
at all**. It now does. Consequences:

- "The measurement wins" is no longer absolute. A measured bash behaviour may be
  waived when it is (i) unreachable except through a construct built to reach
  it, (ii) inconsistent with bash's own observable model, and (iii) expensive to
  reproduce in a way that degrades osh's value model.
- Every future waiver must be argued against those three tests **in
  `known-issues.md`**, not decided silently. A waiver that is not written down is
  a divergence, not a decision.
- This does not loosen §305's frozen fidelity scope. §305 says which behaviours
  are in scope; this says a behaviour in scope may still be waived as a defect.

**Rejected alternative — option C, reproduce only the visible half** via an
out-of-band "poisoned index" marker. It is a second parallel representation of
emptiness, threaded through the same readers as option A, for a less honest
model — most of A's cost without A's one virtue.

**Where it bites:** `userspace/oils/src/interp.rs` (`Shell::declare_ref_bind_read`,
`Shell::arrays`, `Shell::assoc`), the corpus case
`a-declaration-with-nothing-to-do-evaluates-the-subscript-the-reference-carries.sh`
(which covers the evaluated-subscript half osh *does* match and deliberately
stops short of the store), and `known-issues.md` as above.
