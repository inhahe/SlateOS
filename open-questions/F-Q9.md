## F-Q9 — [F] Most MP4 videos' sound, and Apple's music files (AAC), will not play. Include an AAC decoder, and which code should it be? — Status: OPEN (raised 2026-10-04)

**In short:** SlateOS can now play the sound of WebM and MP4 files when it is
Opus or Vorbis. But nearly every MP4 file's sound -- phone and camera
recordings, downloads, podcasts -- and every music file bought from Apple
(`.m4a`) is AAC, and those are refused with "the sound is AAC, which is not
decoded here yet". Playing them needs an AAC decoder. As with H.264
(F-Q8), two things need your say: whether SlateOS may include one at all
(AAC is covered by patents, licensed through a pool), and which code it
should start from, since the choices differ in their licences.

**Terms.** *AAC*: the sound format MP4 files usually carry. *AAC-LC* is its
basic form, from 1997-1999; *HE-AAC* (v1 and v2) adds two later tricks
(SBR, which rebuilds the high frequencies, and PS, which rebuilds stereo)
that low-bitrate streams and radio use. *Patent pool*: AAC's patents are
licensed together, today by "Via LA"; AAC-LC's core patents have largely
expired, HE-AAC's run some years longer. Fedora Linux, which leaves out
patented codecs, has shipped AAC-LC decoding since 2017, after its own legal
review. *Fixed point*:
whole-number arithmetic -- one exactly right output, so a port can be held
to its reference bit for bit, as the Opus and Vorbis decoders here are.
*LGPL*: FFmpeg's licence (F-Q7 asks the same about the MP4 reader).

| Option | *What changes:* |
|---|---|
| **A.** Write it from the AAC specification (ISO/IEC 14496-3), LC and HE-AAC, held bit for bit to FFmpeg's fixed-point AAC decoder, and include it | MP4 and `.m4a` sound plays, every common kind. No licence condition on the programs that contain it. The most work. |
| **B.** Translate FFmpeg's fixed-point AAC decoder, as the MP4 reader was | Plays the same as A, exact by construction; the programs carry FFmpeg's LGPL condition (as the video player already does if F-Q7 keeps the MP4 reader). |
| **C.** A or B, AAC-LC only | Most music and video plays; low-bitrate HE-AAC streams (some radio, some podcasts, YouTube's lowest qualities) are refused. Stays within AAC's older, largely expired patents. |
| **D.** Any of A-C as a separate one-click install | As F-Q1 option B proposes for HEIC: the base system contains no AAC code until the user asks for it. |
| **E.** Not yet | AAC is refused by name, as now. |

(Not offered: Fraunhofer's FDK-AAC, whose licence is not a free one and
whose patent terms do not travel with it; FAAD2, which is GPL, a condition
on every program it is part of.)

**If never answered:** safe. AAC sound is refused with a clear message, as
now; the pictures of an MP4 still play (if their codec is decoded), Opus
and Vorbis sound plays. Nothing else is blocked, and it does not get worse
with time -- but the music player will not play `.m4a` files, and most MP4
videos will be silent, until it is answered.

**Claude's recommendation:** **A**, LC and HE-AAC, included. It is complete
and carries no licence condition, and FFmpeg's fixed-point decoder gives an
exact reference to hold it to, as libopus and Tremor do for Opus and
Vorbis. On patents: what remains of AAC's pool is HE-AAC's and is running
out; if you would rather the base system carry none of it, **C** now and
HE-AAC later, or D.

**Where it bites:** a new crate beside `gui/video/opus` and
`gui/video/vorbis` (`gui/video/aac`); `gui/video/codec`'s `Sound`, which
refuses `SoundCodec::Aac` today; the MP4 reader's `esds` (already read);
`known-issues/F-mp4-plays-only-in-the-codecs-webm-has.md`; the roadmap's
"MP4's sound" item.
