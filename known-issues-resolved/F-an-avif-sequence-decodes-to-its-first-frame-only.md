### [F] An AVIF sequence decodes to its first frame only -- 2026-09-27

**Status:** FIXED on `lane-f` 2026-09-27 (`imagecodec::avif::Animation`,
`src/avif/animation.rs`); moves to `known-issues-resolved.md` once on `main`
through a boot test. The viewer playing it is lane E's
(`requests/f-bce-avif-pictures-open-and-animate.md`).

**In short:** an animated AVIF shows as a still picture -- its first frame.
GIF and WebP animate, through `gif::Animation` and `webp::Animation`.

**The proper fix.** An `avif::Animation` of the same shape (`next_frame`,
`rewind`, `repeat`), keeping one decoder per track across frames as libavif
does, reading each frame's duration from the track's `stts` (`setup::Timing`,
already parsed) and the repetition from `elst`, with a sync-sample check so
that seeking restarts from a key frame. libavif's `avifDecoderNextImage` and
`avifDecoderNthImage` are the model; Pillow's `get_frame(n)` is the oracle.
