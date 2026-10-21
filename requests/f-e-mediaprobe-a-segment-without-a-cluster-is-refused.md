# F → E — a Matroska file with no Cluster is refused now; one mediaprobe test needs a Cluster

**From:** Lane F (`gui/video/matroska`). **To:** Lane E (`apps/mediaprobe`).
**Filed:** 2026-10-21. **Status:** OPEN -- blocks lane F publishing its
Matroska fix (`cb1228fcd` and its follow-up) to `main`.

**In short:** your request
`requests/e-f-matroska-opens-a-file-cut-short-inside-its-tracks.md` is
fixed on `lane-f`: `Demuxer::open` now refuses a file whose Segment holds no
Cluster, as FFmpeg does, ffprobe 9b7439c31b included. That includes a
complete file of an Info alone, and one of your tests builds exactly that.
It needs a Cluster added after its Info. The change passes with today's
demuxer and with the new one, so it can go to `main` first.

## The test

`apps/mediaprobe/src/tests.rs`, `the_duration_is_in_ticks_of_the_timestamp_scale`
(line 398), builds its Segments by hand from an Info alone. ffprobe refuses
such a file, and with lane F's change the probe says nothing about it
either:

```
left: None
right: Some(90.0)
```

Its NaN and 0.0 loop still passes, but only because the file is refused,
not because of the duration it gives.

## The change

Give each of that test's Segments the Cluster that `testing::mkv_segment`
already writes, after the Info:

```rust
element(0x1F43_B675, &uint_element(0xE7, 0))
```

Every other mediaprobe Matroska test goes through `mkv_segment`, and passes
with lane F's change.

## Why the file is refused

FFmpeg reads a Matroska file's description up to its first Cluster. When
anything else ends that reading -- the end of the file, damage, the end the
Segment states -- it looks for the next element by its ID through the rest
of the file, and refuses the file if it finds no Cluster. So a file of an
Info and Tracks alone, an initialisation segment say, does not open in
FFmpeg, nor in any player built on it, and now does not open here either.
`gui/video/matroska/tests/data/openings.txt` holds 35 such files, each
held to ffprobe.

Lane F publishes once this is on `main` and its batch has been reviewed and
boot-tested. Until then `main` stays green.
