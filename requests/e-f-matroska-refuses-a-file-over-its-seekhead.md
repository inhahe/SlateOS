# Lane E -> lane F: `matroska` refuses a whole file over its SeekHead, where FFmpeg reads it

**Filed:** 2026-10-04 by lane E. **For:** lane F (`gui/video/matroska`,
`demux.rs`: `read_top_level`, `read_description`).
**Status:** OPEN.

**In short:** a Matroska file's SeekHead is its table of contents: entries
saying where the file's other parts (its description, its track list) are.
`matroska::Demuxer::open` refuses a file outright in two cases where ffprobe
(the build `matroska` is held to, 2026-03-09 `9b7439c31b`) reads it and lists
its tracks. In the first, the table names a part with an eight-byte number,
which FFmpeg accepts. In the second, one entry of the table is damaged. In
both, FFmpeg plays the file and the player would refuse it. Nothing in lane E
is blocked: since today `apps/mediaprobe` lists a Matroska file's tracks from
`matroska`, so such a file shows as "Matroska" with no tracks. Real muxers
write the four-byte form, so this is rare in practice.

## The two cases

Each file is a WebM header, then a Segment holding a SeekHead whose two
entries point at `Info` (a Duration of 12000 ticks) and `Tracks` (one
`V_AV1` track, 3840 by 2160). Sizes are written in eight bytes
(`01 00 00 00 00 00 00 nn`), as `apps/mediaprobe/src/testing.rs` writes them.

| File | SeekID | Layout | ffprobe | `Demuxer::open` |
|---|---|---|---|---|
| 1 | eight bytes, `00 00 00 00 15 49 A9 66` | SeekHead, Info, Tracks, Cluster | duration 12 s, one AV1 stream 3840x2160 | `Err(Invalid("a binary element too large"))` |
| 2 | nine bytes (too long for anyone) | SeekHead, Info, Tracks, Cluster | logs `Invalid length 0x9 > 0x8 for element with ID 0x53AB`, then the same as 1 | the same refusal |
| 3 | eight bytes | SeekHead, a Cluster of unknown size, Info, Tracks | the same as 1: it follows the SeekHead past the Cluster | the same refusal |
| 4 | nine bytes | as 3 | opens, no streams, no duration | the same refusal |

With a four-byte SeekID, file 3 opens in both, and matches: duration 12000
ticks, `V_AV1` 3840x2160.

1. **SeekID's width.** FFmpeg reads `SeekID` as an unsigned integer
   (`EBML_UINT`, up to 8 bytes, as `matroska_seekhead_entry` declares it);
   `read_top_level` reads it as binary of at most 4 bytes
   (`r.binary(f.size, 4)?`). Files 1 and 3.
2. **A damaged SeekHead ends the open.** The error from inside the SeekHead
   propagates through `read_description`'s `self.read_top_level(...)?`, so
   `open` fails. FFmpeg logs the bad element and goes on: it resynchronises
   to the next top-level element and reads `Info` and `Tracks`. Files 2 and 4.
   For file 4, which has nothing reachable but through the damaged SeekHead,
   the outcome is the same for a reader of tracks: FFmpeg opens it with
   nothing in it, and `matroska` refuses it.

## What is asked

That `Demuxer::open` reads files 1 to 3 as ffprobe does, by whatever route
fits the crate: probably `SeekID` read as `uint` (accepting 1 to 8 bytes),
and a SeekHead that fails to parse passed over -- its good entries kept or
not, as FFmpeg keeps them -- rather than returned as the open's error. How
far the second goes (a damaged `Tags` or `Chapters` is already stepped over
by its size, since nothing reads them) is lane F's call.

## To make the files

```python
import struct

def eid(i):
    return i.to_bytes(8, "big").lstrip(b"\0")

def el(i, body):
    size = bytearray(len(body).to_bytes(8, "big"))
    size[0] = 0x01
    return eid(i) + bytes(size) + body

def uint(i, v):
    return el(i, v.to_bytes(8, "big"))

def case(seek_id_bytes, behind_unknown_cluster):
    info = el(0x1549A966, el(0x4489, struct.pack(">d", 12000.0)))
    video = uint(0xB0, 3840) + uint(0xBA, 2160)
    entry = (uint(0xD7, 1) + uint(0x83, 1) + el(0x86, b"V_AV1")
             + uint(0x23E383, 16_666_666) + el(0xE0, video))
    tracks = el(0x1654AE6B, el(0xAE, entry))
    if behind_unknown_cluster:
        cluster = bytes([0x1F, 0x43, 0xB6, 0x75, 0x01] + [0xFF] * 7) + uint(0xE7, 0)
    else:
        cluster = el(0x1F43B675, uint(0xE7, 0))

    def seek(i, pos):
        return el(0x4DBB, el(0x53AB, i.to_bytes(seek_id_bytes, "big")) + uint(0x53AC, pos))

    head_len = len(el(0x114D9B74, seek(0, 0) + seek(0, 0)))
    info_at = head_len + (len(cluster) if behind_unknown_cluster else 0)
    head = el(0x114D9B74, seek(0x1549A966, info_at) + seek(0x1654AE6B, info_at + len(info)))
    body = head + (cluster + info + tracks if behind_unknown_cluster else info + tracks + cluster)
    return el(0x1A45DFA3, el(0x4282, b"webm")) + el(0x18538067, body)

# File 1: case(8, False); file 2: case(9, False);
# file 3: case(8, True);  file 4: case(9, True); the four-byte control: case(4, True).
```

## If this is never answered

Such files do not open in the player, and `apps/mediaprobe` shows them as a
Matroska file with nothing known about it, where ffprobe lists their tracks.
