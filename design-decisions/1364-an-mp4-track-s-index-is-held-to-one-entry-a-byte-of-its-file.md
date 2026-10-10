## 1364. An MP4 track's index is held to one entry a byte of its file

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** an MP4 file lists its samples (the frames of picture and
sound) in tables, and the `mp4` crate keeps an entry for each sample it is
told of. A table can claim billions of samples in a few bytes -- "every
sample is 10 bytes, and there are 3.9 billion of them" -- and the fuzzer
found a 734-byte file that made the crate ask for 46 GB, which kills the
program reading it. Now a file's tracks between them get at most one entry
for each byte of the file, since every sample of a real file is at least a
byte of it. What a player gets out of such a file is unchanged: the samples
the file holds, then its end, exactly as FFmpeg gives them.

**What was decided.**

1. **The room is the file's length, shared by its tracks.** `Parser` starts
   with `entries_left` = the file's length; each track's index
   (`Stream::build_index`) and each fragment's run (`trun`) take their
   entries from it. Shared, not per track: a per-track room lets a file of
   many tracks multiply it, so memory would grow with the square of the
   file's length.
2. **FFmpeg's own ceilings are kept exactly.** FFmpeg's `av_malloc` refuses
   more than `INT_MAX` bytes, so its index of 24-byte entries holds at most
   89,478,485 (`INDEX_ALLOC`), and its table of sample times (12 bytes) at
   most 178,956,970 (`TTS_ALLOC`); it refuses that table outright from
   `UINT_MAX / 12` samples (`TTS_LIMIT`). A track past them gets no index
   there and none here; a fragment past them is refused there and here.
   Fixtures one sample either side of each hold the crate to ffprobe.
3. **Past the room, the index ends -- and is otherwise FFmpeg's.** A track
   cut short of the samples it claims is edited (`mov_fix_index`) as
   FFmpeg's whole index would be; a fragment's run cut short still moves its
   time and position on by the samples not indexed, so the track's length,
   the next run's start and FFmpeg's refusals (a sample of no bytes, a time
   past 2^63) are FFmpeg's; edits giving the same samples again stop at the
   room as FFmpeg's stop when it cannot add an entry.
4. **What can differ, and only for a file claiming more samples than it has
   bytes:** what the crate infers from the samples it did not index -- a
   picture track's frame rate where the claimed samples' durations vary
   past the room, a table's inconsistency past it (FFmpeg's "wrong sample
   count"), where a seek lands when it asks for a time past the file's end.
   No file a muxer writes, whole or cut short, claims more samples than its
   length in bytes unless the cut leaves less than a byte a sample.

**Alternatives.**

- *FFmpeg's behaviour to its allocator's limits, and no further.* Faithful
  to the byte, but each track may then take some 4 GB here (this crate's
  entries are larger than FFmpeg's), and a file of many tracks many times
  that: the fuzzer's 46 GB came from one track.
- *A fixed ceiling, not the file's length.* Any number is arbitrary: one
  large enough for every real file still lets a 1 KB file take gigabytes.
- *Stop indexing at the first sample past the file's end.* Bounds memory
  only together with the room (chunks may overlap), and changes what a
  common file does: a download cut short, sought past its end, gives the
  last frames before the cut where FFmpeg gives nothing.
- *An index computed from the tables as it is read, never stored.* Memory
  would follow the tables, not the claims; but it rewrites the ported
  `mov_build_index` and `mov_fix_index`, which walk stored entries. Not
  needed for the bound; worth it only if a real file ever meets the room.

**Found with it** (fixed in the same change, each held to ffprobe): the
merge of sample times allocated a table as long as the sample count even
when nothing was merged into it -- 2 GB, kept, for an hour of 48 kHz sound
read in chunks; `stsc`'s numbers were read unsigned where FFmpeg's are
ints, so a count of 2^31 or more was not repaired; and the count of sound's
packets in chunks, and of what is left of a chunk, did not wrap as
FFmpeg's unsigned ints do.
