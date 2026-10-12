## 1375. The MP4 reader stays FFmpeg's, translated, under its LGPL

**Date:** 2026-10-09
**Lane:** F
**Decided by:** Operator ("F-Q7: A", `operator-answers/2026-10-09-open-questions-answers.2.txt`), answering `open-questions/F-Q7.md`, after asking back (`operator-answers/2026-10-09-open-questions-answers.txt`): "I'm concerned that we rewrite it cleanroom, it may not be as performant or as reliable? And the demand is smaller than D-Q6 since it's only one program, and if every other video player out there uses ffmpeg, then it seems to be okay? And wouldn't it be easier to accommodate the LGPL requirement than to do a cleanroom rewrite? I think the decision should not necessarily be tied with D-Q6's decision.." Claude had recommended deciding it with D-Q6, the same way.

**In short:** SlateOS reads MP4 files through a translation of FFmpeg's MP4
reader into Rust, which reads every file exactly as FFmpeg does. That stays.
FFmpeg's licence, the LGPL, comes with it: a program containing the reader
must let its user rebuild it with their own copy of the reader -- free for
SlateOS's own programs, whose sources are public.

**Decision.** `gui/video/mp4` stays a translation of FFmpeg's
`libavformat/mov.c` and `seek.c`, under the LGPL, its licence and notice in
`gui/video/mp4/licenses/` and carried into the image by `gather-notices`. Not
tied to D-Q6, as the operator said.

**Rationale.** A clean-room rewrite would cost a new session's work and risk
the files no test covers, for no gain in speed (an MP4 reader costs nothing
next to decoding); every other video player reads MP4 through FFmpeg. The
LGPL's condition is easy to meet for open programs; a closed program would
reach the reader through a system service or a shared library rather than
link it (lane A's note to the operator).

**Alternatives.** B, a clean-room rewrite, which no program's licence would
bind.

**What it asks, and where it will be done.** Nothing to change: the crate is
already this. `roadmap.md`'s MP4 item drops its "carries FFmpeg's licence:
open-questions F-Q7" line for this entry.

**How to reverse.** A clean-room rewrite held to the crate's own tests, which
pin every rule against ffprobe.
