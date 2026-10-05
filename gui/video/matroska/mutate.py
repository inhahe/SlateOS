"""Mutation test for the matroska crate.

Each row puts back one way of not reading Matroska as FFmpeg reads it -- a
flag ignored, a lacing misread, a Cluster's end missed, a cue passed over, a
seek that reads too far -- and names the tests that have to notice. Most are
the fixtures' tests, which hold every packet and every seek to what ffprobe
makes of the same file (`tests/fixtures.rs`, `tests/data/generate_fixtures.py`);
the rest are what ffprobe cannot show (`tests/beyond_ffprobe.rs`) and the
modules' own.

Adopted from lane E's sweep of `apps/mediaprobe`'s Matroska demuxer (its
`mutate.py`, 2026-10-04), whose rows are here as this crate's code spells
them, when this crate became the tree's one demuxer.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# A video track's projection, as ffprobe reads it (`tests/projection.rs`).
PROJECTIONS = "every_projection_is_read_as_ffmpeg_reads_it"

# The fixtures' tests: every packet, and the packets after each seek, as
# ffprobe gives them.
ORDER = "packets_of_order"
LACINGS = "packets_of_lacings"
LACED = "packets_of_laced"
BAD_LACES = "packets_of_bad_laces"
GROUPS = "packets_of_groups"
UNKNOWN_CLUSTERS = "packets_of_unknown_clusters"
UNKNOWN_SIZES = "packets_of_unknown_sizes"
CODEC_DELAY = "packets_of_codec_delay"
DELAYED = "packets_of_delayed"
VP9_OPUS = "packets_of_vp9_opus"
VP9_ALPHA = "packets_of_vp9_alpha"
SUBTITLES = "packets_of_subtitles"
TICK = "packets_of_tick_100us"
COMPRESSED = "packets_of_compressed"
ENCODINGS = "packets_of_encodings"
ZLIB_LACES = "packets_of_zlib_laces"
EMPTY_FRAMES = "packets_of_empty_frames"
IGNORED_TRACKS = "packets_of_ignored_tracks"
OVERRUN = "packets_of_overrun"
TRACKS_AT_THE_END = "packets_of_tracks_at_the_end"
RESYNC = "packets_of_resync"
DAMAGED = "packets_of_damaged"
SEEKS_CUED = "seeks_in_cued"
SEEKS_UNCUED = "seeks_in_uncued"
SEEKS_VP9_OPUS = "seeks_in_vp9_opus"
SEEKS_OTHER_CUES = "seeks_in_cues_of_another_track"
SEEKS_ONE_CUE = "seeks_in_one_cue_point"
SEEKS_LIVE = "seeks_in_live"
SEEKS_REORDERED = "seeks_in_reordered"
SEEKS_WALK_DAMAGE = "seeks_in_walk_damage"
SEEKS_ZLIB_LACES = "seeks_in_zlib_laces"
RESYNC_POINT = "packets_of_resync_point"
RESYNC_IN_GROUP = "packets_of_resync_in_group"
# What ffprobe cannot show.
WALK = "a_walk_reads_only_as_far_as_the_seek_needs"
SPARSE = "a_seek_goes_to_a_cue_not_past_it"
PRE_ROLL = "a_caller_pre_rolls_by_seeking_that_much_earlier"
SEGMENT = "the_segment_says_its_tick_and_its_length"
STORED = "a_track_stored_as_this_cannot_undo_is_passed_over"
NO_KEYS = "a_seek_in_a_track_without_key_frames_fails_and_reading_goes_on"
CUES_MET = "cues_met_while_reading_are_passed_over"
# Cues the SeekHead names twice, and Cues left unused after a failure.
SEEKS_CUES_LAST_ENTRY = "seeks_in_cues_last_entry"
SEEKS_CUES_BROKEN = "seeks_in_cues_broken"
SEEKS_TWO_CUES = "seeks_in_two_cues"
SEEKS_DAMAGED_CUES = "seeks_in_damaged_cues"
# The metadata: as ffprobe shows it (`tests/metadata.rs`), and the packets
# of the same files (`tests/fixtures.rs`).
META_FFMPEG = "metadata_of_ffmpeg_s_file"
META_TAGS = "metadata_of_tags"
META_CHAPTERS = "metadata_of_chapters"
META_ATTACHMENTS = "metadata_of_attachments"
META_INFO = "metadata_of_two_infos"
META_DATE = "metadata_of_a_date"
META_TWO_TRACKS = "metadata_of_two_tracks_elements"
META_SEEK_HEAD = "metadata_through_the_seek_head"
META_MISMATCH = "metadata_where_the_seek_head_names_the_wrong_element"
META_TRAILING = "metadata_after_the_clusters"
META_CUT = "metadata_cut_short_anywhere_is_ffmpeg_s"
META_DAMAGE = "damage_before_the_first_cluster_keeps_what_was_read"
META_DEEP = "tags_nest_no_deeper_than_ffmpeg_reads"
META_INFO_PACKETS = "packets_of_meta_info"
META_DAMAGED_INFO = "metadata_of_a_damaged_info"
META_DAMAGED_INFO_PACKETS = "packets_of_meta_damaged_info"
META_TWO_TRACKS_PACKETS = "packets_of_meta_two_tracks"

# (name, old, new, [tests that must fail])
BLOCK = [
    (
        "a block's timestamp loses its sign",
        "Ok((track, i16::from_be_bytes([t0, t1]), flags))",
        "Ok((track, i16::from_be_bytes([t0 & 0x7f, t1]), flags))",
        [ORDER, UNKNOWN_SIZES],
    ),
    (
        "Xiph lacing stops at a size byte of 254",
        "if b != 0xff {",
        "if b < 0xfe {",
        [LACINGS],
    ),
    (
        "a Xiph lace's size may run past the block",
        "            if left < total {\n"
        "                return Err(bad(\"a Xiph-laced block's sizes\"));\n"
        "            }\n"
        "            sizes.push(left - total);",
        "            sizes.push(left.wrapping_sub(total));",
        ["xiph_lacing_reads_runs_of_255"],
    ),
    (
        "fixed lacing need not divide evenly",
        "if !left.is_multiple_of(laces) {",
        "if false {",
        [BAD_LACES, "fixed_lacing_shares_the_bytes_out"],
    ),
    (
        "a laced block holds one frame fewer than its count says",
        "let laces = usize::from(count) + 1;",
        "let laces = usize::from(count).max(1);",
        [LACINGS, LACED],
    ),
    (
        "EBML lacing's last frame is as long as its first",
        "                *s = left - total;",
        "                *s = first.min(left);",
        [LACINGS, LACED],
    ),
    (
        "EBML lacing's differences are from the first size, not the one before",
        "                previous = next;",
        "                let _ = next;",
        [LACED],
    ),
]

EBML = [
    (
        "an all-ones size is a size, not unknown",
        "let size = if size == (1u64 << (size_len * 7)) - 1 {",
        "let size = if false {",
        [UNKNOWN_CLUSTERS, UNKNOWN_SIZES, "all_ones_is_an_unknown_size_at_any_length"],
    ),
    (
        "a block's track number keeps its marker bit",
        "let mut v = u64::from(first & value_mask(n));",
        "let mut v = u64::from(first);",
        [ORDER, "lacing_numbers_read_from_bytes"],
    ),
    (
        "EBML lacing's differences are read unsigned",
        "Some((len, i64::try_from(v).ok()? - bias))",
        "Some((len, i64::try_from(v).ok()?))",
        [LACINGS, LACED, "lacing_numbers_read_from_bytes"],
    ),
    (
        "a string keeps its zero padding",
        "            s.truncate(nul);",
        "            let _ = nul;",
        ["values_read_as_ebml_writes_them"],
    ),
    (
        "a signed integer is read unsigned",
        "let mut v = i64::from(self.byte()?.cast_signed());",
        "let mut v = i64::from(self.byte()?);",
        ["values_read_as_ebml_writes_them"],
    ),
    (
        "a reserved ID is damage",
        "        let (_, id) = self.vint(4, true)?;\n",
        "        let (id_len, id) = self.vint(4, true)?;\n"
        "        let value_bits = id_len * 7;\n"
        "        let value = id & ((1u64 << value_bits) - 1);\n"
        "        if value == 0 || value == (1u64 << value_bits) - 1 {\n"
        "            return Err(Error::Invalid(\"a reserved EBML ID\"));\n"
        "        }\n",
        ["packets_of_reserved_ids", "a_reserved_id_is_an_id"],
    ),
    (
        "a child may run past its parent",
        "            if child_end > end {\n"
        "                return Err(Error::Invalid(\"an element running past its parent\"));\n"
        "            }\n",
        "",
        ["a_child_running_past_its_parent_is_an_error"],
    ),
    (
        "a binary element is allocated before it is found to be past the end",
        "        if n > self.remaining() {\n"
        "            return Err(Error::Truncated);\n"
        "        }\n"
        "        let mut v",
        "        let mut v",
        # The allocation fails, which aborts the test binary: the harness
        # scores that a crash, not a named failure.
        ["a_size_past_the_end_is_refused_without_allocating"],
    ),
]

TRACK = [
    (
        "a track's default duration is not read",
        "ids::DEFAULT_DURATION => default_duration = r.uint(c.size, 0)?,",
        "ids::DEFAULT_DURATION => {\n                r.uint(c.size, 0)?;\n            }",
        [LACINGS, ORDER, LACED],
    ),
    (
        "the codec delay is not read",
        "ids::CODEC_DELAY => codec_delay = r.uint(c.size, 0)?,",
        "ids::CODEC_DELAY => {\n                r.uint(c.size, 0)?;\n            }",
        [CODEC_DELAY, DELAYED, VP9_OPUS],
    ),
    (
        "the seek pre-roll is not read",
        "ids::SEEK_PRE_ROLL => seek_pre_roll = r.uint(c.size, 0)?,",
        "ids::SEEK_PRE_ROLL => {\n                r.uint(c.size, 0)?;\n            }",
        [PRE_ROLL],
    ),
    (
        "a stripped header is not put back",
        "                v.extend_from_slice(prefix);",
        "                let _ = prefix;",
        [COMPRESSED, ENCODINGS],
    ),
    (
        "zlib is not undone",
        "deflate::zlib_inflate_limited(frame, limit)",
        "Ok::<Vec<u8>, ()>(frame.to_vec())",
        [COMPRESSED, ZLIB_LACES],
    ),
    (
        "a bzip2 track is read as it is stored",
        "1 => Encoding::Unsupported(\"a bzip2-compressed track\"),",
        "1 => Encoding::None,",
        [STORED],
    ),
    (
        "an encrypted track is read as it is stored",
        "    if e.kind != 0 {",
        "    if e.kind != 0 && false {",
        [STORED],
    ),
    (
        "a track of a kind FFmpeg ignores is read",
        "            _ => None,",
        "            _ => Some(Self::Video),",
        [UNKNOWN_SIZES],
    ),
    (
        "a codec ID of another kind is taken",
        "let Some(codec_id) = codec_id.filter(|id| kind.fits(id)) else {",
        "let Some(codec_id) = codec_id else {",
        [IGNORED_TRACKS],
    ),
    (
        "a track without a codec ID is read",
        "let Some(codec_id) = codec_id.filter(|id| kind.fits(id)) else {",
        "let Some(codec_id) = codec_id.or(Some(b\"V_\".to_vec())).filter(|id| kind.fits(id)) else {",
        [IGNORED_TRACKS],
    ),
    (
        "a mirror's roll is not turned back",
        "let turn = if mirror { roll } else { -roll };",
        "let turn = -roll;",
        [PROJECTIONS],
    ),
    (
        "a mirroring yaw does not mirror",
        "                *v = v.wrapping_neg();",
        "                *v = *v;",
        [PROJECTIONS],
    ),
    (
        "a pitch is turned like a roll",
        "pitch == 0.0 && (yaw == 0.0 || yaw == 180.0 || yaw == -180.0),",
        "yaw == 0.0 || yaw == 180.0 || yaw == -180.0,",
        [PROJECTIONS],
    ),
    (
        "spherical metadata of an unknown version is read",
        "if p.kind != 0 || p.version.is_some_and(|v| v != 0) {",
        "if p.kind != 0 {",
        [PROJECTIONS],
    ),
    (
        "a projection's fault refuses nothing",
        "                return Err(Error::Invalid(why));",
        "                let _ = why;",
        [PROJECTIONS],
    ),
    (
        "an equirectangular private of another size is taken",
        "(_, 1, _) => Some(\"an equirectangular projection's private data\"),",
        "(_, 1, _) => None,",
        [PROJECTIONS],
    ),
    (
        "an equirectangular projection's bounds are not checked",
        "(bottom >= u32::MAX.wrapping_sub(top) || right >= u32::MAX.wrapping_sub(left))",
        "(bottom == top && right == left && top == 1)",
        [PROJECTIONS],
    ),
    (
        "a cubemap's missing private data is taken",
        "(_, 2, 0..=3) => Some(\"a cubemap projection without its private data\"),",
        "(_, 2, 0..=3) => None,",
        [PROJECTIONS],
    ),
    (
        "a cubemap private of another size is taken",
        "(_, 2, _) => Some(\"a cubemap projection's private data\"),",
        "(_, 2, _) => None,",
        [PROJECTIONS],
    ),
]

CUES = [
    (
        "cue positions count from the file's start",
        "cluster: cluster.checked_add(segment_data)?,",
        "cluster: Some(cluster)?,",
        # Not `seeks_in_cued`: its Cues land a few bytes before each Cluster,
        # and the resync from there finds the right one.
        [SEEKS_VP9_OPUS, "seeks_in_av1", "seeks_in_vp8_vorbis"],
    ),
    (
        "one cue point is used",
        "if points.len() < 2 {",
        "if points.is_empty() {",
        [SEEKS_ONE_CUE],
    ),
]

DEMUX = [
    # Reading the description.
    (
        "a damaged Info before the Clusters refuses the file",
        "                    ids::TRACKS => self.read_tracks(&h)?,\n"
        "                    ids::CUES => cues_read.push(h.start),",
        "                    ids::INFO => nest::read_info(&mut self.r, &h, IN_SEGMENT, &mut self.info)?,\n"
        "                    ids::TRACKS => self.read_tracks(&h)?,\n"
        "                    ids::CUES => cues_read.push(h.start),",
        [META_DAMAGED_INFO, META_DAMAGED_INFO_PACKETS],
    ),
    (
        "SeekHead positions count from the file's start",
        "                .checked_add(self.segment_data)\n",
        "                .checked_add(0)\n",
        [TRACKS_AT_THE_END, SPARSE, META_SEEK_HEAD],
    ),
    (
        "Tracks after the Clusters are not looked for",
        "            ids::TRACKS => self.read_tracks(&h),\n",
        "            ids::TRACKS => Ok(()),\n",
        [TRACKS_AT_THE_END],
    ),
    (
        "Cues the SeekHead points to are not noted",
        "            level1.cues_unread().into_iter().collect()\n",
        "            Vec::new()\n",
        [SPARSE],
    ),
    (
        "only the first Cues element before the Clusters is the index",
        "                    ids::CUES => cues_read.push(h.start),",
        "                    ids::CUES if cues_read.is_empty() => cues_read.push(h.start),\n"
        "                    ids::CUES => {}",
        [SEEKS_TWO_CUES],
    ),
    (
        "damaged Cues are no index",
        "                let _ = self.read_cue_points(at, &mut points);",
        "                if self.read_cue_points(at, &mut points).is_err() {\n"
        "                    points.clear();\n"
        "                }",
        [SEEKS_DAMAGED_CUES],
    ),
    (
        "the first SeekHead entry naming the Cues is the one followed",
        "            entry.pos = pos;\n",
        "            if entry.pos == 0 {\n                entry.pos = pos;\n            }\n",
        [SEEKS_CUES_LAST_ENTRY],
    ),
    (
        "Cues are used after following the SeekHead has failed",
        "                    cues_broken = true;\n",
        "",
        [SEEKS_CUES_BROKEN],
    ),
    (
        "a second Tracks before the Clusters is not read",
        "                    ids::TRACKS => self.read_tracks(&h)?,",
        "                    ids::TRACKS if self.tracks.is_empty() => self.read_tracks(&h)?,\n"
        "                    ids::TRACKS => {}",
        [META_TWO_TRACKS_PACKETS, META_TWO_TRACKS],
    ),
    (
        "Tags at a second position are not read",
        "let by_position = id == ids::SEEK_HEAD || id == ids::TAGS;",
        "let by_position = id == ids::SEEK_HEAD;",
        [META_SEEK_HEAD],
    ),
    (
        "Chapters already read are read again where the SeekHead points",
        "let by_position = id == ids::SEEK_HEAD || id == ids::TAGS;",
        "let by_position = true;",
        [META_SEEK_HEAD],
    ),
    (
        "after damage before the first Cluster, the Segment's stated end still bounds it",
        "                                        segment_ends = None;\n",
        "",
        ["after_damage_the_segment_runs_to_the_end_of_the_file"],
    ),
    (
        "damage before the first Cluster refuses the file",
        "                            Err(_) => {\n"
        "                                // Past its four-byte ID, and one more.",
        "                            Err(e) => {\n"
        "                                return Err(e);\n"
        "                                // Past its four-byte ID, and one more.",
        [META_DAMAGE],
    ),
    # Reading the Clusters.
    (
        "a Cluster of unknown size runs on into the next",
        "                if end.is_none()\n",
        "                if end.is_none() && false\n",
        [UNKNOWN_CLUSTERS, UNKNOWN_SIZES],
    ),
    (
        "an element may run past its Cluster",
        "if end.is_some_and(|e| child_end > e) || child_end > self.r.len() {",
        "if child_end > self.r.len() {",
        [OVERRUN],
    ),
    (
        "a Cluster's timestamp is ignored",
        "let t = self.r.uint(h.size, 0)?;",
        "let t = self.r.uint(h.size, 0)? * 0;",
        [ORDER],
    ),
    (
        "a SimpleBlock of no bytes is read",
        "let data = self.r.binary(h.size, MAX_BINARY)?;\n                        if !data.is_empty() {",
        "let data = self.r.binary(h.size, MAX_BINARY)?;\n                        if true {",
        [EMPTY_FRAMES],
    ),
    (
        "a BlockGroup's duration is not read",
        "ids::BLOCK_DURATION => g.duration = r.uint(c.size, 0)?,",
        "ids::BLOCK_DURATION => {\n                    r.uint(c.size, 0)?;\n                }",
        [GROUPS, UNKNOWN_SIZES, SUBTITLES],
    ),
    (
        "BlockAdditions are dropped",
        "                    g.additions.push((id, bytes));",
        "                    let _ = (id, bytes);",
        [VP9_ALPHA, EMPTY_FRAMES],
    ),
    # A block's frames.
    (
        "a block of an undeclared track is passed over",
        "return Err(Error::Invalid(\"a block for a track that is not declared\"));",
        "return Ok(());",
        [IGNORED_TRACKS],
    ),
    (
        "a block of an ignored track is damage",
        "            // A track FFmpeg ignores: so are its blocks.\n            return Ok(());",
        "            return Err(Error::Invalid(\"ignored\"));",
        # Not `unknown_sizes`: its ignored track's block ends its Cluster, so
        # the resync loses nothing.
        [IGNORED_TRACKS],
    ),
    (
        "a SimpleBlock's key flag is ignored",
        "None => (flags & 0x80 != 0, 0, 0, Vec::new()),",
        "None => (false, 0, 0, Vec::new()),",
        [ORDER, LACINGS],
    ),
    (
        "a BlockGroup with a reference is a key frame",
        "                g.references == 0,",
        "                true,",
        [GROUPS, UNKNOWN_SIZES],
    ),
    (
        "overlapping subtitles are key frames",
        "            if t < ended {\n                keyframe = false;\n            }\n",
        "",
        [SUBTITLES],
    ),
    (
        "a codec delay is not taken off the timestamps",
        "            .wrapping_sub(timing.delay)\n",
        "",
        [CODEC_DELAY, DELAYED, VP9_OPUS],
    ),
    (
        "a block's default duration is not times its frames",
        "                .wrapping_mul(laces_u64)\n",
        "",
        [LACINGS, LACED],
    ),
    (
        "a lace lasts the whole block",
        "            let lace_duration = mul_div(duration, n_u64.saturating_add(1), laces_u64)\n"
        "                .wrapping_sub(mul_div(duration, n_u64, laces_u64));",
        "            let lace_duration = duration;",
        [LACINGS, LACED],
    ),
    (
        "laced frames share the first one's time",
        "Some(t) if lace_duration != 0 => Some(t.wrapping_add(lace_duration.cast_signed())),",
        "Some(t) if lace_duration != 0 => Some(t),",
        [LACINGS, LACED],
    ),
    (
        "only a key block's first frame is a key frame, as before FFmpeg 7.1",
        "                    keyframe,\n                    data: frame,",
        "                    keyframe: keyframe && n == 0,\n                    data: frame,",
        [LACINGS, LACED],
    ),
    (
        "an empty frame is a packet",
        "if !(frame.is_empty() && additions.is_empty()) {",
        "if true {",
        [EMPTY_FRAMES],
    ),
    (
        "an empty frame with additions is no packet",
        "if !(frame.is_empty() && additions.is_empty()) {",
        "if !frame.is_empty() {",
        [EMPTY_FRAMES],
    ),
    (
        "empty additions are given out",
        ".filter(|(_, bytes)| !bytes.is_empty())",
        ".filter(|_| true)",
        [EMPTY_FRAMES],
    ),
    # Damage.
    (
        "damage ends the file",
        "if filled >= 4 && ids::is_top_level(window) {",
        "if false {",
        [RESYNC, DAMAGED, BAD_LACES, OVERRUN],
    ),
    (
        "the frames a damaged block gave before its damage are dropped",
        "    fn resync(&mut self) -> Result<bool, Error> {\n",
        "    fn resync(&mut self) -> Result<bool, Error> {\n        self.queue.clear();\n",
        [ZLIB_LACES],
    ),
    (
        "an element FFmpeg does not know counts as good",
        "        _ => return false,",
        "        _ => u64::MAX,",
        [RESYNC_POINT],
    ),
    (
        "a BlockGroup's own elements do not count as good",
        "            if counts(Level::Group, c) {\n                *last_good = c.start;\n            }\n",
        "",
        [RESYNC_IN_GROUP],
    ),
    (
        "Cues met while reading are noted",
        "                // Anything else is passed over -- Cues too: FFmpeg reads Cues\n"
        "                // only before the first Cluster or where the SeekHead points.\n",
        "                if h.id == ids::CUES {\n"
        "                    self.cues_at.push(h.start);\n"
        "                }\n",
        [CUES_MET],
    ),
    # Seeking.
    (
        "a seek uses Cues of other tracks",
        ".cues()?\n            .iter()\n            .filter(|c| c.track == track)",
        ".cues()?\n            .iter()\n            .filter(|_| true)",
        [SEEKS_OTHER_CUES],
    ),
    (
        "a seek in a track without Cues does not walk",
        "        if entries.is_empty() {\n            // No Cues",
        "        if false {\n            // No Cues",
        [SEEKS_UNCUED, SEEKS_OTHER_CUES, SEEKS_LIVE],
    ),
    (
        "a seek walks though the track has Cues",
        "        if entries.is_empty() {\n            // No Cues",
        "        if true {\n            // No Cues",
        [SPARSE],
    ),
    (
        "a seek goes to the first entry at or before the time, not the last",
        "            .rev()\n            .find(|c| c.time <= target)",
        "            .find(|c| c.time <= target)",
        [SEEKS_CUED, SEEKS_UNCUED],
    ),
    (
        "after a seek, packets before the key frame's time are given out",
        "                if timestamp.is_none_or(|t| t < until) {\n                    return Ok(());\n                }\n",
        "",
        [SEEKS_VP9_OPUS],
    ),
    (
        "after a seek, another track's frame that is not a key frame does not end the dropping",
        "                if keyframe || self.skip.key_for != Some(number) {",
        "                if keyframe {",
        [SEEKS_REORDERED],
    ),
    (
        "after a seek, the seek's track does not wait for its key frame",
        "            if self.skip.key_for == Some(number) {",
        "            if false && self.skip.key_for == Some(number) {",
        [SEEKS_REORDERED],
    ),
    (
        "a walk takes a key frame only once its laces are read",
        "            if keyframe && let Some(time) = timestamp {\n                found.push(Cue {",
        "            if keyframe && block::parse(data).is_ok() && let Some(time) = timestamp {\n                found.push(Cue {",
        [SEEKS_WALK_DAMAGE],
    ),
    (
        "a walk reads the blocks' headers alone",
        "                    cluster,\n                });\n            }\n        } else {",
        "                    cluster,\n                });\n            }\n            return Ok(());\n        } else {",
        [SEEKS_WALK_DAMAGE, SEEKS_ZLIB_LACES],
    ),
    (
        "a walk does not undo the frames' encoding",
        "            let frame = track.encoding.undo(stored)?.into_owned();\n"
        "            if walking {\n"
        "                continue;\n"
        "            }\n",
        "            if walking {\n"
        "                continue;\n"
        "            }\n"
        "            let frame = track.encoding.undo(stored)?.into_owned();\n",
        [SEEKS_ZLIB_LACES],
    ),
    (
        "a walk does not stop at a key frame past the time",
        "            if found.is_some_and(past) {\n                break;\n            }",
        "            let _ = found;",
        [WALK],
    ),
    (
        "a walk walks again though it has passed the time",
        "        if self.walk.found.iter().any(past) {\n            return Ok(());\n        }\n",
        "",
        [WALK],
    ),
    (
        "a walk begins again at the start each time",
        "            Ok(()) => Some(self.place()),",
        "            Ok(()) => self.walk.start,",
        [WALK],
    ),
    (
        "a seek that fails leaves reading where its walk stopped",
        "        self.go_to(reading)?;\n        result",
        "        result",
        [NO_KEYS],
    ),
]

METADATA = [
    # FFmpeg's dictionary.
    (
        "a key set again keeps its place",
        "            self.entries.swap_remove(i);",
        "            self.entries.remove(i);",
        [META_TAGS, "setting_a_key_moves_the_last_entry_into_its_place"],
    ),
    (
        "a key is matched with its case",
        ".filter(|(_, (k, _))| k.eq_ignore_ascii_case(key))",
        ".filter(|(_, (k, _))| k == key)",
        [META_TAGS, META_ATTACHMENTS, META_TRAILING],
    ),
    (
        "nothing is renamed",
        "            let k = renamed(&k).map_or(k, <[u8]>::to_vec);",
        "",
        [META_TAGS, "renaming_keeps_both_of_two_values_and_drops_a_repeat"],
    ),
    (
        "renaming drops a second value of a key",
        "            if seen.insert((k.to_ascii_lowercase(), v.clone())) {",
        "            if seen.insert((k.to_ascii_lowercase(), Vec::new())) {",
        [META_TAGS, "renaming_keeps_both_of_two_values_and_drops_a_repeat"],
    ),
    (
        "the renaming pass is skipped after a key it renames",
        "        if matches.next().is_some() || renamed(key).is_some() {",
        "        if matches.next().is_some() {",
        [META_TAGS, "renaming_keeps_both_of_two_values_and_drops_a_repeat"],
    ),
    # Tags.
    (
        "a tag in a language is set under its plain name too",
        "        if t.default != 0 || lang.is_none() {",
        "        if true {",
        [META_TAGS, "a_tag_is_keyed_by_language_prefix_and_nesting"],
    ),
    (
        "the default flag is ignored",
        "        if t.default != 0 || lang.is_none() {",
        "        if lang.is_none() {",
        [META_TAGS, META_TRAILING, "a_tag_is_keyed_by_language_prefix_and_nesting"],
    ),
    (
        "a nested tag's key leaves out its parent's",
        "                convert_tag(&t.sub, metadata, Some(&key), budget);\n"
        "            }\n"
        "        }\n"
        "        if let Some(lang) = lang {",
        "                convert_tag(&t.sub, metadata, None, budget);\n"
        "            }\n"
        "        }\n"
        "        if let Some(lang) = lang {",
        [META_TAGS, META_TRAILING, "a_tag_is_keyed_by_language_prefix_and_nesting"],
    ),
    (
        "a key is not cut at FFmpeg's buffer",
        "            None => name.iter().take(MAX_KEY).copied().collect(),",
        "            None => name.to_vec(),",
        [META_TAGS, "a_key_stops_at_ffmpeg_s_buffer"],
    ),
    (
        "a key behind a prefix is not cut at FFmpeg's buffer",
        "                .take(MAX_KEY)\n",
        "",
        ["a_key_stops_at_ffmpeg_s_buffer"],
    ),
    (
        "the TargetType is no prefix",
        "convert_tag(&t.tags, &mut file, target.kind.as_deref(), &mut budget);",
        "convert_tag(&t.tags, &mut file, None, &mut budget);",
        [META_TAGS, META_TRAILING],
    ),
    (
        "a tag naming a track goes to the file",
        "        } else if target.track != 0 {",
        "        } else if false {",
        [META_TAGS, META_TWO_TRACKS, META_FFMPEG],
    ),
    (
        "a tag naming a chapter and a track goes to the track",
        "        } else if target.chapter != 0 {",
        "        } else if target.chapter != 0 && target.track == 0 {",
        [META_TAGS],
    ),
    (
        "a tag naming an attachment goes to the file",
        "        if target.attachment != 0 {",
        "        if false {",
        [META_ATTACHMENTS, META_TRAILING],
    ),
    # The tracks' own metadata.
    (
        "a track's language `und` is shown",
        "    if t.language != b\"und\" {",
        "    if true {",
        [META_TAGS],
    ),
    (
        "a stereo mode FFmpeg does not name is shown",
        "        if v.stereo_mode != 0\n",
        "        if true\n",
        [META_TAGS],
    ),
    # Chapters.
    (
        "chapters start in any order",
        "if c.start != NOPTS && c.uid != 0 && (max_start == 0 || c.start > max_start) {",
        "if c.start != NOPTS && c.uid != 0 {",
        [META_CHAPTERS],
    ),
    (
        "a chapter ending before it starts is kept",
        "    if given && start > end {",
        "    if false {",
        [META_CHAPTERS],
    ),
    (
        "a chapter left out does not count as started",
        "            chapter = new_chapter(&mut chapters, &mut by_uid, c, &mut budget);\n"
        "            max_start = c.start;",
        "            chapter = new_chapter(&mut chapters, &mut by_uid, c, &mut budget);\n"
        "            if chapter.is_some() {\n"
        "                max_start = c.start;\n"
        "            }",
        [META_CHAPTERS],
    ),
    (
        "a chapter's UID seen again makes another chapter",
        "    let i = *by_uid.entry(atom.uid).or_insert_with(|| {",
        "    let fresh = atom.uid ^ (chapters.len() as u64) << 32;\n"
        "    let i = *by_uid.entry(fresh).or_insert_with(|| {",
        [META_CHAPTERS],
    ),
    (
        "a chapter's end is the file's though another starts before",
        "            && next > c.start\n            && next < end\n",
        "            && false\n",
        [META_CHAPTERS, META_CUT, "a_chapter_without_an_end_ends_where_the_next_starts_or_the_file_ends"],
    ),
    (
        "chapters end in the file's order, not their starts'",
        "    order.sort_by(|&a, &b| start_of(a).cmp(&start_of(b)).then(uid(a).cmp(&uid(b))));",
        "",
        [META_CHAPTERS, "a_chapter_without_an_end_ends_where_the_next_starts_or_the_file_ends"],
    ),
    # Attachments.
    (
        "an attachment of no bytes is kept",
        "        if size == 0 {\n            continue;\n        }\n",
        "",
        [META_ATTACHMENTS],
    ),
    (
        "a media type is matched whole",
        "            .find(|(name, _)| media_type.starts_with(name))",
        "            .find(|(name, _)| media_type == *name)",
        [META_ATTACHMENTS, "a_media_type_is_known_by_its_start"],
    ),
    (
        "an attachment's description is not its title",
        "            metadata.set(b\"title\", Some(d), &mut budget);",
        "            let _ = d;",
        [META_FFMPEG, META_TRAILING],
    ),
    # The file's.
    (
        "the muxing application is not the encoder",
        "    file.set(b\"encoder\", info.muxing_app.as_deref(), &mut budget);",
        "",
        [META_FFMPEG, META_INFO, META_DATE],
    ),
    (
        "creation_time's fraction is the remainder's absolute value",
        "    let fraction = micros % 1_000_000;",
        "    let fraction = (micros % 1_000_000).abs();",
        ["creation_time_is_ffmpeg_s_down_to_its_quirks"],
    ),
]

NEST = [
    # The Info.
    (
        "the timestamp scale is a millisecond whatever the file says",
        "            ids::TIMESTAMP_SCALE => uint(r, &c, 1_000_000, &mut info.timestamp_scale)?,",
        "            ids::TIMESTAMP_SCALE => skip(r, &c)?,",
        [TICK, SEGMENT],
    ),
    (
        "a second Info does not start its numbers afresh",
        "    info.timestamp_scale = 1_000_000;\n    info.duration = None;\n",
        "",
        [META_INFO_PACKETS],
    ),
    (
        "an empty title is none",
        "            ids::TITLE => string(r, &c, None, &mut info.title)?,",
        "            ids::TITLE => {\n"
        "                string(r, &c, None, &mut info.title)?;\n"
        "                if info.title.as_ref().is_some_and(Vec::is_empty) {\n"
        "                    info.title = None;\n"
        "                }\n"
        "            }",
        [META_DATE],
    ),
    (
        "a DateUTC of another size is a date",
        "    } else {\n        *out = None;\n        r.seek_to(h.data.saturating_add(n))?;\n",
        "    } else {\n        r.seek_to(h.data.saturating_add(n))?;\n",
        [META_INFO],
    ),
    (
        "a float cut short is not kept",
        "    *out = Some(if n == 4 {",
        "    if short {\n        return Err(Error::Truncated);\n    }\n    *out = Some(if n == 4 {",
        ["a_float_is_four_or_eight_bytes_and_kept_as_far_as_read"],
    ),
    (
        "a number cut short is not kept",
        "    *out = v;\n    if short {",
        "    if !short {\n        *out = v;\n    }\n    if short {",
        [META_CUT, "a_number_cut_short_keeps_what_was_read"],
    ),
    (
        "a string cut short empties its field",
        "    if n > r.remaining() {\n        return Err(Error::Truncated);\n    }\n    let mut s",
        "    if n > r.remaining() {\n        *out = None;\n        return Err(Error::Truncated);\n    }\n    let mut s",
        [META_CUT, "a_string_is_empty_when_empty_and_untouched_when_cut"],
    ),
    (
        "an empty string is no string",
        "    *out = Some(s);\n    Ok(())",
        "    *out = (!s.is_empty()).then_some(s);\n    Ok(())",
        [META_TAGS, META_ATTACHMENTS, "a_string_is_empty_when_empty_and_untouched_when_cut"],
    ),
    (
        "masters nest as deep as the file goes",
        "    if levels >= MAX_LEVELS {",
        "    if false {",
        [META_DEEP, "tags_nest_only_as_deep_as_ffmpeg_reads"],
    ),
    (
        "nested chapters are read",
        "            // A nested atom too: FFmpeg reads only the top level.\n"
        "            _ => skip(r, &f)?,",
        "            ids::CHAPTER_ATOM => {\n"
        "                let mut nested = RawChapter::default();\n"
        "                read_atom(r, &f, levels, &mut nested)?;\n"
        "                *c = nested;\n"
        "            }\n"
        "            _ => skip(r, &f)?,",
        [META_CHAPTERS, "chapters_come_from_every_edition_and_only_their_top_level"],
    ),
    (
        "only the first edition's chapters are read",
        "        let in_edition = enter(levels)?;\n",
        "        if !out.is_empty() {\n            skip(r, &edition)?;\n            continue;\n        }\n"
        "        let in_edition = enter(levels)?;\n",
        [META_CHAPTERS, "chapters_come_from_every_edition_and_only_their_top_level"],
    ),
    (
        "the first ChapString is kept",
        "                        string(r, &d, None, &mut c.title)?;",
        "                        if c.title.is_none() {\n"
        "                            string(r, &d, None, &mut c.title)?;\n"
        "                        } else {\n"
        "                            skip(r, &d)?;\n"
        "                        }",
        [META_CHAPTERS, META_CUT],
    ),
    (
        "a second Targets keeps the first's UIDs",
        "    target.track = 0;\n    target.chapter = 0;\n    target.attachment = 0;\n",
        "",
        [META_TAGS],
    ),
    (
        # (A SimpleTag without a TagLanguage reads as `und` in FFmpeg, as none
        # would: the two are the same to the conversion, so that default is
        # no row. An empty TagLanguage is, being a language of its own.)
        "an empty TagLanguage is a language, not undetermined",
        "            ids::TAG_LANGUAGE => string(r, &f, Some(b\"und\"), &mut tag.lang)?,",
        "            ids::TAG_LANGUAGE => string(r, &f, None, &mut tag.lang)?,",
        [META_TAGS],
    ),
    (
        "the misspelt TagDefault is not read",
        "            ids::TAG_DEFAULT | ids::TAG_DEFAULT_BOGUS => uint(r, &f, 0, &mut tag.default)?,",
        "            ids::TAG_DEFAULT => uint(r, &f, 0, &mut tag.default)?,",
        [META_TAGS],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [
        (SRC / "block.rs", BLOCK),
        (SRC / "ebml.rs", EBML),
        (SRC / "track.rs", TRACK),
        (SRC / "cues.rs", CUES),
        (SRC / "demux.rs", DEMUX),
        (SRC / "metadata.rs", METADATA),
        (SRC / "nest.rs", NEST),
    ]
    names = [name for _, rows in tables for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    results = [0]
    for src, rows in tables:
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        results.append(sweep(src, rows, "matroska", timeout=300, only=mine or None))
    raise SystemExit(max(results))
