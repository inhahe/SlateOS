## 1358. A Matroska file's chapters, tags and attachments are FFmpeg's, down to its order -- but damage before the Clusters does not make the file be read twice

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** a Matroska (`.mkv`) file can carry chapters (named points to
jump to), tags (keys and values: a title, an artist, a comment) and
attached files (cover art, fonts for its subtitles). The `matroska` crate
used to skip all three; it now reads them and gives them exactly as FFmpeg
does -- the same keys, the same values, in the same order, ffprobe's output
being the test. FFmpeg, the program nearly every player is built on, has
one behaviour this does not copy: when one of these elements is damaged
before the video itself begins, FFmpeg starts reading the file again from
the top, which makes it list every track a second time. Here the damaged
element keeps what was read of it and reading goes on after it, the tracks
listed once.

**What was decided.**

- *Whose rules.* FFmpeg's (`matroska_read_header`, `matroska_convert_tag`,
  `avpriv_new_chapter`, `compute_chapters_end`, as of git `9b7439c31b`),
  reproduced from its behaviour, not translated. Every file in
  `tests/data/meta_*.mkv` is held to ffprobe's output for it, including one
  ffmpeg wrote and one cut short at each of 660 lengths
  (`tests/metadata.rs`).
- *Its dictionary, exactly.* FFmpeg keeps metadata in a list where setting
  a key replaces the first entry of that name (case aside) by moving the
  last entry into its place and adding the new one at the end, and its
  renaming pass (`LEAD_PERFORMER` to `performer`, `PART_NUMBER` to `track`)
  rebuilds the list and can leave a key in it twice. `matroska::Metadata`
  does the same, because ffprobe and every FFmpeg player list the entries
  in that order -- a properties dialog built on this shows the same list.
- *Damage partway through.* What FFmpeg read before an error stays, and so
  does the element it was reading as far as it got (a number cut short by
  the end of the file keeps the bytes read, the rest zeros; a string cut
  short is not kept; an attachment whose bytes are cut short is left out).
  Through the SeekHead, a failure stops the SeekHead being followed, and
  FFmpeg then uses no Cues for seeking; so does this.
- *Damage before the first Cluster -- the one departure.* FFmpeg goes back
  to the start of the Segment and reads every top-level element again --
  repeating each track, chapter, tag and attachment before the damage --
  then reads on from the damaged element's ID. Here the damaged element
  keeps what was read, and reading goes on from the next top-level element
  found after its ID: where FFmpeg's second pass goes on from, without the
  repetition. A damaged Info is read the same way -- which is FFmpeg's
  whole answer for it, its second pass reading the same Info again and
  the Tracks after it once (`meta_damaged_info.mkv`). A damaged Tracks
  still refuses the file, as before: FFmpeg's second pass would list
  each of its tracks before the damage twice.
- *Bounded.* FFmpeg's dictionary costs a search of the whole list per
  change; a file built to hold hundreds of thousands of tags for one
  dictionary would take FFmpeg, and an exact copy, hours. Past 2^28 units of
  work (some 20,000 tags in one dictionary; real files hold tens) the rest
  of a file's tags are left out.
- *Attachments' bytes are read when asked for* (`Demuxer::attachment_data`),
  not when the file is opened as FFmpeg reads them: a film with 40 MB of
  subtitle fonts opens without reading them, and they are read only if the
  subtitles are drawn.
- *A chapter's missing end* is FFmpeg's (the next chapter's start, or the
  file's end), computed by `Demuxer::chapter_ends(start)` from the start of
  the presentation the caller gives -- FFmpeg takes it from the first
  packets it reads while probing, which this does not do when opening.

**The alternatives to the departure.**

| | Tracks after damage before the Clusters | Metadata | Matches ffprobe |
|---|---|---|---|
| Keep what was read, go on after the ID (chosen) | once | what was read, once | metadata yes; the stream list not |
| FFmpeg's: read the Segment again from its start | every one twice | what was read, twice (a list repeats, a key is set again) | yes |
| Refuse the file | none | none | no |
| Drop the damaged element whole | once | none of it | no |

- *FFmpeg's* is the only way to match ffprobe's whole output on such a
  file, but what it gives is a file with each of its tracks twice -- a
  player shows two video tracks, one never playing -- and twice each
  attachment. Copying it would copy a fault.
- *Refusing the file* is what a damaged Tracks already does here;
  for damaged tags it would refuse a file that played before this change,
  over metadata.
- *Dropping the element whole* loses the chapters or tags before the
  damage, which FFmpeg keeps.

**Found along the way, and fixed to FFmpeg's behaviour** (each held to
ffprobe by a new fixture): a SeekHead naming the Cues twice is followed to
the *last* place it names, not the first (`cues_last_entry.mkv`); Cues found
through a SeekHead that then fails are not used (`cues_broken.mkv`); a
second Info before the first Cluster is read, starting the timestamp scale
and duration afresh (`meta_info.mkv`), and a second Tracks adds its tracks
(`meta_two_tracks.mkv`); a SeekHead entry is followed to whatever
top-level element stands where it points, which FFmpeg reads whichever it
is (`meta_seek_mismatch.mkv`); and an element whose ID the specification
reserves (its value bits all ones or all zeros) is passed over as one
FFmpeg does not know, where this used to take it for damage and lose the
frames after it up to the next Cluster -- or the whole file, before the
first (`reserved_ids.mkv`).

**Held to.** `gui/video/matroska/tests/metadata.rs` (eleven files' metadata
and the 660 cuts against ffprobe, and what ffprobe cannot show, the
departure above among it), `tests/fixtures.rs` (the packets and seeks of
the new files), and `mutate.py`'s rows for each rule.

**Revisit if** FFmpeg changes its resynchronisation after header damage --
then follow it -- or if a file with more than some 20,000 tags for one
target turns up in the wild: then index the dictionary by key rather than
raise the bound.
