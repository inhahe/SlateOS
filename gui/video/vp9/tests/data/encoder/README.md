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
