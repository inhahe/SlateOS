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
        "        while at(m).is_some_and(|e| e.flags & DISCARD != 0) && m < b && m < nb - 1 {",
        "        while false && at(m).is_some_and(|e| e.flags & DISCARD != 0) && m < b && m < nb - 1 {",
        ["the_search_steps_over_discarded_entries_as_ffmpeg_does", seeks("two_edits")],
    ),
    (
        "a backward search does not go back to a key frame",
        "            m += if backward { -1 } else { 1 };",
        "            m += 1;",
        ["the_search_finds_key_frames_backward_and_forward", seeks("av1")],
    ),
    (
        "an edit's first key frame ignores the composition offsets",
        "        if self.has_ctts && index >= 0 {",
        "        if false && self.has_ctts && index >= 0 {",
        [packets("edit_before_key_shows"), packets("edit_at_shown_key")],
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
