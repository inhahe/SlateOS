## TD-F-MATROSKA-SEEKHEAD-FOLLOWING-HAS-NO-COST-BOUND (lane F, 2026-10-21) — OPEN

**Status:** OPEN

**In short:** a crafted Matroska file can make `matroska::open` keep about
thirty times the file in memory, by nesting the elements its SeekHead (the
file's table of where its parts are) points at. The crate refuses the same
nesting before the first Cluster; the refusal does not cover the elements
the SeekHead leads to.

**Where:** `gui/video/matroska/src/demux.rs`, `follow_seek_head` and
`read_at`. `second_pass` refuses a file once looking past damage has read,
or had handed over, four times the file's length (`Error::Unsupported`,
"damage before the first Cluster too costly to look past"). Following the
SeekHead has no such budget: each entry for a Tags or SeekHead at a new
position — up to the ~62 top-level slots `Level1` keeps — is read in full
through `read_at` and `read_level1`, and what it lists is kept.

**How it is reached** (found by lane D's Opus review at xhigh, relayed in
`notice-f-d-20261021T200333Z`; checked against the code, not yet by a
test): a SeekHead lists 62 Tags positions. Each Tags_k has a size running to
the end of the file and holds Tag_k, then an element the parser does not
know whose data is exactly Tags_(k+1)'s header, so reading Tags_k skips that
and goes on to read Tag_(k+1) ... Tag_61. Following the entries reads about
62*63/2 Tag elements and keeps every SimpleTag string each time: a 64 MiB
file keeps about 2 GB, and `open` does not refuse it. FFmpeg would keep
the tags too. `lib.rs`'s "A hostile file" promises the refusal only for
damage looked past before the first Cluster; this is the same cost reached
through the SeekHead, which no bound covers. The function is older than
this week's second-pass bound (it came with 2c47c42cb's rewrite).

**Fix:** give following the SeekHead a budget as `second_pass` has —
bytes read and fetched, and what is kept, against a multiple of the file's
length — refusing with `Error::Unsupported` past it, and say so in
`lib.rs`. Test it as `tests/metadata.rs`'s
`damage_that_would_keep_the_file_many_times_over_refuses_it` builds its
nested Infos and Tracks, with the nesting reached through SeekHead entries
to Tags instead. Then `python scripts/fuzz.py run gui/video/fuzz 1200`
(its `matroska` target).
