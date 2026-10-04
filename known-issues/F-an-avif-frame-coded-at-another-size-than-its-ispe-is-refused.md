### [F] An AVIF frame coded at another size than its `ispe` is refused -- 2026-09-27

**Status:** OPEN — answered on lane-f 2026-10-04, and moves to
`known-issues-resolved/` once that has had a boot test on `main`.

**The answer:** `gui/video/yuv/src/scale.rs` (first written as
`gui/imagecodec/src/avif/scale.rs`), called from `decode.rs`'s
`scale_tile` where the refusal was: libyuv 1924's `ScalePlane` and
`ScalePlane_12` with `kFilterBox`, every method they pick by the sizes, as
libyuv's C computes them (design-decisions §1344 for why the C and not the
x86 SIMD). Held to libyuv built with its x86 code off on 62,208 size
combinations and 57 larger cases, and to Pillow byte for byte on 28 fixtures
whose `ispe` is rewritten after encoding (`tests/data/generate_avif_rescale.py`,
`tests/avif.rs`'s `every_rescaled_fixture_decodes_to_pillow_s_pixels`).

**In short:** a rare kind of AVIF, whose AV1 frame is stored at a different
size from the size the file declares for it, fails to open with "unsupported";
Chrome and Pillow rescale the frame and show it. None of the 224 files in the
test corpus (libavif's test data and the AOM sample set) needs this.

**Where.** `gui/imagecodec/src/avif/decode.rs`, `decode_as`: the check after
each tile's decode returns `Error::Unsupported("AVIF frame of another size
than its ispe")` where libavif calls `avifImageScaleWithLimit`.

**The proper fix.** Port libavif's `src/scale.c` and the libyuv functions it
calls -- `ScalePlane` and `ScalePlane_12` with `kFilterBox`, which choose among
box downscaling, bilinear upscaling and the special ratios by the sizes -- into
`avif/libyuv.rs`, and test it against Pillow with a fixture whose `ispe` is
rewritten after encoding (the way `generate_avif_pixels.py` rewrites `colr`).
