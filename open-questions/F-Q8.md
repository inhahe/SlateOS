## F-Q8 — [F] Most MP4 videos (H.264) will not play. Include an H.264 decoder, and which code should it be? — Status: OPEN (raised 2026-10-04)

**In short:** SlateOS's video player can now open MP4 files, but only those
holding VP8, VP9 or AV1 video. Most MP4 files -- everything a phone or a camera
records, most downloads, most of the web outside YouTube -- hold H.264 instead,
and those are refused with "the video is H.264, which is not decoded here yet".
Playing them needs an H.264 decoder. Two things need your say: whether SlateOS
may include one at all (H.264 is covered by patents, licensed through a
pool), and which code it should start from, since the choices differ in their
licences.

**Terms.** *Decoder*: the code that turns the compressed video back into
pictures. *Patent pool*: the companies holding H.264's patents license them
together (today through "Via LA"); its terms have always charged nothing for
the first 100,000 copies a year, many of the patents have expired, and the
rest expire over the next few years.
*LGPL*: FFmpeg's licence -- any program containing code derived from it must let
its user swap in their own build of that code (open-questions F-Q7 asks the
same about the MP4 reader). *BSD*: a licence with no such condition.
*Conformance streams*: the test videos the H.264 standard publishes, with the
exact pictures every correct decoder must produce. H.264 is defined to the
last pixel, so a decoder is either exactly right or wrong -- which makes
"written from the specification" checkable.

| Option | *What changes:* |
|---|---|
| **A.** Write it from the H.264 specification, checked picture for picture against FFmpeg and the conformance streams, and include it | H.264 MP4 files play. No licence condition on the programs that contain it. The most work of the three, and complete: every kind of H.264, including interlaced TV recordings and 10-bit video. |
| **B.** Translate FFmpeg's decoder, as the MP4 reader was | Plays the same files as A, exact by construction; the video player carries FFmpeg's LGPL condition (as it already does if F-Q7 keeps the MP4 reader). |
| **C.** Port Cisco's OpenH264 decoder (BSD) | Plays what phones, cameras and the web record. Not interlaced broadcast video, nor 10-bit or professional 4:2:2 video, which OpenH264 does not decode. |
| **D.** Any of A-C, but as a separate one-click install | As F-Q1 option B proposes for HEIC: the base system contains no H.264 code until the user asks for it. |
| **E.** Not yet | H.264 files keep being refused by name. |

**HEVC** (H.265: newer iPhones' and some cameras' video) is the same question
with heavier patents -- several pools, most of them active for years yet. It
is the decoder F-Q1 already asks about for iPhone photos (HEIC), and should
follow F-Q1's answer: one HEVC decoder would serve both.

**If never answered:** safe. H.264 and HEVC files are refused with a clear
message, as now; VP8, VP9 and AV1 play. Nothing else is blocked, and it does
not get worse with time -- though the video player will mostly say "not
decoded here" until it is answered.

**Claude's recommendation:** **A**, included. It gives the complete decoder
with no licence condition, and the exactness of H.264 means "written from the
specification" can be held to FFmpeg's pictures as strictly as the other
decoders here are held to their references. On patents: what remains of
H.264's pool costs nothing at SlateOS's scale and is running out; if you would
rather the base system carry none at all, D with A.

**Where it bites:** a new crate beside `gui/video/vp9` (`gui/video/h264`);
`gui/video/codec`'s decoder dispatch (`decoder.rs`), which refuses
`Codec::H264` today; `known-issues/F-mp4-plays-only-in-the-codecs-webm-has.md`;
the roadmap's "H.264 and HEVC video" item.
