## F-Q7 — [F] The MP4 reader is translated from FFmpeg, whose licence then binds the video player. Keep it, or have it written again? — Status: OPEN (raised 2026-10-04)

**In short:** SlateOS now opens MP4 files -- the commonest video format
there is -- through a reader (`gui/video/mp4`) made by translating the
corresponding part of FFmpeg, the program nearly every video player is
built on, from C into Rust. That makes every MP4 file come out exactly as
FFmpeg reads it, which the tests check packet by packet. But FFmpeg's
licence, the LGPL, comes with the translation: any program that contains
the reader -- the video player, and anything else that plays MP4 -- must
let the person who receives it rebuild it with their own copy of the
reader. This is the same choice D-Q6 asks about the C library, made
smaller: there the condition reaches every program, here only those that
play MP4. Keep the translation, or have the reader written again without
FFmpeg's code?

**Terms.** *LGPL*: FFmpeg's licence -- free to use and change, but code
derived from it stays under it, and a program containing it must let its
user swap in their own build of that code. *Translated*: written by
reading FFmpeg's functions and writing each again in Rust, decision for
decision; the crate names each function it follows. *Clean-room rewrite*:
written again by someone who has not read FFmpeg's code, from the MP4
specification and from what FFmpeg does with files (its outputs), held to
the same tests.

**What is translated.** `gui/video/mp4`, all of it, from FFmpeg's
`libavformat/mov.c` and `seek.c` (git 9b7439c31b): how the boxes are
walked and damage read; how the sample tables become a list of samples;
how an edit list trims, retimes and repeats them; fragmented files; the
order the tracks' samples come out in; seeking. Its licence text and a
notices entry are in `gui/video/mp4/licenses/`, so the image would carry
them. Nothing else of lane F's is translated from FFmpeg: the Matroska
reader (`gui/video/matroska`) was written from the Matroska specification,
holding to FFmpeg's choices without its code.

| | A. Keep the translation | B. Clean-room rewrite |
|---|---|---|
| *What changes* | the video player, and any program playing MP4, carries the LGPL's condition | no program carries it |
| Behaviour | FFmpeg's, exactly: 54 tests, every packet of 31 files and 14 sets of seeks held to ffprobe | the same tests decide; the risk is files no test covers |
| Cost | none | a new session that has not read `mov.c` writes it from the specification and these tests (it cannot be this one) |
| Follows | | design-decisions §1133's reasoning, as D-Q6's option B would |

**Recommendation: decide this with D-Q6, the same way.** If programs may
carry the LGPL's condition, keep both. If they may not, both need
rewriting -- this one is the easier of the two, since its tests already pin
every rule: the box walk, the tables, the edit lists and the reading
order are all held to ffprobe's output, so a rewrite can be judged
complete by the same suite (`gui/video/mp4/tests/`).

**If it is never answered:** nothing breaks. MP4 plays; the binaries that
contain the reader carry FFmpeg's licence and notice, which `gather-notices`
puts in the image. The question only becomes urgent before SlateOS is
distributed under terms that the LGPL's condition would conflict with.

**Where it bites.** `gui/video/mp4` (all of it); `gui/video/codec`
(`videocodec`), once it plays MP4, and every program built on it.
