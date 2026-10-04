# The encoder's reference encode

`rt8.ivf` is libvpx's own encode of the input the encoder's port is checked
against (`tests/encoder.rs`, `frames_match_vpxenc_realtime`): its frames are
the bytes the port's must be.

**Input.** The first 30 pictures the conformance vector
`vp90-2-22-svc_1280x720_1.webm` shows (fetched with the full suite by
`tools/fetch_vectors.py`), 1280x720 I420:

    vpxdec --i420 --limit=30 -o in720.yuv vp90-2-22-svc_1280x720_1.webm

**Encoder.** libvpx v1.17.0 configured for the C code paths and 8-bit
streams only -- `configure --target=generic-gnu --enable-vp9
--disable-vp9-highbitdepth` (vpxenc's default bit depth, and libvpx's
reference C, which its SIMD builds are tested against) -- then:

    vpxenc --codec=vp9 --rt --cpu-used=8 --end-usage=cbr --target-bitrate=1000 \
      --lag-in-frames=0 --threads=1 --tile-columns=0 --aq-mode=3 --kf-max-dist=9999 \
      --undershoot-pct=50 --overshoot-pct=50 --buf-sz=1000 --buf-initial-sz=500 \
      --buf-optimal-sz=600 --min-q=2 --max-q=52 --noise-sensitivity=0 --resize-allowed=0 \
      --i420 -w 1280 -h 720 --fps=30/1 --ivf -o rt8.ivf in720.yuv

`vpxdec --md5 rt8.ivf` gives `51e08b78a895f10ae405e121dad85472`; the file is
134977 bytes, 30 frames.

# The second reference encode

`rt8cut.ivf` reaches what the first reference's thirty pictures of steady
motion never do (`tests/encoder.rs`,
`frames_match_vpxenc_through_cuts_noise_and_edges`; `src/enc/replay.rs`
replays libvpx's own decisions from it too).

**Input.** 150 pictures of 651x357, I420, cut from two conformance vectors
by `common::cut_reference_input` (`tests/common/mod.rs`): a window on
`vp90-2-22-svc_1280x720_1.webm` (two people talking), a cut to a window on
`vp90-2-02-size-lf-1920x1080.webm` (leaves against a sky), that vector's
last picture held still with fresh noise on every picture, then a cut back
to the first vector fading to black. The size is not a whole number of 8x8
cells either way, so blocks hang over the picture's right and bottom edges.
To write it out for `vpxenc` (its MD5 is
`58155d770c2098e80eee156307a3c300`):

    VP9_WRITE_INPUT=cut651.yuv cargo test --release --target x86_64-pc-windows-gnu \
      -p vp9 --test encoder write_cut_reference_input -- --ignored

**Encoder.** The same `vpxenc` as above, at 600 kbit/s:

    vpxenc --codec=vp9 --rt --cpu-used=8 --end-usage=cbr --target-bitrate=600 \
      --lag-in-frames=0 --threads=1 --tile-columns=0 --aq-mode=3 --kf-max-dist=9999 \
      --undershoot-pct=50 --overshoot-pct=50 --buf-sz=1000 --buf-initial-sz=500 \
      --buf-optimal-sz=600 --min-q=2 --max-q=52 --noise-sensitivity=0 --resize-allowed=0 \
      --i420 -w 651 -h 357 --fps=30/1 --ivf -o rt8cut.ivf cut651.yuv

`vpxdec --md5 rt8cut.ivf` gives `19c219ea1b45d6365bb784ddd1844cd8`; the
file is 379148 bytes, 150 frames.

**What it reaches**, read from libvpx's decision trace
(`tools/trace/README.md`): scene cuts at pictures 50 and 115, each coded at
the quantiser the overshoot check raises it to, with the cyclic refresh off;
golden refreshes at 40, 90 and 130; the noise estimate rising to Medium at
picture 96 and falling after the noise stops; every content state, the
change of light (`LowVarHighSumdiff`) on 692 superblocks; and every way a
superblock is partitioned -- copied, the 64x64 early exit, copied after the
source check, and by variance.

# The third reference encode

`rt8small.ivf` is small enough -- 350x286, 352x288 pixels or fewer -- that
libvpx's speed 8 partitions its inter frames by search, trimmed by a small
network, rather than by variance (`tests/encoder.rs`,
`frames_match_vpxenc_when_partitioned_by_search`; `src/enc/replay.rs`
replays its decisions too).

**Input.** 90 pictures of 350x286, I420, from `common::small_reference_input`
(`tests/common/mod.rs`): a window on `vp90-2-22-svc_1280x720_1.webm` (its
first 50 pictures), a cut to a window on `vp90-2-02-size-lf-1920x1080.webm`
(25 pictures), then the first vector again fading to black (15). Its MD5 is
`e75fb192ba8ab28ad9fde1224898daed`; to write it out:

    VP9_WRITE_INPUT=small350.yuv VP9_WRITE_WHICH=small cargo test --release \
      --target x86_64-pc-windows-gnu -p vp9 --test encoder write_cut_reference_input -- --ignored

**Encoder.** The same `vpxenc`, at 200 kbit/s, on an x86-64 processor with
FMA (the learned partitioning's logarithms are glibc's FMA build's:
design-decisions §1341):

    vpxenc --codec=vp9 --rt --cpu-used=8 --end-usage=cbr --target-bitrate=200 \
      --lag-in-frames=0 --threads=1 --tile-columns=0 --aq-mode=3 --kf-max-dist=9999 \
      --undershoot-pct=50 --overshoot-pct=50 --buf-sz=1000 --buf-initial-sz=500 \
      --buf-optimal-sz=600 --min-q=2 --max-q=52 --noise-sensitivity=0 --resize-allowed=0 \
      --i420 -w 350 -h 286 --fps=30/1 --ivf -o rt8small.ivf small350.yuv

`vpxdec --md5 rt8small.ivf` gives `d4b1ceac5f843cdd0fac4f2d38dda5b3`; the
file is 77061 bytes, 90 frames. Its trace has 2,670 superblock estimates,
12,803 network predictions and 16,041 partition decisions; the scene cuts
come at pictures 50 and 75.
