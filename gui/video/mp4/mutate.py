"""Mutation test for the mp4 crate.

Each row puts back one way of not reading MP4 as FFmpeg reads it -- an edit
list's rule skipped, a table misread, a box's damage rule dropped, a picture's
description read another way -- and names the tests that have to notice.
Nearly all are the fixtures' tests, which hold every packet, every seek and
every track's description to what ffprobe makes of the same file
(`tests/fixtures.rs`, `tests/data/generate_fixtures.py`); the rest are the
modules' own.

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


def packets(name):
    """The fixture test holding `name`'s packets and description."""
    return f"packets_of_{name}"


def seeks(name):
    """The test holding the packets after `name`'s seeks."""
    return f"seeks_in_{name}"


# One track read alone (`Demuxer::select_tracks`), and the read-ahead.
ALONE = "a_track_selected_alone_gives_the_packets_it_gives_among_the_others"
ALONE_READS_OWN = "a_track_selected_alone_reads_its_own_samples_alone"

# FFmpeg's walks over an index, held to the walks as FFmpeg writes them.
SEARCH_IS_FFMPEGS = "the_search_answers_as_ffmpegs_walking_search_does"
EDIT_SEARCH_IS_FFMPEGS = "an_edits_search_answers_as_ffmpegs_walking_one_does"

# The room the file's length leaves the indexes, and FFmpeg's ceilings.
ROOM = "a_track_takes_no_more_entries_than_the_room_left"
FILE_ROOM = "a_files_tracks_take_no_more_entries_than_it_has_bytes"
CUT_EDITED = "an_index_held_to_the_room_is_edited_as_a_whole_one_would_be"


INDEX = [
    (
        "PCM is not read in chunks",
        "        let chunked = self.kind == Kind::Audio\n",
        "        let chunked = false && self.kind == Kind::Audio\n",
        [packets("pcm"), seeks("pcm")],
    ),
    (
        "stss's numbers are read from 0",
        "        let key_off = u32::from(\n",
        "        let key_off = 0 * u32::from(\n",
        [packets("av1"), packets("h264_bframes")],
    ),
    (
        "an stss of no entries makes every sample a key frame",
        "                if !self.keyframe_absent\n",
        "                if true\n",
        [packets("empty_stss")],
    ),
    (
        "partial sync samples are not key frames",
        "                } else if !self.stps.is_empty()\n",
        "                } else if false && !self.stps.is_empty()\n",
        [packets("stps"), seeks("stps")],
    ),
    (
        "a 'rap ' sample group makes no key frames",
        "                    if group > 0 {\n                        keyframe = true;\n                    }",
        "                    let _ = group;",
        [packets("rap_group"), seeks("rap_group")],
    ),
    (
        "samples of another sample entry are kept",
        "                if self.pseudo_stream_id == -1\n",
        "                if true || self.pseudo_stream_id == -1\n",
        [packets("two_codecs")],
    ),
    (
        "every sample lasts as long as the first",
        "                let step = self.tts.get(stts_index).map_or(0, |t| t.duration);",
        "                let step = self.tts.first().map_or(0, |t| t.duration);",
        [packets("stts_negative")],
    ),
    (
        "a search does not step over discarded entries",
        "        if at(m).is_some_and(|e| e.flags & DISCARD != 0) && m < b && m < nb - 1 {",
        "        if false && at(m).is_some_and(|e| e.flags & DISCARD != 0) && m < b && m < nb - 1 {",
        ["the_search_steps_over_discarded_entries_as_ffmpeg_does", seeks("two_edits"), SEARCH_IS_FFMPEGS],
    ),
    (
        "a backward search does not go back to a key frame",
        "            m = if backward {\n                walks\n                    .key_to(from)",
        "            m = if false {\n                walks\n                    .key_to(from)",
        ["the_search_finds_key_frames_backward_and_forward", seeks("av1"), SEARCH_IS_FFMPEGS],
    ),
    # The walks in one step (`Walks`, `Offsets`): each held to FFmpeg's
    # walks an entry at a time, on random indexes, with the tables built at
    # once and partway through. The bound itself, SHORT_WALK, has no row:
    # unbounded, the scale tests do not fail, they run for hours, and the
    # harness would wait out its timeout three times to call that caught.
    (
        "the step over discarded entries ignores the upper bound's time",
        "            m = if stop == b && at(b).is_some_and(|e| e.timestamp >= wanted) {",
        "            m = if false && stop == b && at(b).is_some_and(|e| e.timestamp >= wanted) {",
        ["the_search_steps_over_discarded_entries_as_ffmpeg_does", SEARCH_IS_FFMPEGS],
    ),
    (
        "the step over discarded entries does not stop at the last",
        "            let stop = kept.min(b).min(nb - 1);",
        "            let stop = kept.min(b);",
        [SEARCH_IS_FFMPEGS],
    ),
    (
        "the table of the next kept entry is one off",
        "            next = u32::try_from(i).ok()?;",
        "            next = u32::try_from(i + 1).ok()?;",
        [SEARCH_IS_FFMPEGS, EDIT_SEARCH_IS_FFMPEGS],
    ),
    (
        "the table of the last key frame says the one after it",
        "            last = u32::try_from(i).ok()?.checked_add(1)?;",
        "            last = u32::try_from(i).ok()?.checked_add(2)?;",
        [SEARCH_IS_FFMPEGS, EDIT_SEARCH_IS_FFMPEGS],
    ),
    (
        "an edit's search goes back over no entries of its time",
        "                Some(before) if time(before) == time(i) => i = before,",
        "                Some(before) if false && time(before) == time(i) => i = before,",
        [EDIT_SEARCH_IS_FFMPEGS],
    ),
    (
        "an edit's search back over its time takes no key frame",
        "        let earliest = if any { start } else { walks.key_from(start) };",
        "        let earliest = if any { start } else { found };",
        [EDIT_SEARCH_IS_FFMPEGS],
    ),
    (
        "an entry past a run of no samples is in no run",
        "            return if self.stuck {",
        "            return if false {",
        [EDIT_SEARCH_IS_FFMPEGS],
    ),
    (
        "a long walk back finds the first key frame shown in time",
        "        self.descend(node * 2 + 1, mid, high, to, pts)\n            .or_else(|| self.descend(node * 2, low, mid, to, pts))",
        "        self.descend(node * 2, low, mid, to, pts)\n            .or_else(|| self.descend(node * 2 + 1, mid, high, to, pts))",
        [EDIT_SEARCH_IS_FFMPEGS],
    ),
    (
        "a long walk back takes a frame as shown at its decoding time",
        "                *leaf = Some(e.timestamp.wrapping_add(i64::from(t.offset)));",
        "                *leaf = Some(e.timestamp);",
        [EDIT_SEARCH_IS_FFMPEGS],
    ),
    (
        "an edit's dropped frames count back by the durations an edit before left",
        "                buffer.clear();",
        "                let _ = buffer;",
        [packets("edits_stale_discards"), "an_edits_discarded_frames_count_back_by_their_own_durations"],
    ),
    (
        "an edit's first key frame ignores the composition offsets",
        "        if self.has_ctts {\n            (*tts_index, *tts_sample) = offsets.position(index);",
        "        if false {\n            (*tts_index, *tts_sample) = offsets.position(index);",
        [packets("edit_before_key_shows"), packets("edit_at_shown_key"), EDIT_SEARCH_IS_FFMPEGS],
    ),
    (
        "sound's edit is not searched a second early",
        "                search = (search - i64::from(self.time_scale)).max(first_ts);",
        "                let _ = first_ts;",
        [packets("aac"), packets("sound_edit_mid_frame")],
    ),
    (
        "frames outside an edit are not marked discarded",
        "                        flags |= DISCARD;",
        "                        let _ = flags;",
        [packets("edit_mid_gop"), packets("two_edits")],
    ),
    (
        "no key frame after an edit is kept for its B-frames",
        "                        if !audio && !found_keyframe_after_edit {",
        "                        if false && !audio && !found_keyframe_after_edit {",
        [packets("mpeg4_bframes"), seeks("mpeg4_bframes")],
    ),
    (
        "a video's earliest time is not moved to 0",
        "        if self.kind == Kind::Video && self.min_corrected_pts > 0 {",
        "        if false && self.kind == Kind::Video && self.min_corrected_pts > 0 {",
        [packets("h264_bframes"), packets("mpeg4_bframes")],
    ),
    (
        "the duration is not cut to the edit list",
        "        self.duration = self.duration.min(edit_list_dts_entry_end - start_dts);",
        "        let _ = start_dts;",
        [packets("two_edits"), packets("edit_mid_gop")],
    ),
    (
        "sound's priming is not skipped",
        "        self.start_pad = self.skip_samples;",
        "        self.start_pad = 0;",
        [packets("aac"), seeks("vp9_opus")],
    ),
    (
        "a track's index takes nothing from the room",
        "        *room = room.saturating_sub(taken);",
        "        let _ = taken;",
        [ROOM, FILE_ROOM],
    ),
    (
        "an index is not held to the room",
        "        let samples = self.sample_count.min(limit);",
        "        let samples = self.sample_count;",
        [ROOM, FILE_ROOM],
    ),
    (
        "an index held to the room is left unedited",
        "                if current_sample >= samples {\n                    // The rest",
        "                if current_sample >= samples {\n                    finished = false;\n                    // The rest",
        [CUT_EDITED],
    ),
    (
        "an index FFmpeg cannot allocate is built",
        "        if self.sample_count > INDEX_ALLOC {",
        "        if false && self.sample_count > INDEX_ALLOC {",
        [packets("claims_past_ffmpeg_index"), "an_index_ffmpeg_cannot_allocate_is_none"],
    ),
    (
        "times are spread out past FFmpeg's ceiling",
        "        if self.sample_count == 0 || self.sample_count >= TTS_LIMIT {",
        "        if self.sample_count == 0 {",
        [packets("chunked_at_ffmpeg_tts_limit")],
    ),
    (
        "times are spread out past what FFmpeg can allocate",
        "        if (ctts || stts) && self.sample_count > TTS_ALLOC {",
        "        if false && (ctts || stts) && self.sample_count > TTS_ALLOC {",
        [packets("chunked_past_ffmpeg_tts")],
    ),
    (
        "a table of times is allocated though nothing is spread into it",
        "        let mut tts = vec![Tts::default(); len];",
        "        let mut tts = vec![Tts::default(); if ctts || stts { len } else { samples }];",
        ["sound_in_chunks_keeps_no_table_of_times_it_does_not_merge"],
    ),
    (
        "times are spread out past the room",
        "        let samples = usize::try_from(self.sample_count.min(limit)).unwrap_or(0);",
        "        let samples = usize::try_from(self.sample_count).unwrap_or(0);",
        [ROOM],
    ),
    (
        "sound in chunks is indexed past what FFmpeg can allocate",
        "        if total > INDEX_ALLOC {",
        "        if false && total > INDEX_ALLOC {",
        ["packets_ffmpeg_cannot_index_in_chunks_are_none"],
    ),
    (
        "the count of sound's packets does not wrap as FFmpeg's",
        "            total = total.wrapping_add(chunk_count.wrapping_mul(count));",
        "            total = total.saturating_add(chunk_count.saturating_mul(count));",
        [packets("chunked_total_wraps")],
    ),
    (
        "sound in chunks is not held to the room",
        "                if self.index.len() >= packets {",
        "                if false && self.index.len() >= packets {",
        [ROOM],
    ),
    (
        "sound in chunks held to the room is left unedited",
        "                if self.index.len() >= packets {\n                    // The rest",
        "                if self.index.len() >= packets {\n                    finished = false;\n                    // The rest",
        [CUT_EDITED],
    ),
    (
        "a frame count left below zero ends its chunk",
        "                chunk_samples = chunk_samples.wrapping_sub(samples);",
        "                chunk_samples = chunk_samples.saturating_sub(samples);",
        [packets("chunked_misaligned_frames")],
    ),
    (
        "edits giving samples again take more than the room",
        "                if self.index.len() >= room {",
        "                if false && self.index.len() >= room {",
        ["edits_giving_samples_again_take_no_more_than_the_room"],
    ),
]

PARSE = [
    (
        "a 64-bit box size is not read",
        "            if size == 1 && total + 8 <= parent_size {",
        "            if false && size == 1 && total + 8 <= parent_size {",
        [packets("largesize")],
    ),
    (
        "a box of size 0 does not run to its parent's end",
        "            if size == 0 {\n                size = parent_size - total + 8;\n            }",
        "            let _ = parent_size;",
        [packets("moov_to_end"), packets("trak_to_end")],
    ),
    (
        "a moov written as hoov is not read",
        "            if ((kind == *b\"free\" && self.moov_retry) || kind == *b\"hoov\") && size >= 8 {",
        "            if (kind == *b\"free\" && self.moov_retry) && size >= 8 {",
        [packets("hoov")],
    ),
    (
        "a large stts delta is not read as a correction",
        "            if delta > MAX_STTS_DELTA {",
        "            if false && delta > MAX_STTS_DELTA {",
        [packets("stts_negative")],
    ),
    (
        "every ctts entry shifts the decoding times",
        "            if u64::from(i).saturating_add(2) < u64::from(entries) {",
        "            if u64::from(i) < u64::from(entries) {",
        [packets("ctts_tail")],
    ),
    (
        "an stsc is not repaired",
        "        repair_stsc(&mut stsc);",
        "        let _ = &mut stsc;",
        # The repair's own unit test calls it directly: only a file's notices
        # its call gone.
        [packets("stsc_repair")],
    ),
    (
        "a fragment's base is never its moof",
        "        } else if flags & TFHD_DEFAULT_BASE_IS_MOOF != 0 {",
        "        } else if false && flags & TFHD_DEFAULT_BASE_IS_MOOF != 0 {",
        [packets("fragmented_moof_base")],
    ),
    (
        "a run's data offset is not read",
        "        let data_offset = if flags & TRUN_DATA_OFFSET != 0 {\n            self.r.u32()?.cast_signed()",
        "        let data_offset = if flags & TRUN_DATA_OFFSET != 0 {\n            self.r.u32()?.cast_signed() * 0",
        [packets("fragmented"), packets("fragmented_moof_base")],
    ),
    (
        "nclx's range is not read",
        "                    Some(self.r.u8()? >> 7 != 0)",
        "                    self.r.u8().map(|_| None)?",
        [packets("colr_nclx"), packets("colr_rare_codes")],
    ),
    (
        "nclc's silence on the range clears vpcC's",
        "                        full_range: range.or(before),",
        "                        full_range: range,",
        [packets("colr_nclc_after_vpcc")],
    ),
    (
        "a code point FFmpeg has no name for is kept",
        "        0..=12 | 22 | 256 => primaries,",
        "        _ if true => primaries,",
        # ffprobe prints "unknown" for an unspecified code point and for one
        # it has no name for alike, so no fixture's answer tells them apart.
        ["code_points_without_a_name_in_ffmpeg_are_unspecified"],
    ),
    (
        "vpcC's initialization data is not refused",
        "            if self.r.u16()? != 0 {",
        "            if self.r.u16()? != 0 && false {",
        [packets("vpcc_init_data")],
    ),
    (
        "a short vpcC is not refused",
        "        if a.size < 5 {",
        "        if a.size < 0 {",
        [packets("vpcc_short")],
    ),
    (
        "a vpcC of another version is read",
        "        if self.r.u8()? == 1 {",
        "        if self.r.u8()? <= 1 {",
        [packets("vpcc_version_0")],
    ),
    (
        "a pasp with no vertical spacing is kept",
        "        if let Some((_, d)) = self.last()\n            && v != 0\n        {\n            d.pasp = Some((h, v));",
        "        if let Some((_, d)) = self.last() {\n            d.pasp = Some((h, v.max(1)));",
        [packets("pasp_no_vertical")],
    ),
    (
        "pasp's spacings are not reduced",
        "        d.sample_aspect = Some(rational::reduce(i64::from(h), i64::from(v), rational::MAX).0);",
        "        d.sample_aspect = Some(Q { num: h, den: v });",
        [packets("pasp_reduced")],
    ),
    (
        "tkhd's size against the picture's gives no shape",
        "    let unsaid = d.sample_aspect.is_none_or(|q| q.num == 0);",
        "    let unsaid = false && d.sample_aspect.is_none_or(|q| q.num == 0);",
        [packets("tkhd_size"), packets("pasp_no_horizontal")],
    ),
    (
        "a stretching matrix gives no shape",
        "                d.sample_aspect = Some(shape);",
        "                let _ = shape;",
        [packets("matrix_stretch")],
    ),
    (
        "the movie's matrix is not applied",
        "        let matrix = display_matrix(own, self.movie_matrix);",
        "        let matrix = display_matrix(own, [[1 << 16, 0, 0], [0, 1 << 16, 0], [0, 0, 1 << 30]]);",
        [packets("movie_rotation"), packets("both_rotations")],
    ),
    (
        "an aperture wider than the picture is taken",
        "    if cmp(whole(width), aperture_w) < 0 || cmp(whole(height), aperture_h) < 0 {",
        "    if false && cmp(whole(width), aperture_w) < 0 || cmp(whole(height), aperture_h) < 0 {",
        # clap_too_wide's edges fall outside the picture too, which refuses it
        # without this check.
        [packets("clap_wider_by_half")],
    ),
    (
        "an invalid clap clears an earlier crop",
        "            d.crop = Some(crop);\n        }\n        Ok(())",
        "            d.crop = Some(crop);\n        } else if let Some((_, d)) = self.last() {\n            d.crop = None;\n        }\n        Ok(())",
        [packets("clap_twice")],
    ),
    (
        "a frame rate is found where FFmpeg finds none",
        "            d.frame_duration = frame_duration(sc);",
        "            d.frame_duration = sc.tts.first().map(|t| t.duration);",
        [packets("stts_negative")],
    ),
    (
        "a timed text sample entry is not subtitles",
        "                if let Some(c) = subtitle_codec(format) {",
        "                if let Some(c) = subtitle_codec(format).filter(|_| false) {",
        [packets("mov_text")],
    ),
    (
        "a subtitle track's setup is not kept",
        "            d.config = setup;",
        "            let _ = setup;",
        [packets("mov_text")],
    ),
    (
        "a subtitle track's setup runs from the sample entry's start",
        "        let read = self.r.pos().saturating_sub(start);",
        "        let read = 0;",
        [packets("mov_text")],
    ),
    (
        "the room is not the file's length",
        "        let entries_left = r.len();",
        "        let entries_left = u64::MAX;",
        [FILE_ROOM],
    ),
    (
        "an stsc's numbers are unsigned, not FFmpeg's ints",
        "            || count < 1\n",
        "            || count == 0\n",
        ["an_stsc_is_repaired_as_ffmpeg_repairs_it", packets("stsc_count_negative")],
    ),
    (
        "a run FFmpeg cannot index is read",
        "        if indexed.saturating_add(u64::from(entries)) > u64::from(INDEX_ALLOC) {",
        "        if false && indexed.saturating_add(u64::from(entries)) > u64::from(INDEX_ALLOC) {",
        [packets("trun_past_ffmpeg_index")],
    ),
    (
        "a run is indexed past the room",
        "        let kept = entries.min(u32::try_from(self.entries_left).unwrap_or(u32::MAX));",
        "        let kept = entries;",
        [FILE_ROOM],
    ),
    (
        "a run's samples past the room do not move its time on",
        "                dts = i64::try_from(end).unwrap_or(i64::MAX);",
        "                let _ = end;",
        [packets("trun_claims_a_million"), packets("trun_runs_claim_millions")],
    ),
    (
        "a run's samples past the room are not refused as FFmpeg refuses them",
        "                if frag.size == 0 || end > i128::from(i64::MAX) {",
        "                if false {",
        [packets("trun_time_overflows"), packets("trun_run_of_no_size")],
    ),
    (
        "a run's samples giving their fields are indexed past the room",
        "            if n < kept {",
        "            if true {",
        [FILE_ROOM],
    ),
    (
        "a run's samples take nothing from the room",
        "                self.entries_left = self.entries_left.saturating_sub(1);\n",
        "",
        [FILE_ROOM],
    ),
]

DEMUX = [
    (
        "tracks are read by time alone",
        "                    (diff <= TIME_BASE.unsigned_abs() && e.pos < best_pos)\n",
        "                    (diff <= TIME_BASE.unsigned_abs() && dts < best_dts)\n",
        [packets("co64_stz2"), packets("fragmented")],
    ),
    (
        "a sample cut short is not marked",
        "                p.corrupt = short;",
        "                p.corrupt = false && short;",
        [packets("truncated")],
    ),
    (
        "the composition offsets are not added",
        "            dts.saturating_add(i64::from(sc.dts_shift).saturating_add(i64::from(t.offset)))",
        "            dts.saturating_add(i64::from(sc.dts_shift))",
        [packets("h264_bframes"), packets("ctts_tail")],
    ),
    (
        "the priming is given with no packet",
        "        let skip = u32::try_from(sc.skip_samples.max(0)).unwrap_or(0);",
        "        let skip = 0 * u32::try_from(sc.skip_samples.max(0)).unwrap_or(0);",
        [packets("aac"), packets("vp9_opus")],
    ),
    (
        "a seek leaves the other tracks where they were",
        "        for i in 0..self.streams.len() {\n            if i == track {",
        "        for i in 0..0 {\n            if i == track {",
        [seeks("vp9_opus"), seeks("aac")],
    ),
    (
        "a seek's time is not moved onto the decoding timeline",
        "            timestamp.wrapping_sub(sc.min_corrected_pts.wrapping_add(i64::from(sc.dts_shift)));",
        "            timestamp.wrapping_sub(0 * sc.min_corrected_pts.wrapping_add(i64::from(sc.dts_shift)));",
        [seeks("h264_bframes"), seeks("mpeg4_bframes")],
    ),
    (
        "a track not selected is read and given",
        "                || self.selected.as_ref().is_some_and(|s| !s.contains(&i));",
        "                || false && self.selected.as_ref().is_some_and(|s| !s.contains(&i));",
        [ALONE, ALONE_READS_OWN],
    ),
    (
        "a track not selected is read, then let go",
        "            if !discarded {\n                let pos",
        "            if true {\n                let pos",
        [ALONE_READS_OWN],
    ),
]

TRACK = [
    (
        "QuickTime's text sample entry is not timed text",
        '        b"tx3g" | b"text" => Some(Codec::MovText),',
        '        b"tx3g" => Some(Codec::MovText),',
        ["a_code_is_subtitles_by_ffmpegs_table"],
    ),
    (
        "CEA-608 captions are not subtitles",
        '        b"c608" => Some(Codec::Other),\n',
        "",
        ["a_code_is_subtitles_by_ffmpegs_table"],
    ),
]

READER = [
    (
        "a new read-ahead reads on from where the old one had read to",
        "        if let Err(e) = old.seek(SeekFrom::Start(self.pos)) {",
        "        if let Err(e) = old.stream_position() {",
        ["a_new_read_ahead_reads_on_from_where_reading_is", ALONE],
    ),
    (
        "a new read-ahead keeps the old one's size",
        "        self.inner = Some(BufReader::with_capacity(bytes, old.into_inner()));",
        "        self.inner = Some(BufReader::with_capacity(READ_AHEAD, old.into_inner()));",
        ["a_small_read_ahead_reads_little_past_what_is_read"],
    ),
    (
        "a read-ahead that cannot be changed loses the source",
        "            self.inner = Some(old);\n            return Err(e.into());",
        "            return Err(e.into());",
        ["a_read_ahead_that_cannot_be_changed_leaves_reading_as_it_was"],
    ),
]

PROBE = [
    (
        "a free box does not say MP4",
        "                    | b\"free\"\n",
        "                    | b\"fre_\"\n",
        ["mp4s_first_boxes_are_known_by_their_types"],
    ),
    (
        "a size too small for a box is not stepped over",
        "            offset = offset.saturating_add(4);\n            continue;",
        "            return false;",
        ["other_files_are_not_mp4"],
    ),
]

RATIONAL = [
    (
        "a reduction past the limit keeps the last convergent",
        "            if left > right {",
        "            if left > right && false {",
        ["reduce_finds_the_nearest_fraction_within_the_limit"],
    ),
    (
        "infinity converts to 2^63",
        "        signed(d - TWO_63) ^ (1 << 63)",
        "        signed(d - TWO_63) | (1 << 63)",
        ["a_double_converts_to_unsigned_as_c_does", packets("clap_infinite")],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [
        (SRC / "index.rs", INDEX),
        (SRC / "parse.rs", PARSE),
        (SRC / "demux.rs", DEMUX),
        (SRC / "lib.rs", PROBE),
        (SRC / "rational.rs", RATIONAL),
        (SRC / "track.rs", TRACK),
        (SRC / "reader.rs", READER),
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
        results.append(sweep(src, rows, "mp4", timeout=600, only=mine or None))
    raise SystemExit(max(results))
