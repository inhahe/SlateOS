## §200 — The B-KNULLJUMP hunt runs the *uninstrumented* kernel first (E), and escalates to the optimized KASAN build (A) only if that fails to settle it

**Date:** 2026-08-15
**Decided by:** Operator (Claude proposed this option — it was Claude's revised
recommendation after measuring the instrumented boot; the operator adopted it)
**Lane:** A

**In short:** there is a rare bug — a jump through a null pointer inside the
kernel, `B-KNULLJUMP` — that shows up on roughly **1 boot in 120**. To catch it
in the act we built a special "instrumented" kernel that checks every memory
access, but that kernel turned out to boot **~20× slower**, which would make the
hunt take over a week of machine time. The question was how to make the hunt
affordable. The answer: **first just run the ordinary kernel** many times, now
that it carries a suspected fix, and see whether the bug stops happening. Only
if that is inconclusive do we go back to the slow instrumented kernel, built
with optimizations on to claw back the speed.

**The options, and why E won.** The full option set (A–E, with measurements) is
preserved in `open-questions.md` → Q43's original analysis, which this entry
replaces as the decision of record.

- **E — soak the plain kernel carrying the `B-NO-CLD-ON-INTERRUPT-ENTRY` fix.**
  Cheap (~283–318 s/boot, versus 5500–8500 s instrumented), and it tests the
  thing we actually care about: whether the bug still happens in the kernel we
  ship. Chosen as the first step.
- **A — build the instrumented kernel `--release` and soak that.** Kept as the
  fallback, not discarded.

**"If necessary" has a specific shape, and it is not symmetric.** This is the
half most likely to be misread later:

- **E *catching* a B-KNULLJUMP falsifies the `B-NO-CLD-ON-INTERRUPT-ENTRY`
  hypothesis**, and is precisely the outcome that gives A a well-motivated job.
- **E coming back clean is suggestive, not proof.** It cannot separate "fixed"
  from "got lucky" at a 1-in-120 base rate. A clean E is therefore **a reason to
  stop, not a reason to escalate** — escalating on it would spend a week of
  machine time to re-answer a question E has already answered as well as it can
  be answered.

**A's cheap gate still stands, and is not optional.** **No release kernel has
ever been booted in this project** — every boot test to date is the debug
profile. So before any release soak: build `--release`, run
`scripts/kasan-check-preshadow.py`, and attempt **exactly one** boot (~30 min).
That answers both unknowns — does it boot at all, and what does it actually
cost — before the soak is committed to.

**A clean *release* soak is weaker evidence than a clean debug one.**
Optimization perturbs instruction timing and layout, which is exactly what a
1-in-120 race depends on; the base rate itself is a debug-build measurement and
may not carry over. Any result reported from A must carry this caveat attached
(§119 already records it).

**Two caveats on E's own numbers**, from the 2026-08-13 update: it samples a
SMAP-enabled kernel, which the 1-in-120 base rate was **not** measured on, and
its per-boot wall time is ~355 s rather than the ~283–318 s the ~21 h soak
budget was built from.

**Where it lives:** `scripts/kasan-build.sh`,
`scripts/kasan-check-preshadow.py`, `scripts/boot-test.sh` (the soak driver),
`known-issues.md` → `B-KNULLJUMP` and `B-NO-CLD-ON-INTERRUPT-ENTRY`, and
§107/§118/§119 for how the instrumented profile got here.

**Provenance:** the operator answered in a Lane B session ("q43: e, then a if
necessary"); Lane B relayed it as `requests/b-a-operator-answered-q43.md`
rather than writing into Lane A's §200–299 range itself.
