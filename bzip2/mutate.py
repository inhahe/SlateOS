"""Mutation test for bzip2.

The port from libbzip2 1.0.8: each row puts back one way of being *not*
libbzip2 -- a check it makes skipped, a choice it makes changed -- and names
the test that has to notice. Most compressor rows are caught by the one test
that holds the output to libbzip2's bytes; the decoder's by the corpus of
corruptions libbzip2 itself judged, or by a test aimed at the one behaviour.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

DECODE_SRC = Path(__file__).parent / "src" / "decompress.rs"
ENCODE_SRC = Path(__file__).parent / "src" / "compress.rs"
SORT_SRC = Path(__file__).parent / "src" / "blocksort.rs"
HUFF_SRC = Path(__file__).parent / "src" / "huffman.rs"

# (name, old, new, [tests that must fail])
DECODE_MUTATIONS = [
    (
        "the stream CRC is not checked",
        "if stored != combined_crc {",
        "if stored != combined_crc && false {",
        [
            "a_corrupted_stream_is_refused_exactly_when_libbzip2_refuses_it",
            "what_follows_a_stream_is_read_as_bzip2_does",
        ],
    ),
    (
        "a block's CRC is not checked",
        "if computed != stored_crc {",
        "if computed != stored_crc && false {",
        ["a_corrupted_stream_is_refused_exactly_when_libbzip2_refuses_it"],
    ),
    (
        "the streams' CRCs combine without the rotation",
        "combined_crc = combined_crc.rotate_left(1) ^ block_crc;",
        "combined_crc ^= block_crc;",
        # A single-block stream's rotation is of zero: only the multi-block
        # fixtures notice, which the round trip (one block) is not.
        ["decompress_reads_every_fixture", "the_limit_holds_across_blocks"],
    ),
    (
        "bytes that cannot begin a stream are an error after one",
        "if !could_begin_stream(rest) {",
        "if !could_begin_stream(rest) && false {",
        ["what_follows_a_stream_is_read_as_bzip2_does"],
    ),
    (
        "the output limit is not checked",
        "if count > limit.saturating_sub(out.len()) {",
        "if count > limit.saturating_sub(out.len()) && false {",
        [
            "the_limit_is_checked_before_the_output_grows_past_it",
            "the_limit_holds_across_blocks",
            "the_limit_spans_streams",
        ],
    ),
    (
        "a randomised block is not un-randomised",
        "byte ^= mask.next();",
        "let _ = mask.next();",
        ["a_randomised_block_is_read"],
    ),
    (
        "the randomisation table wraps an entry early",
        "if self.t_pos == R_NUMS.len() {",
        "if self.t_pos == R_NUMS.len() - 1 {",
        ["a_randomised_block_is_read"],
    ),
    (
        "more selectors than a block holds are refused",
        "if n_selectors < 1 {",
        "if n_selectors < 1 || n_selectors > MAX_SELECTORS {",
        ["more_selectors_than_a_block_needs_are_read_and_ignored"],
    ),
    (
        "only the first selector is kept",
        "if i < MAX_SELECTORS {",
        "if i < 1 {",
        ["decompress_reads_every_fixture", "round_trips_at_every_level"],
    ),
    (
        "RUNB counts as RUNA",
        "                    n.wrapping_mul(2)\n                });",
        "                    n\n                });",
        ["decompress_reads_every_fixture", "round_trips_at_every_level"],
    ),
    (
        "four equal bytes may end a block",
        "            if used >= nblock {\n                return Err(Error::InvalidRun);",
        "            if used >= nblock && false {\n                return Err(Error::InvalidRun);",
        ["a_run_without_its_count_byte_is_refused"],
    ),
]

ENCODE_MUTATIONS = [
    (
        "a tie goes to the later table",
        "if i32::from(c) < bc {",
        "if i32::from(c) <= bc {",
        ["compress_writes_libbzip2s_bytes"],
    ),
    (
        "three refinement passes, not four",
        "for _ in 0..N_ITERS {",
        "for _ in 1..N_ITERS {",
        ["compress_writes_libbzip2s_bytes"],
    ),
    (
        "the initial ranges do not alternate",
        "&& (n_groups_i - n_part) % 2 == 1",
        "&& (n_groups_i - n_part) % 2 == 0",
        ["compress_writes_libbzip2s_bytes"],
    ),
    (
        "a block fills to the level's whole size",
        "nblock_max: size.wrapping_sub(19),",
        "nblock_max: size,",
        ["compress_writes_libbzip2s_bytes"],
    ),
    (
        "runs stop at 254",
        "|| self.state_in_len == 255 {",
        "|| self.state_in_len == 254 {",
        ["compress_writes_libbzip2s_bytes", "runs_are_coded_as_libbzip2_codes_them"],
    ),
    (
        "the compressor's stream CRC is not rotated",
        "self.combined_crc = self.combined_crc.rotate_left(1) ^ block_crc;",
        "self.combined_crc ^= block_crc;",
        ["compress_writes_libbzip2s_bytes"],
    ),
    (
        "blocks are marked randomised",
        "self.w.put(1, 0);",
        "self.w.put(1, 1);",
        ["compress_writes_libbzip2s_bytes", "round_trips_at_every_level"],
    ),
    (
        "a short block gets three tables",
        "0..200 => 2,",
        "0..200 => 3,",
        ["compress_writes_libbzip2s_bytes"],
    ),
    (
        "zero runs are coded off by one",
        "z = (z.wrapping_sub(2)) / 2;",
        "z = (z.wrapping_sub(1)) / 2;",
        ["zero_runs_are_bijective_base_two", "compress_writes_libbzip2s_bytes"],
    ),
]

SORT_MUTATIONS = [
    (
        "the main sort orders the first twelve bytes backwards",
        "        for (c1, c2) in a.iter().zip(b) {\n            if c1 != c2 {\n                return c1 > c2;",
        "        for (c1, c2) in a.iter().zip(b) {\n            if c1 != c2 {\n                return c1 < c2;",
        ["the_main_sort_sorts_large_blocks", "compress_writes_libbzip2s_bytes"],
    ),
    (
        "the fallback's insertion sort runs backwards",
        "        while j <= hi && ec_tmp > ld(eclass, ux(ld(fmap, ix(j)))) {\n            st(fmap, ix(j - 1), ld(fmap, ix(j)));",
        "        while j <= hi && ec_tmp < ld(eclass, ux(ld(fmap, ix(j)))) {\n            st(fmap, ix(j - 1), ld(fmap, ix(j)));",
        ["the_fallback_sorts_small_blocks", "compress_writes_libbzip2s_bytes"],
    ),
]

HUFF_MUTATIONS = [
    (
        "equal weights ignore the subtree's depth",
        "weights | depth.wrapping_add(1)",
        "weights | 1",
        ["compress_writes_libbzip2s_bytes"],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [
        (DECODE_SRC, DECODE_MUTATIONS),
        (ENCODE_SRC, ENCODE_MUTATIONS),
        (SORT_SRC, SORT_MUTATIONS),
        (HUFF_SRC, HUFF_MUTATIONS),
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
        results.append(sweep(src, rows, "bzip2", timeout=900, only=mine or None))
    raise SystemExit(max(results))
