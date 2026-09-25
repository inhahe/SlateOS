"""Mutation test for `wavpcm`: its reader, its converters and its writer.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "24-bit samples lose their sign",
        "            let v = i32::from_le_bytes([0, at(0), at(1), at(2)]) >> 8;",
        "            let v = i32::from_le_bytes([at(0), at(1), at(2), 0]);",
        ["stored_samples_read_as_the_format_says"],
    ),
    (
        "a prefix is taken for the whole file",
        "        let remaining = whole.max(bytes.len()).saturating_sub(start);",
        "        let remaining = bytes.len().saturating_sub(start);",
        ["a_prefix_reads_as_the_whole_files_header"],
    ),
    (
        "a streaming writer's zero size is believed",
        "        let len = if size <= remaining && !(&id == b\"data\" && size == 0) {",
        "        let len = if size <= remaining {",
        ["a_zero_data_size_means_the_rest_of_the_file"],
    ),
    (
        "a streaming writer's all-ones size is refused",
        "        } else if &id == b\"data\" {\n"
        "            remaining",
        "        } else if false {\n"
        "            remaining",
        ["extensible_headers_and_other_chunks_are_read"],
    ),
    (
        "a torn chunk after the samples refuses the file",
        "        } else if seen_data {\n"
        "            break;",
        "        } else if false {\n"
        "            break;",
        ["chunks_after_the_samples_are_read_and_a_torn_tail_is_ignored"],
    ),
    (
        "chunks are not padded to even",
        "        // Chunks are padded to an even length.\n"
        "        at = start.saturating_add(len).saturating_add(len & 1);",
        "        // Chunks are padded to an even length.\n"
        "        at = start.saturating_add(len);",
        ["extensible_headers_and_other_chunks_are_read"],
    ),
    (
        "a peak's low is the last sample, not the lowest",
        "            p.low = p.low.min(v);",
        "            p.low = v;",
        ["peaks_are_the_extremes_of_each_stretch"],
    ),
    (
        "old markers are kept beside the new",
        "        if &chunk.id == b\"cue \" || chunk.is_list(bytes, b\"adtl\") {",
        "        if false {",
        ["markers_survive_a_round_trip_with_the_rest_of_the_file"],
    ),
    (
        "a marker's name is not NUL-terminated",
        "        labl.push(0);",
        "",
        ["markers_survive_a_round_trip_with_the_rest_of_the_file"],
    ),
    (
        "a cut keeps the markers outside it",
        "        .filter(|c| (start..end).contains(&u64::from(c.frame)))",
        "        .filter(|_| true)",
        ["a_cut_is_the_stretch_as_stored_with_its_markers"],
    ),
    (
        "a cut's markers are not moved",
        "            frame: u32::try_from(u64::from(c.frame).saturating_sub(start)).unwrap_or(u32::MAX),",
        "            frame: c.frame,",
        ["a_cut_is_the_stretch_as_stored_with_its_markers"],
    ),
    (
        "a cut drops the title",
        "    for chunk in all.iter().filter(|c| c.is_list(bytes, b\"INFO\")) {",
        "    for chunk in all.iter().filter(|_| false) {",
        ["a_cut_is_the_stretch_as_stored_with_its_markers"],
    ),
    (
        "an extensible header is read as its wrapper",
        "    let tag = if tag == 0xFFFE {",
        "    let tag = if false {",
        ["extensible_headers_and_other_chunks_are_read"],
    ),
    (
        "a frame size that contradicts the header is believed",
        "    if expected != Some(block) {",
        "    if false {",
        ["what_is_not_a_wav_this_reads_says_so"],
    ),
    (
        "no dither",
        "        let v = f64::from(s) * f64::from(scale) + f64::from(dither.triangular());",
        "        let v = f64::from(s) * f64::from(scale);",
        ["dither_keeps_what_is_quieter_than_a_step"],
    ),
    (
        "the resampler's weights are not normalised",
        "                (*a / weight) as f32",
        "                (*a) as f32",
        ["a_constant_stays_constant_to_the_ends"],
    ),
    (
        "no anti-alias filter",
        "    let cutoff = 0.5 * (1.0 / ratio).min(1.0) * 0.97;",
        "    let cutoff = 0.5 * 0.97;",
        ["downsampling_does_not_alias"],
    ),
    (
        "a 5.1 mono mix keeps the subwoofer",
        "                let sum = ch(0) + ch(1) + ch(2) + ch(4) + ch(5);",
        "                let sum = ch(0) + ch(1) + ch(2) + ch(3) + ch(5);",
        ["channels_are_mixed_the_standard_ways"],
    ),
    (
        "mono is sent to every channel",
        "                    samples.push(if i < 2 { ch(0) } else { 0.0 });",
        "                    samples.push(ch(0) + 0.0 * i as f32);",
        ["channels_are_mixed_the_standard_ways"],
    ),
    (
        "the header's byte rate is wrong",
        "        .checked_mul(u32::from(block))",
        "        .checked_mul(1)",
        ["a_cd_quality_header_is_the_standard_forty_four_bytes"],
    ),
    (
        "an odd data chunk is not padded",
        "    if pad == 1 {\n"
        "        out.push(0);\n"
        "    }",
        "",
        ["an_odd_chunk_is_padded"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "wavpcm", timeout=600, only=only))
