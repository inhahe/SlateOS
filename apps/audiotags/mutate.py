"""Mutation test for the audio file readers.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
reader is not testing the reader.

The readers came out of `apps/musicplayer`'s binary, where they decoded a
Latin-1 tag as UTF-8, read a 32-bit FLAC as 16, and were never called.  Every
row here is a way a reader can be wrong while still returning something that
looks like an answer -- a length, a bitrate, a title -- which is the failure
nobody sees: a wrong number in a column is believed.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

CBR = "a_constant_bitrate_mp3_is_timed_by_its_size"
XING = "a_variable_bitrate_mp3_is_timed_by_its_xing_header"
ENC = "id3v2_text_is_read_in_every_encoding_and_trimmed_of_its_nul"
V24 = "id3v2_4_sizes_are_synchsafe_and_a_second_value_is_left_out"
GROUP = "a_grouped_frame_is_read_past_its_group_byte"
V1 = "an_id3v1_tag_fills_what_the_id3v2_one_does_not_say"
V10 = "an_id3v1_0_comment_is_not_a_track_number"
FLAC = "a_flac_file_reads_its_streaminfo_and_its_own_tags"
FLACID3 = "a_flac_behind_an_id3_tag_is_a_flac_and_both_tags_are_read"
SYNC = "a_lone_sync_before_the_first_frame_is_not_a_frame"
DEPTH = "a_32_bit_flac_is_32_bits"
OGG = "an_ogg_vorbis_file_is_timed_by_its_last_page_and_read_for_its_tags"
OGGLAST = "an_ogg_file_is_timed_by_its_own_streams_last_page"
OPUS = "an_opus_file_counts_its_granule_at_48_khz_less_its_pre_skip"
WAV = "a_wav_file_reads_its_format_its_length_and_its_info_tags"
WAVCUT = "a_cut_or_streamed_wav_is_timed_by_what_it_holds"
NOTAUDIO = "what_is_not_audio_says_nothing_and_a_cut_file_does_not_panic"
CONTROL = "a_control_character_inside_a_field_is_a_space"
FIXTURES = "the_test_files_read_as_what_they_were_made_as"
SYNCHSAFE = "a_synchsafe_integer_is_seven_bits_a_byte_and_a_high_bit_is_refused"
GENRE = "a_genre_number_is_its_name_and_a_name_is_itself"
V22 = "id3v2_2_frames_have_three_letter_names_and_three_byte_sizes"
UNSYNC = "an_unsynchronised_tag_is_read_as_it_was_written"
EXT = "an_extended_header_is_stepped_over"
FOOTER = "a_tag_with_a_footer_ends_after_it"
COMPRESSED = "a_compressed_or_encrypted_frame_is_passed_over"
VBRI = "a_variable_bitrate_mp3_is_timed_by_its_vbri_header"
INFO = "an_info_header_times_the_file_and_the_bytes_there_give_the_rate"
MPEG2 = "an_mpeg2_mono_file_is_timed_and_its_xing_header_found"
MONO1 = "an_mpeg1_mono_files_xing_header_is_after_17_bytes"
LAYER1 = "a_layer_one_file_is_timed_by_its_frames"
LAYER2 = "a_layer_two_frame_reads_its_own_bitrate_table"
LACING = "an_ogg_packet_runs_across_its_segments"
ODD = "a_wav_steps_over_the_pad_after_an_odd_chunk_and_reads_every_field"
FLACLEN = "a_flac_that_does_not_know_its_length_says_so"
ZEROFRAMES = "a_vbr_header_counting_no_frames_is_passed_over"
OGGZERO = "an_ogg_stream_of_no_length_has_no_bitrate"
BIGTAG = "a_flac_behind_a_tag_longer_than_the_first_read_is_a_flac"
SIZES = "frame_sizes_over_127_are_read_as_each_version_writes_them"
PADDED = "a_padded_frame_is_a_byte_longer"
HEADERS = "frame_headers_are_read_bit_by_bit"
NOTTEXT = "a_comment_that_is_not_text_is_passed_over"
LONGFLAC = "a_flac_of_more_samples_than_32_bits_is_timed"

MUTATIONS = [
    # ---- what a file is ----
    (
        "a RIFF file is a WAV whatever it holds",
        '        if head.get(..4) == Some(b"RIFF") && head.get(8..12) == Some(b"WAVE") {',
        '        if head.get(..4) == Some(b"RIFF") {',
        [NOTAUDIO],
    ),
    (
        "an ID3 tag makes what follows it an MP3",
        '            if head.get(tag..tag.saturating_add(4)) == Some(b"fLaC") {',
        "            if false {",
        [FLACID3],
    ),
    (
        "a tag longer than the first read is not looked past",
        "        && tag >= head.len()\n",
        "        && false\n",
        [BIGTAG],
    ),
    (
        "what is behind an ID3 tag is nothing",
        "        } else if let Some(tag) = id3v2_len(head) {",
        "        } else if let Some(tag) = None::<usize> {",
        [ENC],
    ),
    # ---- one tag filling another ----
    (
        "an ID3v1 tag overrides the ID3v2 one",
        "            if mine.is_none() {\n                *mine = theirs;",
        "            if theirs.is_some() {\n                *mine = theirs;",
        [V1],
    ),
    (
        "an ID3v1 track number is not used",
        "        if self.track.is_none() {\n            self.track = other.track;",
        "        if false {\n            self.track = other.track;",
        [V1],
    ),
    # ---- numbers ----
    (
        "a rate of nothing gives a length",
        "    (rate > 0).then(|| count as f64 / f64::from(rate))",
        "    Some(count as f64 / f64::from(rate))",
        [FLACLEN],
    ),
    (
        "a length of nothing gives a rate",
        "    (secs > 0.0).then(|| (bytes as f64 * 8.0 / secs / 1000.0).round() as u32)",
        "    Some((bytes as f64 * 8.0 / secs / 1000.0).round() as u32)",
        [OGGZERO],
    ),
    (
        "a bitrate is cut rather than rounded",
        "(bytes as f64 * 8.0 / secs / 1000.0).round() as u32)",
        "(bytes as f64 * 8.0 / secs / 1000.0).floor() as u32)",
        [INFO],
    ),
    (
        "a bitrate given in bits is cut",
        "    bits.saturating_add(500) / 1000",
        "    bits / 1000",
        [FIXTURES],
    ),
    # ---- text ----
    (
        "a control character inside a field is kept",
        "            .map(|c| if c.is_control() { ' ' } else { c })",
        "            .map(|c| c)",
        [CONTROL],
    ),
    (
        "the NUL a field is padded with is kept",
        r"    let t = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());",
        "    let t = text.trim_matches(|c: char| c.is_whitespace());",
        [WAV],
    ),
    (
        "Latin-1 is read as UTF-8",
        "    bytes.iter().map(|&b| char::from(b)).collect()",
        "    String::from_utf8_lossy(bytes).into_owned()",
        [ENC],
    ),
    (
        "a little-endian byte order mark is not obeyed",
        "        Some([0xFF, 0xFE]) => (bytes.get(2..)?, false),",
        "        Some([0xFF, 0xFE]) => (bytes.get(2..)?, true),",
        [ENC],
    ),
    (
        "UTF-16BE without a mark is read little-endian",
        "        2 => utf16(body, true)?,",
        "        2 => utf16(body, false)?,",
        [ENC],
    ),
    (
        "UTF-8 text is not read",
        "        3 => String::from_utf8(body.to_vec()).ok()?,",
        "        3 => return None,",
        [V24],
    ),
    (
        "a second value is joined to the first",
        "    let first = text\n        .split('\\0')\n        .find(|v| !v.trim().is_empty())\n        .unwrap_or(\"\");\n    clean(first)",
        "    clean(&text)",
        [V24],
    ),
    # ---- genre and track ----
    (
        "a genre number in brackets is not a number",
        "    let number = t\n        .strip_prefix('(')\n        .and_then(|rest| rest.split(')').next())\n        .unwrap_or(t);",
        "    let number = t;",
        [GENRE],
    ),
    (
        "a genre number past the list is dropped",
        "        Ok(n) => GENRES.get(n).map_or(text.clone(), |g| (*g).to_owned()),",
        "        Ok(n) => GENRES.get(n).map_or(String::new(), |g| (*g).to_owned()),",
        [GENRE],
    ),
    (
        "a track of a total is not a number",
        "    text.split('/').next()?.trim().parse().ok()",
        "    text.trim().parse().ok()",
        [ENC],
    ),
    # ---- ID3v2 structure ----
    (
        "a synchsafe byte's high bit is let through",
        "    if s.iter().any(|&x| x & 0x80 != 0) {\n        return None;\n    }",
        "    if false {\n        return None;\n    }",
        [SYNCHSAFE],
    ),
    (
        "a synchsafe integer is eight bits a byte",
        "    Some(s.iter().fold(0_u32, |acc, &x| (acc << 7) | u32::from(x)))",
        "    Some(s.iter().fold(0_u32, |acc, &x| (acc << 8) | u32::from(x)))",
        [SYNCHSAFE],
    ),
    (
        "unsynchronisation keeps its zeros",
        "        if !(last == 0xFF && b == 0) {",
        "        if !(last == 0xFF && b == 0) || true {",
        [UNSYNC],
    ),
    (
        "a footer is not counted",
        "    let footer = if head.get(5)? & 0x10 != 0 { 10 } else { 0 };",
        "    let footer = 0;",
        [FOOTER],
    ),
    (
        "the whole tag's unsynchronisation is not undone",
        "    let body = if flags & 0x80 != 0 && major < 4 {",
        "    let body = if false {",
        [UNSYNC],
    ),
    (
        "an extended header is read as frames",
        "    if flags & 0x40 != 0 && major >= 3 {",
        "    if false {",
        [EXT],
    ),
    (
        "a v2.3 extended header's size counts itself",
        "            be32(&body, 0).map(|n| n.saturating_add(4))",
        "            be32(&body, 0)",
        [EXT],
    ),
    (
        "v2.2 frames are read as v2.3's",
        "    let (id_len, header_len) = if major == 2 { (3, 6) } else { (4, 10) };",
        "    let (id_len, header_len) = (4, 10);",
        [V22],
    ),
    (
        "a v2.2 size is its last byte",
        "                .get(3..6)",
        "                .get(5..6)",
        [V22],
    ),
    (
        "a v2.3 frame size is synchsafe",
        "            3 => be32(header, 4),",
        "            3 => synchsafe(header.get(4..8).unwrap_or(&[])),",
        [SIZES],
    ),
    (
        "a v2.4 frame size is a plain integer",
        "            _ => synchsafe(header.get(4..8).unwrap_or(&[])),",
        "            _ => be32(header, 4),",
        [SIZES],
    ),
    (
        "a compressed or encrypted frame is read as text",
        "            if compressed || encrypted {\n                pos = end;\n                continue;\n            }",
        "            if false {\n                pos = end;\n                continue;\n            }",
        [COMPRESSED],
    ),
    (
        "v2.3 compression is looked for in v2.4's bits",
        "                    fflags & 0x80 != 0,\n                    fflags & 0x40 != 0,\n                    fflags & 0x20 != 0,",
        "                    fflags & 0x08 != 0,\n                    fflags & 0x04 != 0,\n                    fflags & 0x20 != 0,",
        [COMPRESSED],
    ),
    (
        "a v2.4 frame's own unsynchronisation is not undone",
        "            if unsync {\n                frame = unsynchronise(&frame);\n            }",
        "            if false {\n                frame = unsynchronise(&frame);\n            }",
        [UNSYNC],
    ),
    (
        "a group's byte is read as text",
        "            let before_text = usize::from(grouped)",
        "            let before_text = usize::from(false && grouped)",
        [GROUP],
    ),
    (
        "a frame's own length is read as text",
        "if length_first { 4 } else { 0 }",
        "if length_first { 0 } else { 0 }",
        [GROUP],
    ),
    (
        "v2.4 grouping is looked for in v2.3's bit",
        "                    fflags & 0x04 != 0,\n                    fflags & 0x40 != 0,",
        "                    fflags & 0x04 != 0,\n                    fflags & 0x20 != 0,",
        [GROUP],
    ),
    (
        "the artist is looked for under another frame",
        '            b"TPE1" | b"TP1" => tags.artist = text(),',
        '            b"TPE2" | b"TP1" => tags.artist = text(),',
        [ENC],
    ),
    # ---- ID3v1 ----
    (
        "an ID3v1 tag needs no TAG",
        '    if last.len() != 128 || last.get(..3) != Some(b"TAG") {',
        "    if last.len() != 128 {",
        [CBR],
    ),
    (
        "an ID3v1.0 comment is a track number",
        "    let track = (last.get(125) == Some(&0))",
        "    let track = (last.get(125).is_some())",
        [V10],
    ),
    (
        "the ID3v1 artist is read a byte late",
        "        artist: field(33, 30),",
        "        artist: field(34, 30),",
        [V1],
    ),
    (
        "the ID3v1 genre is read from the track's byte",
        "            .get(127)\n            .and_then(|&g| GENRES.get(usize::from(g)))",
        "            .get(126)\n            .and_then(|&g| GENRES.get(usize::from(g)))",
        [V1],
    ),
    (
        "an ID3v1 tag is timed as audio",
        "    let audio_end = if v1.is_some() {\n        len.saturating_sub(128)\n    } else {\n        len\n    };",
        "    let audio_end = len;",
        [V1],
    ),
    # ---- MPEG frames ----
    (
        "a layer I frame has 1152 samples",
        "            (1, _) => 384,",
        "            (1, _) => 1152,",
        [LAYER1],
    ),
    (
        "an MPEG-2 layer III frame has 1152 samples",
        "            (3, 2 | 25) => 576,",
        "            (3, 2 | 25) => 1152,",
        [MPEG2],
    ),
    (
        "a layer I frame is counted in bytes, not slots",
        "                .saturating_add(pad)\n                .saturating_mul(4),",
        "                .saturating_add(pad),",
        [LAYER1],
    ),
    (
        "an MPEG-2 layer III frame is as long as an MPEG-1 one",
        "            3 if self.version != 1 => bits\n                .saturating_mul(72)",
        "            3 if self.version != 1 => bits\n                .saturating_mul(144)",
        [MPEG2],
    ),
    (
        "padding is not counted",
        "        let pad = u64::from(self.padding);",
        "        let pad = 0_u64;",
        [PADDED],
    ),
    (
        "a stereo Xing header is looked for where a mono one is",
        "            (1, false) => 36,",
        "            (1, false) => 21,",
        [XING],
    ),
    (
        "an MPEG-1 mono Xing header is looked for after 32 bytes",
        "            (1, true) | (_, false) => 21,",
        "            (_, false) => 21,\n            (1, true) => 36,",
        [MONO1],
    ),
    (
        "an MPEG-2 mono Xing header is looked for after 17 bytes",
        "            (_, true) => 13,",
        "            (_, true) => 21,",
        [MPEG2],
    ),
    (
        "every layer reads layer III's bitrates",
        "        *MPEG1_KBPS.get(row)?.get(index)?",
        "        *MPEG1_KBPS.get(2)?.get(index)?",
        [LAYER2],
    ),
    (
        "an MPEG-2 frame reads MPEG-1's bitrates",
        "    let bitrate_kbps = if version == 1 {",
        "    let bitrate_kbps = if version != 0 {",
        [MPEG2],
    ),
    (
        "MPEG-2 does not halve the rate",
        "        2 => base / 2,",
        "        2 => base,",
        [MPEG2],
    ),
    (
        "a reserved version is MPEG-2",
        "        3 => 1,\n        _ => return None,\n    };\n    let layer",
        "        3 => 1,\n        _ => 2,\n    };\n    let layer",
        [HEADERS],
    ),
    (
        "a reserved layer is layer I",
        "        3 => 1,\n        _ => return None,\n    };\n    let index",
        "        3 => 1,\n        _ => 1,\n    };\n    let index",
        [HEADERS],
    ),
    (
        "a reserved sample rate is 44.1 kHz",
        "        2 => 32_000,\n        _ => return None,",
        "        2 => 32_000,\n        _ => 44_100,",
        [HEADERS],
    ),
    (
        "free format is taken for a frame",
        "    if bitrate_kbps == 0 {\n        return None;",
        "    if false {\n        return None;",
        [HEADERS],
    ),
    (
        "joint stereo is mono",
        "        mono: (h >> 6) & 3 == 3,",
        "        mono: (h >> 6) & 3 != 0,",
        [HEADERS],
    ),
    (
        "the first header found is taken unconfirmed",
        "            Some(after) if after.len() >= 4 => mp3_frame(after).is_some(),",
        "            Some(after) if after.len() >= 4 => true,",
        [SYNC],
    ),
    # ---- VBR headers ----
    (
        "a VBR file is timed as if its bitrate were constant",
        '    let vbr = if matches!(xing.get(..4), Some(b"Xing" | b"Info")) {',
        "    let vbr = if false {",
        [XING],
    ),
    (
        "an Info header is not a Xing header",
        'Some(b"Xing" | b"Info")',
        'Some(b"Xing")',
        [INFO],
    ),
    (
        "a Xing byte count is not used",
        "kbps(bytes.map_or(audio_bytes, u64::from), s)",
        "kbps(audio_bytes, s)",
        [XING],
    ),
    (
        "a Xing count of no frames is a length of nothing",
        "            .flatten()\n            .filter(|&f| f > 0);",
        "            .flatten();",
        [ZEROFRAMES],
    ),
    (
        "a VBRI header is not looked for",
        '    } else if first.get(36..40) == Some(b"VBRI") {',
        "    } else if false {",
        [VBRI],
    ),
    (
        "a VBRI header's frames and bytes are swapped",
        "        be32(first, 50)\n            .filter(|&f| f > 0)\n            .map(|f| (f, be32(first, 46)))",
        "        be32(first, 46)\n            .filter(|&f| f > 0)\n            .map(|f| (f, be32(first, 50)))",
        [VBRI],
    ),
    (
        "a VBRI count of no frames is a length of nothing",
        "        be32(first, 50)\n            .filter(|&f| f > 0)\n",
        "        be32(first, 50)\n",
        [ZEROFRAMES],
    ),
    (
        "the ID3 tag is timed as audio",
        "audio_end.saturating_sub(start.saturating_add(u64::try_from(at).unwrap_or(0)))",
        "audio_end.saturating_sub(u64::try_from(at).unwrap_or(0))",
        [FIXTURES],
    ),
    (
        "a constant bitrate is taken as bytes a second",
        "            let secs = audio_bytes as f64 * 8.0 / (f64::from(frame.bitrate_kbps) * 1000.0);",
        "            let secs = audio_bytes as f64 / (f64::from(frame.bitrate_kbps) * 1000.0);",
        [CBR],
    ),
    # ---- Vorbis comments ----
    (
        "a comment's key is matched with its case",
        "        let slot = match key.to_ascii_uppercase().as_str() {",
        "        let slot = match key {",
        [FLAC],
    ),
    (
        "a repeated key's last value is kept",
        "        if slot.is_none() {\n            *slot = clean(value);\n        }",
        "        *slot = clean(value);",
        [FLAC],
    ),
    (
        "DATE is not a year",
        '            "DATE" | "YEAR" => &mut tags.year,',
        '            "YEAR" => &mut tags.year,',
        [FLAC],
    ),
    (
        "GENRE is not read",
        '            "GENRE" => &mut tags.genre,',
        '            "GENRES" => &mut tags.genre,',
        [FLAC],
    ),
    (
        "the vendor string is read as comments",
        "    let mut pos = 4_usize.saturating_add(vendor);",
        "    let mut pos = 4_usize;",
        [FLAC],
    ),
    (
        "a comment that is not text ends the block",
        "        let Ok(comment) = std::str::from_utf8(bytes) else {\n            continue;\n        };",
        "        let Ok(comment) = std::str::from_utf8(bytes) else {\n            break;\n        };",
        [NOTTEXT],
    ),
    # ---- FLAC ----
    (
        "STREAMINFO's rate is read from whole bytes",
        "                    let rate = (u32::from(a) << 12) | (u32::from(b) << 4) | (u32::from(c) >> 4);",
        "                    let rate = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);",
        [FLAC],
    ),
    (
        "a FLAC has one channel fewer",
        "                    let channels = u16::from((c >> 1) & 7).saturating_add(1);",
        "                    let channels = u16::from((c >> 1) & 7);",
        [FLAC],
    ),
    (
        "a FLAC's depth is hi | (lo + 1) again",
        "                    let depth = ((u16::from(c & 1) << 4) | u16::from(d >> 4)).saturating_add(1);",
        "                    let depth = (u16::from(c & 1) << 4) | u16::from(d >> 4).saturating_add(1);",
        [DEPTH],
    ),
    (
        "a sample count's top four bits are dropped",
        "(u64::from(d & 0x0F) << 32) | u64::from(be32(&si, 14).unwrap_or(0))",
        "u64::from(be32(&si, 14).unwrap_or(0))",
        [LONGFLAC],
    ),
    (
        "the last metadata block is not the last",
        "        if kind & 0x80 != 0 {\n            break;\n        }",
        "        if false {\n            break;\n        }",
        [FLAC],
    ),
    (
        "the comment block is not read",
        "            4 => {\n                let block",
        "            5 => {\n                let block",
        [FLAC],
    ),
    (
        "a FLAC's ID3 tag is not used",
        "    if let Some(id3) = id3 {\n        tags.fill_from(id3);\n    }",
        "    if let Some(id3) = id3 {\n        drop(id3);\n    }",
        [FLACID3],
    ),
    (
        "a FLAC's bitrate counts its metadata",
        "        info.bitrate_kbps = kbps(len.saturating_sub(pos), secs);",
        "        info.bitrate_kbps = kbps(len, secs);",
        [FLAC],
    ),
    # ---- Ogg ----
    (
        "a packet ends at a full segment",
        "            if segment < 255 {",
        "            if segment <= 255 {",
        [LACING],
    ),
    (
        "the Vorbis comment packet is not read",
        r'        if let Some(comment) = second.filter(|p| p.get(..7) == Some(b"\x03vorbis")) {',
        "        if let Some(comment) = second.filter(|_| false) {",
        [OGG],
    ),
    (
        "Vorbis's rate is read a byte early",
        "        let rate = le32(first, 12);",
        "        let rate = le32(first, 11);",
        [OGG],
    ),
    (
        "the nominal bitrate is not used",
        "        info.bitrate_kbps = nominal.map(kbps_of_bits);",
        "        info.bitrate_kbps = None;",
        [OGG],
    ),
    (
        "Opus counts at the rate it was made at",
        "        (Some(48_000), Some(48_000), pre_skip)",
        "        (Some(48_000), le32(first, 12), pre_skip)",
        [OPUS],
    ),
    (
        "the pre-skip is not taken off",
        "        info.duration_secs = seconds(granule.saturating_sub(pre_skip), rate);",
        "        info.duration_secs = seconds(granule, rate);",
        [OPUS],
    ),
    (
        "a page's version is not checked",
        " && page.get(4) == Some(&0) && page.get(14..18) == serial;",
        " && page.get(14..18) == serial;",
        [OGGLAST],
    ),
    (
        "another stream's page ends this one",
        " && page.get(4) == Some(&0) && page.get(14..18) == serial;",
        " && page.get(4) == Some(&0);",
        [OGGLAST],
    ),
    (
        "a granule of -1 is a length",
        "            .filter(|&g| g != u64::MAX)\n    });",
        "\n    });",
        [OGGLAST],
    ),
    (
        "an Ogg file with no nominal rate has none",
        "        if info.bitrate_kbps.is_none()\n            && let Some(secs) = info.duration_secs\n        {",
        "        if false\n            && let Some(secs) = info.duration_secs\n        {",
        [OPUS],
    ),
    # ---- WAV ----
    (
        "the sample rate is read from the byte rate's place",
        "                info.sample_rate = le32(&fmt, 4).filter(|&r| r > 0);",
        "                info.sample_rate = le32(&fmt, 8).filter(|&r| r > 0);",
        [WAV],
    ),
    (
        "the byte rate is read from the sample rate's place",
        "                byte_rate = le32(&fmt, 8).unwrap_or(0);",
        "                byte_rate = le32(&fmt, 4).unwrap_or(0);",
        [WAV],
    ),
    (
        "a data chunk's size is believed",
        "        let bytes = if size == u32::MAX {\n            there\n        } else {\n            u64::from(size).min(there)\n        };",
        "        let bytes = u64::from(size);",
        [WAVCUT],
    ),
    (
        "an unknown length is taken as four gigabytes",
        "        let bytes = if size == u32::MAX {",
        "        let bytes = if false {",
        [WAVCUT],
    ),
    (
        "an odd chunk has no pad byte",
        "            .saturating_add(u64::from(size & 1));",
        "            .saturating_add(0);",
        [ODD],
    ),
    (
        "an odd INFO value has no pad byte",
        "                        at = start.saturating_add(n).saturating_add(n & 1);",
        "                        at = start.saturating_add(n);",
        [FIXTURES],
    ),
    (
        "a WAV's album is not read",
        '                            b"IPRD" => tags.album = value,',
        '                            b"IPRD" => {}',
        [FIXTURES],
    ),
    (
        "a WAV's track is not read",
        '                            b"ITRK" => tags.track = value.as_deref().and_then(track_number),',
        '                            b"ITRK" => {}',
        [ODD],
    ),
    (
        "INFO text that is not UTF-8 is dropped",
        "unwrap_or_else(|_| latin1(text))",
        "unwrap_or_default()",
        [ODD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "audiotags", timeout=600, only=only))
