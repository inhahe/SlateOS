## 1373. Hot loops choose newer processor instructions at run time, behind one checked unsafe call each

**Date:** 2026-10-09
**Lane:** F
**Decided by:** Operator ("F-Q5: A, but maybe we should remember somehow to simplify this if we ever make the whole system depend on x86-64-3 or later?", `operator-answers/2026-10-09-open-questions-answers.txt`), answering `open-questions/F-Q5.md` — Claude's recommendation.

**In short:** on one processor core SlateOS decodes VP9 video at about half
the browsers' speed, VP8 at about 60%, because the browsers' decoders use the
newer vector instructions nearly every processor of the last ten years has,
choosing them when the program starts, and the Rust here may not. It now may:
the few hot routines are compiled twice from the same Rust -- once as today,
once for the newer instructions -- and the second copy is used when the
processor says it has them. Older processors run what they run today.

**Decision.** In lane F's decoders and encoders (`gui/video/vp9`, `vp8`, and
the same gaps listed in F-Q5 for the rest of lane F), a hot routine may have a
second copy marked `#[target_feature(enable = ...)]` (SSSE3, AVX2, or SSE2
where a lane-shuffle needs it), chosen by `is_x86_feature_detected!`. Each
call into such a copy is the one `unsafe` line, its `// SAFETY:` comment
naming the check that establishes the processor has the feature. Nothing that
reads a file from a stranger is unchecked; the copies compute the same samples
as the plain routine, and are tested against it sample for sample.

**The operator's note, kept.** If SlateOS is ever built for x86-64-v3 or later
throughout, `is_x86_feature_detected!` folds to a constant "yes" by itself
(it is `cfg!(target_feature) || a run-time check`); what then remains is to
delete the plain copies and the `unsafe` calls. Recorded in
`deferred-questions.md` with that trigger.

**Rationale.** The standard way Rust programs use newer instructions without
leaving older machines behind; the unchecked part is a handful of call sites,
each guarded by the check std provides.

**Alternatives.** B, build all of SlateOS for x86-64-v3: no `unsafe`, but no
longer starting on machines before 2013-2015, a whole-system decision beyond
video. C, leave it: video plays, at a cost in battery and in headroom on one
core.

**What it asks, and where it will be done.** `roadmap.md` (lane F): VP9's and
VP8's single-thread speed items, which F-Q5 blocked, now open; the crates'
`#![forbid(unsafe_code)]` becomes `deny` with each exception named.

**How to reverse.** Delete the second copies and their calls; the plain
routines are what they replace.
