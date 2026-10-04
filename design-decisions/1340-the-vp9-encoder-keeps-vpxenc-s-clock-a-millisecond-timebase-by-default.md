## 1340. The VP9 encoder keeps vpxenc's clock: a millisecond timebase by default, frame times rounded down to it

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), within §1339's rule that the encoder's
frames are `vpxenc`'s byte for byte.

**In short:** the encoder needs to know how long each frame lasts, because
the bitrate is shared out per second: a frame that lasts longer may spend
more. `vpxenc`, the reference it is checked against, measures time in
whole milliseconds even when told "30 frames a second", so its frames last
33, 33 and 34 ms in turn rather than exactly a thirtieth of a second each,
and each gets a slightly different share of the bitrate. The encoder does
the same by default, so its frames match `vpxenc`'s; a caller who wants
exact thirtieths can say so.

**What was decided.** `EncoderConfig` has a `timebase` (`vpxenc`'s
`--timebase`), defaulting to 1/1000 s. Each picture is stamped with its
start in those units, rounded down -- frame *n* at `n * fps_den * tb_den /
(fps_num * tb_num)` -- and libvpx's frame-rate tracking (`adjust_frame_rate`)
runs on the stamps, converted to its ten-million-a-second clock as libvpx
converts them. A timebase with a zero term is refused.

**Why.** The first difference between the port and the reference encode,
once both made the same decisions, was the rate control's buffer: 351,469
bits against libvpx's 351,136, from a first frame given 33,333 bits where
libvpx gave 33,000. The cause was the clock, not the arithmetic: `vpxenc`
sets a millisecond timebase whatever the frame rate. Every rate-control
number downstream depends on it, so matching `vpxenc` means keeping its
clock.

**Alternatives.**

| | For | Against |
|---|---|---|
| A configurable timebase, `vpxenc`'s by default (chosen) | the reference encodes match; the same knob `vpxenc` and libvpx's API have | at a millisecond, 30 fps frames get 3% uneven shares, as they do in `vpxenc` |
| Exact durations from the frame rate | each frame's share even | frames differ from `vpxenc`'s from the first, and the byte-for-byte test is lost |
| Per-frame timestamps from the caller (libvpx's `vpx_codec_encode` takes a stamp and a duration) | what a screen capture, whose frames come when the screen changes, will want | nothing calls it yet; it is an addition to this, not instead of it, when the capture stream arrives |

**Revisit when** the compositor's capture stream encodes: its frames are
irregular, and it should pass each frame's own time rather than a frame
rate.
