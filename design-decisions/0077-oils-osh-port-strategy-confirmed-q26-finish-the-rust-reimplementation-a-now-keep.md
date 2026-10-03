## 77. Oils (OSH) port strategy confirmed (Q26) — **finish the Rust reimplementation (A) now; keep A as a permanent user option even if a faithful C++ `oils-for-unix` port (B) lands later**

**Date:** 2026-07-21
**Decided by:** Operator (Claude proposed/recommended finishing A; operator
confirmed and added the long-term B-as-well framing) — resolves open-question
**Q26**, which §72 had deferred to the operator as a large, costly-to-reverse
call.

**Context.** §72 committed to building `userspace/oils` as a Rust
reimplementation of the OSH language because there is no C/C++ → `x86_64-slateos`
cross-toolchain in-tree, so a faithful `oils-for-unix` C++ cross-compile (option
B) is prerequisite-blocked. Q26 asked the operator to ratify that or re-order.

**Decision.** Finish the **Rust reimplementation (A)** — which had already reached
high maturity — and ship it. Keep A as a **permanent user-selectable option**
even after a genuine C++ `oils-for-unix` port (B) eventually becomes possible: the
project may adopt B later for bit-for-bit fidelity, but users will still be able
to choose the in-tree Rust `osh`. B is therefore an *additive future option*, not
a replacement that retires A.

**Rationale.** *Pro:* A is the only path that runs on SlateOS today; it's already
nearly done, so finishing it delivers a working bash-superset shell now, and
retaining it as an option hedges against B's fidelity/porting risk and gives
users a lightweight native-Rust shell that needs no C++ toolchain. *Con:*
maintaining two OSH implementations long-term (A and an eventual B) is ongoing
cost; A will never be bit-for-bit upstream OSH on obscure corners.

**Operator's exact words.** "Since I think you're already mostly done with option
A, go ahead and finish it if you're not already finished, but we may eventually
want to go with B, but we'll still have a as option for the user."

**Where it lives.** `userspace/oils/`. Supersedes the "open" status of Q26 in
open-questions.md (now removed). §72 records the original A-vs-B rationale.
