## 1376. H.264 video is FFmpeg's decoder, translated

**Date:** 2026-10-09
**Lane:** F
**Decided by:** Operator ("F-Q8: B", `operator-answers/2026-10-09-open-questions-answers.2.txt`), answering `open-questions/F-Q8.md`, after asking back: "Again, I'm concerned about a cleanroom write laking performance or reliability? And the time it would take. In what ways would we be limited if we translated FFmpeg and had to abide by their LGPL and patents? And what is the likelihood a user would want to view interlaced, 10-bit or 4:2:2 video?" (`operator-answers/2026-10-09-open-questions-answers.txt`). Claude recommended A (written from the specification); the operator chose B after lane A's answers on speed, reliability, time, licence and patents.

**In short:** most MP4 videos -- what phones and cameras record, most of the
web outside YouTube -- are H.264, and SlateOS refuses them today. It will play
them with a translation of FFmpeg's H.264 decoder into Rust, as its MP4
reader is: every kind of H.264, interlaced TV recordings, 10-bit and 4:2:2
included, exact by construction, and the quickest of the options to finish.
The LGPL comes with it, as with the MP4 reader.

**Decision.** A new crate, `gui/video/h264`, translated from FFmpeg's H.264
decoder (`libavcodec/h264*.c` and what it uses), held picture for picture to
FFmpeg and to the H.264 conformance streams; included in the base system
(option B, not D's optional install). `videocodec` stops refusing
`Codec::H264`. Its speed comes from the run-time choice of newer instructions
(§1373) and from threading, as the VP9 port's did -- a translation does not
carry FFmpeg's hand-written assembly over by itself.

**What lane A's answers to the operator settled** (in chat, 2026-10-09):
speed is even between a translation and a rewrite once §1373 allows the
newer instructions; FFmpeg's twenty years of handling broken files come along
with a translation; the translation is the fastest to finish; patents are the
same under every option (they cover what a decoder does, not whose code it
is). Lane A also suggested running the decoders as a system media service,
which keeps the LGPL out of other programs: a design question for when the
player is built, not decided here.

**Alternatives.** A, written from the specification (no licence condition);
C, Cisco's OpenH264 (BSD, no interlaced, 10-bit or 4:2:2); D, an optional
install; E, not yet.

**What it asks, and where it will be done.** `roadmap.md` (lane F): "H.264
and HEVC video" -- H.264 now this translation; HEVC an optional install
(§1370). `known-issues/F-mp4-plays-only-in-the-codecs-webm-has.md` names it.

**How to reverse.** Replace the crate with a rewrite (A) or OpenH264 (C)
behind the same `Decoder`.
