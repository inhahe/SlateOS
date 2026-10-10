## 1372. AV1 and AVIF decode with rav1d's hand-written assembly

**Date:** 2026-10-09
**Lane:** F
**Decided by:** Operator ("F-Q4: A", `operator-answers/2026-10-09-open-questions-answers.txt`), answering `open-questions/F-Q4.md` — Claude's recommendation.

**In short:** AVIF pictures and AV1 video decode correctly here but about
twice as slowly as in a browser, because the browsers' AV1 decoder runs some
160,000 lines of hand-written x86 assembly (processor instructions written
directly) and the copy vendored here was brought in without it. It will be
brought in: the browsers' speed at once, from the same code Chrome, Firefox
and Android run on every AV1 picture, which produces exactly the same pixels
as the Rust (dav1d's own tests hold the two to each other).

**Decision.** `gui/video/rav1d` builds with rav1d 1.1.0's `asm` feature: its
x86-64 assembly, assembled at build time with NASM, chosen at run time by the
processor's features. The plain Rust stays, for other processors and as the
reference the assembly is checked against. NASM joins the build machine's
tools (option A's "small install").

**Rationale.** The assembly is the most exercised code in its field; writing
the hot routines again in Rust (option B) would take long to reach what A
gives at once, and remains possible later for any routine that proves to
matter. The cost is code Rust's checks cannot see; the operator chose speed
here.

**Alternatives.** B, the hottest routines in Rust with vector instructions;
C, leave it (about twice as slow as a browser).

**What it asks, and where it will be done.** `roadmap.md` (lane F), under
AVIF: the `asm` feature, the build step that assembles it (left out when rav1d
was vendored -- `gui/video/rav1d/VENDORED.md`), NASM on the build machine, and
the pixels held to the Rust's on every existing fixture and benchmark
(`known-issues.md` "[F] AVIF decoding has no committed benchmark").

**How to reverse.** Turn the feature off; the Rust path is untouched.
