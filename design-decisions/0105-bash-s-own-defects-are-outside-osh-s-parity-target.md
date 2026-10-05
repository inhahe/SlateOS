## §105 — bash's own defects are outside osh's parity target

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended this option). This was Q37 "How far
should osh's bash parity go when the behavior being matched is an upstream bash
*defect*?" in `open-questions.md`.

**Context.** osh is driven toward byte-exact bash 5.2.37 parity, and until this
question every divergence found had turned out to be *designed* bash behaviour
once its source was read. `declare -n q='n[1]'; declare q` — a valueless,
flagless declaration through a reference to an array element — is not: bash
binds a **null value** into `n[1]` via
`bind_variable(q, NULL, ASS_FORCE)` → `assign_array_element("n[1]", NULL, …)` →
`array_insert(a, 1, NULL)`, a NULL that was never checked for. Every reader of
`n` then stops at the null (`${#n[@]}` is 0, `${!n[@]}` is empty) while the
elements are all still there and reappear on the next store. It ignores
`readonly`, and it turns a scalar base into an *empty* array.

**Decision.** Divergences whose bash side is an unchecked defect rather than a
behaviour are waived, marked in the corpus with the reasoning, and recorded in
`known-issues.md`. They are not reproduced.

**Rationale.** The parity target is worth a great deal, but not the core value
model. Reproducing this one means making the array element type nullable
(`Option<Str>`) and teaching every reader — listing, `${!a[@]}`, `${#a[@]}`,
`${a[@]}`, `${a[i]-D}`, arithmetic reads, `unset`, iteration — a "stop at the
first null" rule, purely to chase a state bash cannot explain and may fix
upstream, at which point the change becomes dead weight to unwind.

**Alternatives considered.**
- *B — reproduce it* in the value model. Byte-exact parity with no exceptions,
  which is the stated goal. Rejected as a large invasive change to
  `Shell::arrays` / `Shell::assoc`, threaded through most of `interp.rs`, for a
  defect.
- *C — reproduce only the observable surface*, with a "poisoned" flag on the
  variable that makes readers report it empty until the next store. Rejected as
  exactly the band-aid CLAUDE.md forbids: the flag is a fiction that does not
  survive the next edge case — bash's `n[5]=z` recovery already needs a rule of
  its own.

**The cost, stated plainly.** This sets a precedent that requires judging "bug
vs. design" case by case. The bar: a divergence is waivable only when the bash
side has been traced to its source and found to be an unchecked error path with
nothing in the manual or the comments suggesting intent. Anything short of that
is designed behaviour and gets matched.

**Where it lives.** `known-issues.md`,
`TD-OILS-A-DECLARATION-WITH-NOTHING-TO-DO-BINDS-A-NULL-THROUGH-THE-REFERENCE`;
`userspace/oils/src/interp.rs` — `Shell::declare_ref_bind_read`.
