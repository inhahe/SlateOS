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
        "a streaming writer's size is believed",
        "            let len = if size == 0 || size > remaining {",
        "            let len = if false {",
        ["extensible_headers_and_other_chunks_are_read"],
    ),
    (
        "chunks are not padded to even",
        "        at = body.saturating_add(size).saturating_add(size & 1);",
        "        at = body.saturating_add(size);",
        ["extensible_headers_and_other_chunks_are_read"],
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
