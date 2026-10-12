## 1377. AAC sound is FFmpeg's fixed-point decoder, translated

**Date:** 2026-10-09
**Lane:** F
**Decided by:** Operator ("F-Q9: B", `operator-answers/2026-10-09-open-questions-answers.2.txt`), answering `open-questions/F-Q9.md`, after asking back: "The same questions apply as above - performance, reliability, time it would take, how the license and patents would affect us, likelihood of a user needing features option C doesn't provide." (`operator-answers/2026-10-09-open-questions-answers.txt`). Claude recommended A (written from the specification); the operator chose B after lane A's answers.

**In short:** nearly every MP4 file's sound, and every music file bought from
Apple (`.m4a`), is AAC, which SlateOS refuses today. It will play them with a
translation of FFmpeg's fixed-point AAC decoder (whole-number arithmetic, so
exactly one right output, held to FFmpeg bit for bit) -- the basic AAC and its
low-bitrate forms (HE-AAC) alike. The LGPL comes with it, as with the MP4
reader.

**Decision.** A new crate, `gui/video/aac`, translated from FFmpeg's
fixed-point AAC decoder (`libavcodec/aacdec_fixed.c` and what it uses), held
bit for bit to FFmpeg's output on every fixture, as the Opus and Vorbis
decoders are held to theirs; included in the base system. `videocodec`'s
`Sound` stops refusing `SoundCodec::Aac`.

**Rationale.** As §1376: the same speed once §1373 allows newer
instructions, FFmpeg's handling of broken streams brought along, the quickest
to finish, and the patents the same under every option. Lane A noted for the
operator that an AAC-LC-only decoder could still play HE-AAC's core, muffled,
rather than refuse it; the translation needs no such fallback, since it
decodes HE-AAC itself.

**Alternatives.** A, written from ISO/IEC 14496-3; C, LC only; D, an optional
install; E, not yet. Not offered: FDK-AAC (not a free licence), FAAD2 (GPL).

**What it asks, and where it will be done.** `roadmap.md` (lane F): "MP4's
sound" -- AAC now this translation.

**How to reverse.** Replace the crate behind `Sound`'s decoder dispatch.
