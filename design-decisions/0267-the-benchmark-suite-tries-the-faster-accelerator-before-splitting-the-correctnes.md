## §267 — The benchmark suite tries the faster accelerator before splitting; the correctness gate never leaves TCG

**Date:** 2026-08-21
**Decided by:** Operator (Claude recommended this option; operator: "i'll go
with your recommendation")
**Lane:** A

**In short:** The emulator we test in has a second, much faster mode (3.5×) that
would roughly halve the benchmark-noise problem in §266 — but it silently
switches off three CPU security features the kernel is built around. The
decision is: first spend one boot per candidate measuring whether the fast mode
actually removes the noise (it might make the whole question disappear); if it
does, **split** — benchmarks run on the fast accelerator, the correctness gate
stays on the slow one that still exercises the security features. Never simply
switch everything over.

### Why splitting is safe here specifically

The two accelerators' weaknesses are disjoint *by purpose*. Losing SMEP/SMAP/UMIP
coverage matters for the **correctness** gate, where a missing `stac` shows up as
a fault. The instruction-layout artefact matters only for **benchmarks**, where
it is currently making 71% of the suite unreadable. Splitting puts each weakness
where it does not bite. The infrastructure already exists: §237 made the
accelerator part of the comparison key precisely so two series can coexist
without contaminating each other.

Switching wholesale was rejected outright: giving up the only place SMEP, SMAP
and UMIP are ever exercised, in order to make a benchmark faster, trades a
security property for a convenience.

### The bill that comes with the fast accelerator, stated up front

Fixing the contamination floor restores the *measurement*, not automatically the
*verdict*. Under the fast accelerator the 25% tolerance would apply to a
0.85-cycle quantity whose real variation has never been observed there — a store
costs 0.85 cycles on the real CPU against 5.16 under emulation. And a second
check in the same family (the scattered-access scale test) asserts something
that is simply **false on real hardware**: that a per-access cost cannot depend
on how many pages the loop walks, which ignores caches entirely. So the true
cost is *re-establishing the benchmark suite's self-validation on a platform
where its assumptions do not hold*.

Separately and regardless of this decision: the canary's floor threshold is
**100× too strict** — its stated derivation yields 4 hundredths of a cycle at
the precision the code has used since 2026-08-14, and TCG itself was clearing it
by only 29%. That is a defect under TCG too and is being fixed either way. Full
arithmetic in `known-issues.md` →
`B-A-THE-CONTAMINATION-CANARY-IS-A-TCG-ONLY-INSTRUMENT`.
