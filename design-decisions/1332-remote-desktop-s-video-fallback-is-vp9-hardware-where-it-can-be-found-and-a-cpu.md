## 1332. Remote desktop's video fallback is VP9: hardware where it can be found, and a CPU codec that uses every core

**Date:** 2026-09-27
**Lane:** F
**Decided by:** Operator — answering `open-questions.md` F-Q2. Claude
recommended D (VP8 first, VP9 later); the operator chose VP9 directly, and
added the hardware and threading requirements.

**In short:** when a game or a video is on screen, remote desktop will film
the screen and send it as VP9 video. VP9 is a free (no patent fees) format that
needs about half the bandwidth of H.264 at the same quality. Where the
computer's graphics chip can encode or decode VP9 itself, it does; everywhere
else a software codec does it, split across as many threads as the machine has
cores. The operator's words: "do vp9. if possible, find or write hardware
encoders and decoders for it (preferably find), and also have cpu fallback
either way - multithreaded according to the number of cores in a user's
system."

**What that means in practice.**

- **Software: port libvpx**, Google's reference VP9 encoder and decoder (BSD
  licence, so no conditions beyond keeping the notice). It is the codec every
  browser's VP9 was checked against, and it is the "find" the operator
  prefers to "write". It threads by tile columns and rows, and the thread
  count is the machine's core count. SIMD (vector instructions) comes with it.
- **Hardware: find, not write.** Graphics chips expose their video engines
  through drivers. The open ones that do VP9 are Intel's media driver (MIT
  licence) and AMD's through Mesa (MIT). Both need the GPU stack (lane A's
  kernel driver, lane F's Mesa port), so they come when it does. NVIDIA's video
  engines are reachable only through its closed driver, so they are out of
  reach until that changes.
- **Both directions.** The encoder runs where the screen is filmed (the
  compositor's capture stream). The decoder runs in SlateOS's own remote
  viewer, and anywhere else VP9 is played.

**Alternatives.**

| | For | Against |
|---|---|---|
| VP9 (chosen) | half H.264's bandwidth at the same quality; no patent fees; plays in browsers | the most work of the three; slower to encode than VP8 |
| VP8 first, VP9 later (Claude's recommendation) | a working fallback soonest, extending the WebP decoder already here | two codecs to maintain, and the worse one first |
| H.264 | decoded in hardware almost everywhere | patent-pooled; the basic profile's patents mostly expired, not all |

**How to reverse.** The capture stream names its codec; a second codec
is an addition beside libvpx, not a replacement of it.
