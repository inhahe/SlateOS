## 1345. Video files are read by a Matroska demuxer written from its RFC, held to FFmpeg's packets

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** a video file is a container -- a box holding the compressed
pictures and sound, with their timings -- around codecs like VP9 and AV1.
SlateOS could already decode VP9 and AV1, but had nothing to open the box,
so the video player could not play anything. WebM (the web's own format,
used by YouTube) and its parent Matroska (`.mkv`) are now read by a new
crate, `gui/video/matroska`, written from the format's specification. Where
the specification leaves a reader a choice, it does what FFmpeg -- the
reader inside VLC, mpv and Chrome -- does, and the tests check its output
packet for packet against FFmpeg's.

**What was decided.**
- *Written, not ported:* from RFC 9559 (Matroska) and RFC 8794 (EBML), with
  FFmpeg's `libavformat/matroskadec.c` as the reference for behaviour only
  -- no FFmpeg code (it is LGPL).
- *FFmpeg's choices* where the specification is silent: how a laced block's
  frames are timed, which frames count as key frames, what a codec delay does
  to a timestamp, what an empty frame is, how damage is read past (the rest
  of the damaged Cluster is dropped and reading resumes at the next one),
  where a seek lands. FFmpeg's current behaviour, which changed since 7.1 in
  one place (every frame of a key block is now a key frame).
- *Held to `ffprobe`* with its parsers and fill-in turned off (`-fflags
  +noparse+nofillin`), so that the answers are the demuxer's alone: every
  packet of eleven fixtures (ffmpeg's own WebM files, and hand-written ones
  for lacing, compression, unknown sizes, references and damage), thirty
  seeks, and every decoded frame of VP9, VP9 with alpha and AV1 against
  ffmpeg's decoders.

**Alternatives.**

| | For | Against |
|---|---|---|
| Write it from the RFCs, held to FFmpeg (chosen) | no licence beyond the project's; FFmpeg's behaviour, the one most players share, checked by its own tool | written from scratch -- but the format is small, and the checks are strict |
| Port FFmpeg's demuxer | its behaviour by construction | LGPL; and its structure is built for streams that cannot seek, which makes it larger and harder to bound |
| Port libwebm (Google, BSD) or nestegg (Mozilla, ISC) | permissive licences; nestegg is Firefox's | each makes its own choices where FFmpeg makes others (timing laced frames, damage), and neither has a checking tool as complete as `ffprobe` |

**How to reverse.** The crate is self-contained (`Demuxer`, `Track`,
`Packet`); another demuxer can stand behind the same three types.

**Revisit when** a real file plays differently here than in FFmpeg-based
players: the fixtures and their generator (`tests/data/generate_fixtures.py`)
are the place to add it.
