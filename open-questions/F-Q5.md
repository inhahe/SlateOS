## F-Q5 — [F] VP9 video decodes at about half a browser's speed on one processor core. Allow a few checked-at-run-time lines of `unsafe` to use newer processor instructions? — Status: OPEN (raised 2026-10-03)

**In short:** SlateOS now decodes VP9 video (the format of most WebM files
and of much of YouTube) correctly, and on several cores it is as fast as the
browsers' decoder, libvpx, or faster. On *one* core it is about half as
fast. The difference is not the program but the instructions it may use:
every x86-64 processor has the basic vector instructions (SSE2) SlateOS is
built for, while libvpx also uses the newer ones nearly every processor of
the last ten years has (SSSE3, AVX2), choosing at start-up by asking the
processor. Rust can do the same -- build the few hot routines twice, and
pick at run time -- but calling the newer copy is one line Rust's safety
checker cannot see through (`unsafe`), and this crate forbids `unsafe`
today, because it reads files from strangers.

**Measured** (`gui/video/vp9/tests/bench.rs`, frames per second, an
i7-8700K, best runs on a machine busy with other work):

| Video | Here, 1 core | Here, 6 cores | libvpx, 1 core | libvpx, 6 cores |
|---|---|---|---|---|
| 1080p, one tile column (one core only, for any decoder) | 42 | 49 | 77 | 70 |
| 1080p in 4x4 tiles | 56 | 92 | 71 | 79 |
| 4K (3840x2160), 8 tile columns | 26 | 41 | 31 | 69 |

(A *tile column* is a strip of the picture coded independently, which is
what lets several cores share a frame. YouTube's 1080p video has four.)

**The options.**

| Option | *What changes:* |
|---|---|
| **A.** Allow the run-time choice | The hot routines (motion compensation, the loop filter) are compiled a second time for SSSE3/AVX2 from the *same* Rust, and chosen when the processor says it has them. One core gets closer to libvpx's speed; older processors run the code they run today. The only unchecked line is each call into the second copy, whose single requirement -- "this processor has AVX2" -- is exactly what the run-time check establishes. Nothing that reads the video file is unchecked. |
| **B.** Raise SlateOS's minimum processor | Build all of SlateOS for x86-64-v3 (AVX2: Intel from 2013, AMD from 2015). The compiler uses the newer instructions everywhere, with no `unsafe` anywhere -- but SlateOS would no longer start on older machines at all. A whole-system decision, beyond video. |
| **C.** Leave it | One core decodes 1080p at about 40 frames a second -- faster than it plays -- and video with tile columns uses more cores. Fine for watching; a cost in battery and in headroom for 4K or 60 frames a second on one core. |

**If never answered:** safe, and nothing is blocked: video plays (option
C). It does not get worse over time. The VP9 encoder, next on the roadmap,
will raise the same question, since searching for motion is the same kind
of loop.

**Claude's recommendation:** **A.** The unchecked part is a handful of call
sites, each guarded by the processor check std provides, while everything
that parses the file stays in checked Rust; it is the standard way Rust
programs use newer instructions without leaving older machines behind. B is
worth weighing for the whole system someday, but not for video's sake.

**Where it bites:** `gui/video/vp9/src/inter.rs` and `loopfilter.rs` (the
crate's `#![forbid(unsafe_code)]` would become a single module allowed it);
design-decisions §1339, which set the crate's no-`unsafe` rule and said
this would come here rather than change quietly.
