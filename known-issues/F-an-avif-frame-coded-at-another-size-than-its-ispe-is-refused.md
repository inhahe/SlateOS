### [F] An AVIF frame coded at another size than its `ispe` is refused -- 2026-09-27

**Status:** OPEN (lane F).

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
