## 1366. FFmpeg's walks over an MP4 index are taken in one step, with FFmpeg's answers

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** reading an MP4 file, FFmpeg looks things up in its list of a
track's samples by stepping through the list one sample at a time -- to the
next sample not thrown away, back to the last picture a decoder can start
from, and so on. A file built for it makes every one of those steps cover
the whole list, once for each sample in it: a 33 KB file the fuzzer made
took a third of a second to seek in, and a 400 KB file of 16,000 edits
eight seconds to open. The `mp4` crate copied those steps faithfully, so it
had the same flaw. It now takes each step in one move, from small tables it
builds the first time a step turns out long, and gives exactly the answers
FFmpeg's step-by-step code gives.

### Where FFmpeg walks

| Walk | FFmpeg | Long when |
|---|---|---|
| To the next entry kept, inside the binary search | `ff_index_search_timestamp` | a run of discarded entries above the time wanted: each step backs off one entry, and the next walks the run again |
| To the key frame at or before (or after) the found entry | the same | key frames are few |
| Back over entries of the found entry's time | `find_prev_closest_index`, once an edit | many entries share a time |
| To the found entry's composition-offset run, counting from the first | the same | always: it is the entry's index |
| Back to a key frame shown at or before the edit's time | the same | key frames are few, or shown late |

`mov_fix_index` asks `find_prev_closest_index` once for every edit, so the
last four are paid once an edit: a file of many edits over many samples is
quadratic to open, FFmpeg's included. Its index is held to one entry a byte
of the file (§1364), so the file gets no larger an index for it, but the
walks went on: every edit after the room filled still walked the index.

### What was decided

`index.rs`: `Walks` (the next kept entry, the key frames either side, the
start of an entry's run of one time) and `Offsets` (each entry's run, from
the runs' running totals; and a tree of the least time each span of key
frames is shown, for the walk back). Each walk goes 64 entries
(`SHORT_WALK`) one at a time first -- an ordinary file never needs more,
and builds nothing -- then builds its table, once, and looks up. An edit
list builds them once for all its edits; a seek, once for its search.

Each is held to FFmpeg's code as written: the tests keep it, verbatim, and
compare the answers -- and, for an edit, the place among the offsets the
walks leave -- on thousands of random indexes of the shapes the walks meet
(times rising, replayed, or out of order; key frames everywhere, nowhere,
or between; discarded entries scattered or in long runs; runs of no
samples), with the tables built at once, partway through, and never.

### The alternative

**A budget of steps for a file, past which its edits stop being applied or
its seeks give up** -- what §1364 did for the index's size. *For:* a few
lines; the files it changes are made to be slow. *Against:* it changes
answers, and what it changes depends on how far a file's walks go, so a
real file with many edits or a long track -- a feature film's index runs
to hundreds of thousands of entries -- can cross it; every limit of that kind is
one a real file eventually meets. The tables change no answer for any file,
and cost an ordinary one nothing.

### Found on the way

FFmpeg overwrites its buffer of dropped frames' durations at each edit
(`frame_duration_buffer[num_discarded_begin - 1]`); the crate appended to
it, so an edit that never reached its start left its durations for the
next edit's dropped frames to be dated by. Fixed, and held to ffprobe
(`edits_stale_discards.mp4`).
