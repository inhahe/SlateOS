## 1383. The compositor's video says its colour in the scene protocol: sRGB, coded with BT.601's weights

**Date:** 2026-10-10
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** a window that draws its own pixels -- a game, a video player
-- reaches a remote viewer as VP9 video. The desktop's pixels are sRGB.
Coding turns them into video's brightness-and-colour-difference form with
BT.601's weights, and the stream says "BT.601" in VP9's one colour field,
so that no viewer guesses another set of weights. But Chrome, whose way
with video colour SlateOS now follows (§1381), reads that one field as a
whole colour. It takes SMPTE 170M's primaries (the exact reds, greens and
blues the numbers mean) and its curve along with the weights, and converts
those primaries to the screen's. So a viewer that follows Chrome would
shift the desktop's colours, a strong red by several levels. The scene
protocol, which is the stream's container, now says the colour itself:
sRGB's primaries and curve, BT.601's weights, limited range. A viewer hands
that to its decoder as a file's colour, and Chrome's VP9 decoder takes a
file's colour before the bitstream's. The pictures come back unconverted.

### The choices

| Question | Decided | Alternative | Why |
|---|---|---|---|
| Where the colour is said | The scene protocol: `guiremote::scene::VIDEO_VP9_COLOUR`, H.273's codes for what every `VIDEO_VP9` stream carries | Code with BT.709's weights and say BT.709 in the stream (VP9's colour space 2, which Chrome takes as sRGB's primaries, needing no word from the container) | Both show the desktop's colours unconverted. But the conversion back to pixels here is libyuv's, which caps the weight of blue at 2.0 where BT.709 says 2.112: a strong blue would come back up to 15 levels dark (`known-issues/F-ordinary-video-is-libyuv-s-arithmetic-not-chrome-s.md`), against two for BT.601's 2.018. |
| How it is said | One constant per codec, the codec byte naming both | Four bytes in every `SceneVideo` (scene version 4) | The colour is fixed by the converter the compositor codes with. A different colour is a different stream, which an older viewer should refuse (`BadVideoCodec`) rather than show wrongly. No bytes per frame, and no colour can change in the middle of a stream. |
| What the bitstream says | Still BT.601 (VP9's colour space 1) | Nothing (space 0) | A reader of the bitstream alone -- FFmpeg, a dump of the stream -- takes BT.601's weights and converts nothing. Said nothing, it would guess BT.709 for a window 1280 wide or more, and shift every colour. |
| The weights' code | 6 (SMPTE 170M), Chrome's name for BT.601 | 5 (BT.470BG), FFmpeg's for VP9's space 1 | The same weights. 6 is what Chrome's reading of the bitstream gives, so both places name them by one number. |
| The curve's code | 13 (sRGB) | 1 (BT.709) | What the desktop's pixels are. Chrome shows either unconverted beside BT.709's primaries. |

A compositor test decodes the stream twice, as Chrome would with and
without the protocol's word, and holds both: the stream alone says BT.601
whole (and would be converted), and with the container's colour it comes
back unconverted, within the coding's loss of the buffer.

### Revisit if

The conversion of ordinary video becomes Chrome's float arithmetic (the
known issue above). BT.709's weights then come back as exactly as
BT.601's, and a stream saying BT.709 needs no container to be shown right
-- by any reader, the protocol's own included.
