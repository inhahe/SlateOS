"""Mutation test for the video file readers.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
reader is not testing the reader.

The crate is four files -- the crate root, and a reader each for MP4,
Matroska and AVI -- and the harness mutates one file a sweep, so this runs a
sweep a file.  A wrong length or a wrong codec in a media info panel is
believed; every row is a way to be wrong and still return an answer.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

MP4 = "an_mp4_is_read_for_its_length_its_picture_and_its_sound"
MP3ESDS = "an_mp4a_entry_is_mp3_when_its_descriptor_says_so"
V1 = "version_1_headers_have_64_bit_times"
FRAG = "a_fragmented_mp4_is_timed_by_its_mehd_and_else_by_its_longest_track"
LARGE = "a_box_with_a_64_bit_size_is_stepped_over"
TOEND = "a_box_sized_to_the_end_of_the_file_is_read"
UNKNOWNLEN = "an_unknown_movie_length_is_taken_from_the_tracks"
ESDSFIELDS = "an_esds_with_optional_fields_is_read_past_them"
TITLE = "the_title_is_read_in_both_styles"
SUBS = "subtitles_disabled_tracks_and_a_presentation_size"
QTV2 = "a_quicktime_version_2_sound_description_is_read_where_it_moved"
ENTRIES = "every_sample_entry_names_its_codec"
MKV = "a_matroska_file_is_read_for_its_length_its_picture_and_its_sound"
WEBM = "a_webm_file_is_named_so_by_its_header"
FLAGS = "a_track_says_its_name_language_and_flags"
SCALE = "the_duration_is_in_ticks_of_the_timestamp_scale"
AUDIODEF = "an_audio_track_takes_matroskas_defaults_and_its_output_rate"
SEEKHEAD = "what_a_cluster_of_unknown_length_hides_is_found_through_the_seek_head"
VINT = "an_ebml_integer_is_as_long_as_its_leading_zeros_say"
AVI = "an_avi_is_read_for_its_length_its_picture_and_its_sound"
ODML = "an_opendml_avi_counts_its_frames_past_the_first_gigabyte"
STRN = "a_stream_says_its_name_and_whether_it_is_off"
EXT = "an_extensible_wave_format_names_its_sub_format_and_a_blank_compression_its_handler"
NOTVIDEO = "what_is_not_a_video_says_nothing"
PREFERRED = "the_preferred_track_is_the_default_one"

LIB = [
    (
        "a NaN length is a length",
        "    probe.duration_secs = probe.duration_secs.filter(|s| s.is_finite() && *s > 0.0);",
        "    probe.duration_secs = probe.duration_secs.filter(|_| true);",
        [SCALE],
    ),
    (
        "the container is not said",
        "    probe.container = container;\n",
        "",
        [MP4],
    ),
    (
        "a Matroska file is read as an MP4",
        "        Container::Matroska | Container::WebM => mkv::probe(r, len)?,",
        "        Container::Matroska | Container::WebM => mp4::probe(r, len)?,",
        [MKV],
    ),
    (
        "any RIFF file is an AVI",
        '        } else if head.get(..4) == Some(b"RIFF") && head.get(8..12) == Some(b"AVI ") {',
        '        } else if head.get(..4) == Some(b"RIFF") {',
        [NOTVIDEO],
    ),
    (
        "a line break in a title is kept",
        "            .map(|c| if c.is_control() { ' ' } else { c })",
        "            .map(|c| c)",
        [FLAGS],
    ),
    (
        "undetermined is a language",
        '    (!c.is_empty() && !c.eq_ignore_ascii_case("und")).then(|| c.to_owned())',
        "    (!c.is_empty()).then(|| c.to_owned())",
        [FLAGS],
    ),
    (
        "the preferred track ignores the default",
        "            .find(|t| t.kind == kind && t.default)",
        "            .find(|t| t.kind == kind)",
        [PREFERRED],
    ),
]

MP4_ROWS = [
    (
        "a 64-bit size is the header's length",
        "            Some(large) => (16, large),",
        "            Some(_) => (16, 16),",
        [LARGE],
    ),
    (
        "a box to the end of the file is nothing",
        "        0 => (8_u64, limit.saturating_sub(at)),",
        "        0 => return Ok(None),",
        [TOEND],
    ),
    (
        "the movie length is not read",
        "            probe.duration_secs = secs(scale, duration);\n",
        "",
        [MP4],
    ),
    (
        "a version 1 header is read as version 0",
        "    if b.first() == Some(&1) {\n        let scale",
        "    if false {\n        let scale",
        [V1],
    ),
    (
        "an unknown length is 49 days",
        "be32(b, 16).filter(|&d| d != u32::MAX).map(u64::from)",
        "be32(b, 16).map(u64::from)",
        [UNKNOWNLEN],
    ),
    (
        "mehd is not read",
        "    if probe.duration_secs.is_none()\n        && let Some(mvex)",
        "    if false\n        && let Some(mvex)",
        [FRAG],
    ),
    (
        "the longest track is not the length",
        "    if probe.duration_secs.is_none() {\n        probe.duration_secs = longest_track;\n    }",
        "",
        [FRAG],
    ),
    (
        "the longest track is the shortest",
        "longest_track.map_or(s, |l| l.max(s))",
        "longest_track.map_or(s, |l| l.min(s))",
        [FRAG],
    ),
    (
        "a disabled track is the default",
        "        track.default = be32(&b, 0).is_some_and(|v| v & 1 != 0);",
        "        track.default = true;",
        [SUBS],
    ),
    (
        "the presentation size is not taken",
        "        track.width = track.width.or(presented.0);",
        "        track.width = track.width.or(None);",
        [SUBS],
    ),
    (
        "the language is read two bytes late",
        "        let at = if b.first() == Some(&1) { 32 } else { 20 };",
        "        let at = if b.first() == Some(&1) { 32 } else { 22 };",
        [MP4],
    ),
    (
        "a picture track is not a picture",
        '            Some(b"vide") => Kind::Video,',
        '            Some(b"vid ") => Kind::Video,',
        [MP4],
    ),
    (
        "sbtl is not a subtitle",
        '            Some(b"sbtl" | b"text" | b"subt" | b"clcp") => Kind::Subtitle,',
        '            Some(b"text" | b"subt" | b"clcp") => Kind::Subtitle,',
        [SUBS],
    ),
    (
        "the frame rate is the frame count",
        "            track.frame_rate = be32(&b, 8).map(|count| f64::from(count) / secs);",
        "            track.frame_rate = be32(&b, 8).map(f64::from);",
        [MP4],
    ),
    (
        "the sample entry is read four bytes early",
        "box_at(r, stsd.body.saturating_add(8), stsd.end, len)",
        "box_at(r, stsd.body.saturating_add(4), stsd.end, len)",
        [MP4],
    ),
    (
        "the width is the height",
        "            track.width = be16(&b, 24).map(u32::from).filter(|&w| w > 0);",
        "            track.width = be16(&b, 26).map(u32::from).filter(|&w| w > 0);",
        [MP4],
    ),
    (
        "QuickTime version 2 sound is read as version 0",
        "            let children_at = if be16(&b, 8) == Some(2) {",
        "            let children_at = if false {",
        [QTV2],
    ),
    (
        "a 16.16 rate is read whole",
        "                track.sample_rate = be32(&b, 24).map(|v| v >> 16);",
        "                track.sample_rate = be32(&b, 24);",
        [MP4],
    ),
    (
        "the channels are the sample size",
        "                track.channels = be16(&b, 16);",
        "                track.channels = be16(&b, 18);",
        [MP3ESDS],
    ),
    (
        "an mp4a's descriptor is not read",
        '            if &entry.kind == b"mp4a" {',
        "            if false {",
        [MP3ESDS],
    ),
    (
        "the descriptor is looked for past QuickTime version 1's fields",
        "                if be16(&b, 8) == Some(1) { 44 } else { 28 }",
        "                if be16(&b, 8) == Some(1) { 44 } else { 44 }",
        [MP3ESDS],
    ),
    (
        "a dependent stream's id is read as the configuration",
        "    if flags & 0x80 != 0 {",
        "    if false {",
        [ESDSFIELDS],
    ),
    (
        "a URL is read as the configuration",
        "    if flags & 0x40 != 0 {",
        "    if false {",
        [ESDSFIELDS],
    ),
    (
        "an OCR stream's id is read as the configuration",
        "    if flags & 0x20 != 0 {",
        "    if false {",
        [ESDSFIELDS],
    ),
    (
        "object type 0x6B is not MP3",
        "        0x69 | 0x6B => Some(Codec::Mp3),",
        "        0x69 => Some(Codec::Mp3),",
        [MP3ESDS],
    ),
    (
        "four bytes that are not letters are named",
        "    if code.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {",
        "    if true {",
        [ENTRIES],
    ),
    (
        "ISO meta is read as QuickTime's",
        '        let first = if peek.as_slice() == b"hdlr" {',
        "        let first = if true {",
        [TITLE],
    ),
    (
        "the title's type and locale are read as text",
        "            if let Some(title) = b.get(8..).and_then(text) {",
        "            if let Some(title) = b.get(0..).and_then(text) {",
        [TITLE],
    ),
]

MKV_ROWS = [
    (
        "an id loses its marker",
        "    let Some((id, id_len)) = vint(&head, 0, true) else {",
        "    let Some((id, id_len)) = vint(&head, 0, false) else {",
        [MKV],
    ),
    (
        "a length keeps its marker",
        "    let Some((size, size_len)) = vint(&head, id_len, false) else {",
        "    let Some((size, size_len)) = vint(&head, id_len, true) else {",
        [MKV],
    ),
    (
        "the marker mask is a bit short",
        "        0xFF_u8.checked_shr(u32::try_from(n).ok()?).unwrap_or(0)",
        "        0xFF_u8.checked_shr(u32::try_from(n).ok()?.saturating_sub(1)).unwrap_or(0)",
        [VINT],
    ),
    (
        "a cut-short integer is read as zeros",
        "        value = value.checked_shl(8)? | u64::from(*b.get(at.checked_add(i)?)?);",
        "        value = value.checked_shl(8)? | u64::from(b.get(at.checked_add(i)?).copied().unwrap_or(0));",
        [VINT],
    ),
    (
        "Info is not read",
        "            INFO => info = Some(el),",
        "            INFO => {}",
        [MKV],
    ),
    (
        "Tracks are not read",
        "            TRACKS => tracks = Some(el),",
        "            TRACKS => {}",
        [MKV],
    ),
    (
        "the seek head is not read",
        "            SEEK_HEAD => seeks.extend(read_seek_head(r, el, len)?),",
        "            SEEK_HEAD => {}",
        [SEEKHEAD],
    ),
    (
        "a seek position is from the file's start",
        "        let at = segment.body.saturating_add(position);",
        "        let at = position;",
        [SEEKHEAD],
    ),
    (
        "the walk stops at the first of Info and Tracks",
        "(info.is_none() || tracks.is_none())",
        "(info.is_none() && tracks.is_none())",
        [MKV],
    ),
    (
        "the timestamp scale is ignored",
        "                    scale = s;",
        "                    let _ = s;",
        [SCALE],
    ),
    (
        "a four-byte float is no float",
        "        4 => Some(f64::from(f32::from_be_bytes(b.try_into().ok()?))),",
        "        4 => None,",
        [SCALE],
    ),
    (
        "the title is not read",
        "            TITLE => probe.title = text(value),",
        "            TITLE => {}",
        [FLAGS],
    ),
    (
        "a track is not default unless it says",
        "        default: true,\n",
        "        default: false,\n",
        [MKV],
    ),
    (
        "English is not the default language",
        '    let (mut iso, mut bcp47) = (Some(String::from("eng")), None);',
        "    let (mut iso, mut bcp47) = (None, None);",
        [MKV],
    ),
    (
        "ISO 639-2 wins over BCP 47",
        "    track.language = bcp47.or(iso).as_deref().and_then(language);",
        "    track.language = iso.or(bcp47).as_deref().and_then(language);",
        [FLAGS],
    ),
    (
        "a forced flag of 0 is forced",
        "            FLAG_FORCED => track.forced = uint(value) == Some(1),",
        "            FLAG_FORCED => track.forced = uint(value).is_some(),",
        [FLAGS],
    ),
    (
        "the default flag is read backwards",
        "            FLAG_DEFAULT => track.default = uint(value) != Some(0),",
        "            FLAG_DEFAULT => track.default = uint(value) == Some(0),",
        [FLAGS],
    ),
    (
        "the frame rate is the frame's nanoseconds",
        "        let fps = frame_ns.map(|ns| 1e9 / ns as f64);",
        "        let fps = frame_ns.map(|ns| ns as f64);",
        [MKV],
    ),
    (
        "the height is the width",
        "                        PIXEL_HEIGHT => track.height = pixels(),",
        "                        PIXEL_HEIGHT => track.width = pixels(),",
        [MKV],
    ),
    (
        "the coded rate wins over the output rate",
        "    track.sample_rate = whole_hertz(output.unwrap_or(coded));",
        "    track.sample_rate = whole_hertz(coded);",
        [AUDIODEF],
    ),
    (
        "no 8 kHz default",
        "    let (mut coded, mut output, mut channels) = (8000.0, None, 1_u64);",
        "    let (mut coded, mut output, mut channels) = (0.0, None, 1_u64);",
        [AUDIODEF],
    ),
    (
        "no one-channel default",
        "    let (mut coded, mut output, mut channels) = (8000.0, None, 1_u64);",
        "    let (mut coded, mut output, mut channels) = (8000.0, None, 0_u64);",
        [AUDIODEF],
    ),
    (
        "WebM is Matroska",
        '    if doc_type.as_deref() == Some("webm") {',
        "    if false {",
        [WEBM],
    ),
    (
        "an AAC id with a profile is not AAC",
        '        id if id.starts_with("A_AAC") => Codec::Aac,',
        '        id if id == "A_AAC" => Codec::Aac,',
        [AUDIODEF],
    ),
]

AVI_ROWS = [
    (
        "the header list is not found",
        '        if &id == b"LIST" && head.get(8..12) == Some(b"hdrl") {',
        '        if &id == b"LIST" && head.get(8..12) == Some(b"movi") {',
        [AVI],
    ),
    (
        "a top-level chunk's pad byte is not stepped over",
        "            .saturating_add(u64::from(size & 1));",
        "            .saturating_add(0);",
        [STRN],
    ),
    (
        "a chunk's pad byte is not stepped over",
        "        at = end.saturating_add(size & 1);",
        "        at = end;",
        [STRN],
    ),
    (
        "the main header's count is its frame time",
        "                main_frames = le32(body, 16);",
        "                main_frames = le32(body, 0);",
        [STRN],
    ),
    (
        "OpenDML's count is not used",
        "            let frames = opendml_frames.unwrap_or(length);",
        "            let frames = length;",
        [ODML],
    ),
    (
        "a stream that is off is on",
        "                track.default = le32(body, 8).is_some_and(|f| f & 1 == 0);",
        "                track.default = true;",
        [STRN],
    ),
    (
        "a top-down picture's height is negative",
        "                    .map(|v| i32::from_le_bytes(v.to_le_bytes()).unsigned_abs())",
        "                    .map(|v| v)",
        [ODML],
    ),
    (
        "a blank compression is a codec",
        "                .filter(|c| c != &[0; 4])",
        "                .filter(|_| true)",
        [EXT],
    ),
    (
        "the extensible sub-format is not read",
        "            if tag == Some(0xFFFE) {",
        "            if false {",
        [EXT],
    ),
    (
        "the frame rate is upside down",
        "                track.frame_rate = Some(f64::from(rate) / f64::from(scale));",
        "                track.frame_rate = Some(f64::from(scale) / f64::from(rate));",
        [AVI],
    ),
    (
        "MP3 is MP2",
        "        0x0055 => Codec::Mp3,",
        "        0x0055 => Codec::Mp2,",
        [AVI],
    ),
    (
        "a lowercase code is not known",
        "    upper.make_ascii_uppercase();\n",
        "",
        [ODML],
    ),
    (
        "the stream's name is not read",
        '            b"strn" => track.name = text(body),',
        '            b"strn" => {}',
        [STRN],
    ),
    (
        "the sound's clock is the picture's",
        "                    if track.kind == Kind::Video && video_clock.is_none() {",
        "                    if video_clock.is_none() {",
        [STRN],
    ),
]

TABLES = {
    "lib.rs": LIB,
    "mp4.rs": MP4_ROWS,
    "mkv.rs": MKV_ROWS,
    "avi.rs": AVI_ROWS,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "mediaprobe", timeout=600, only=mine))
    raise SystemExit(worst)
