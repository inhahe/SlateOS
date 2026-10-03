"""Mutation test for sevenz.

The port of 7-Zip's LZMA and LZMA2 decoding (`LzmaDec.c`, `Lzma2Dec.c` and
the `Lzma2Decoder` / `Lzma2DecMt` / `MtDec` drivers): each row puts back one
way of not being 7-Zip -- a check it makes skipped, a rule it keeps broken --
and names the tests that have to notice. Most are caught by the tests that
hold the reader to 7-Zip 26.00's own verdicts on every one-byte corruption
of three small archives and on the chunk headers and edges of two LZMA2
archives of several chunks, tested by 7-Zip with several threads and with
one (`tests/data/mutations*.txt`).

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

CORPUS = "a_corrupted_archive_fails_where_7zip_says"
CORPUS_ONE = "a_corrupted_archive_fails_where_7zip_with_one_thread_says"
MADE = "archives_7zip_made_are_read"
CHUNKY = "lzma2_of_many_chunks_and_blocks_is_read"

# (name, old, new, [tests that must fail])
LZMA_DEC = [
    (
        "an LZMA stream's first byte may be anything",
        "if self.temp_buf_size != 0 && self.temp_buf[0] != 0 {",
        "if self.temp_buf_size != 0 && self.temp_buf[0] != 0 && false {",
        ["the_first_byte_must_be_zero", CORPUS, CORPUS_ONE],
    ),
    (
        "a repeated match may open a stream",
        "&& self.code >= BAD_REP_CODE {",
        "&& self.code >= BAD_REP_CODE && false {",
        # Corruption cannot tell: the repeated match then fails as "cannot
        # happen", which a 7z reader reports as damage too.
        ["the_first_symbol_may_not_be_a_repeated_match"],
    ),
    (
        "a distance may reach the byte before the first",
        "if distance >= reach {",
        "if distance > reach {",
        # The same: the copy then fails as "cannot happen".
        ["a_match_may_not_reach_before_the_first_byte"],
    ),
    (
        "a match running past the limit is refused whole, as liblzma does",
        "                if rem == 0 {\n",
        "                if rem < len as usize {\n                    len += MATCH_SPEC_LEN_ERROR_DATA;\n",
        [CORPUS, CORPUS_ONE, "a_match_running_past_the_limit_is_copied_to_it"],
    ),
    (
        "the end marker may leave the range coder unfinished",
        "                if self.code != 0 {\n                    return Ok(Step::fault(consumed, Status::NotSpecified, Fault::Data));",
        "                if self.code != 0 && false {\n                    return Ok(Step::fault(consumed, Status::NotSpecified, Fault::Data));",
        ["the_end_marker_must_finish_the_range_coder"],
    ),
    (
        "a symbol is not followed by a normalisation",
        "        normalize!();\n\n        self.range = range;",
        "\n        self.range = range;",
        [MADE, "a_sound_stream_decodes_and_ends_with_its_marker"],
    ),
    (
        "the look-ahead ignores the normalisation after a symbol",
        "        // The final NORMALIZE_CHECK: only whether its byte is there counts.\n        if range < TOP_VALUE {\n            if buf >= input.len() {\n                return Ok(None);\n            }\n            buf += 1;\n        }\n",
        "",
        [MADE, "any_split_of_the_input_decodes_the_same"],
    ),
    (
        "a call may pass the dictionary's size before checking distances by it",
        "            if limit.saturating_sub(out.len()) > rem {",
        "            if false && limit.saturating_sub(out.len()) > rem {",
        ["a_distance_past_the_dictionary_is_damage_once_it_is_full"],
    ),
]

LZMA2_DEC = [
    (
        "an LZMA chunk may read past its packed size",
        "                in_cur = in_cur.min(self.pack_size as usize);\n                let Some(chunk)",
        "                let Some(chunk)",
        [CORPUS, CORPUS_ONE, "a_chunk_gets_its_packed_size_and_no_more"],
    ),
    (
        "a chunk may leave packed bytes unread",
        "                        || self.pack_size != 0\n",
        "",
        [CORPUS, CORPUS_ONE],
    ),
    (
        "a stored chunk may come first without a dictionary reset",
        "} else if b > 2 || self.need_init_level == 0xE0 {",
        "} else if b > 2 {",
        ["the_first_chunk_must_reset_the_dictionary"],
    ),
    (
        "an LZMA chunk may come first without a dictionary reset",
        "if b < self.need_init_level {",
        "if b < self.need_init_level && false {",
        ["the_first_chunk_must_reset_the_dictionary", CORPUS, CORPUS_ONE],
    ),
    (
        "lc and lp together may pass four",
        "if lc + lp > LCLP_MAX {",
        "if lc + lp > LCLP_MAX + 4 {",
        ["lc_and_lp_together_are_at_most_four"],
    ),
]

LZMA_CODER = [
    (
        "an LZMA stream need not use its packed bytes up",
        "let ok = stopped_well && step.consumed == input.len();",
        "let ok = stopped_well;",
        ["lzma_with_an_end_marker_after_its_size_is_sound"],
    ),
    (
        "an LZMA end marker may come before the size",
        "            Status::FinishedWithMark => out.len() == out_size,\n            // `outFinished`",
        "            Status::FinishedWithMark => true,\n            // `outFinished`",
        ["lzma_with_an_end_marker_after_its_size_is_sound"],
    ),
    (
        "an LZMA2 stream need not use its packed bytes up",
        "let ok = ok && in_processed == input.len() && out.len() == out_size;",
        "let ok = ok && out.len() == out_size;",
        ["a_wrong_size_or_trailing_input_is_damage"],
    ),
    (
        "an LZMA2 stream may fall short of its size",
        "let ok = ok && in_processed == input.len() && out.len() == out_size;",
        "let ok = ok && in_processed == input.len();",
        ["a_wrong_size_or_trailing_input_is_damage"],
    ),
    (
        "a walked block may decode past what the walk found",
        "                block_base + block.out_pre_size,\n",
        "                out_size,\n",
        [CORPUS, "a_chunk_running_past_the_end_parts_the_two_ways"],
    ),
    (
        "a walked block with no output is decoded, not handed to the sequence",
        "            } else if block.out_pre_size == 0 {",
        "            } else if block.out_pre_size == 0 && false {",
        [CORPUS],
    ),
    # Not a row: "the walk never cuts a block" (`parse_pos >= SMALL_BLOCK`
    # made never true). Where blocks are cut decides how big they are, not
    # what comes out: a damaged stream fails at the same byte cut either
    # way, and a walk that cannot settle a block falls back to the last
    # reset it passed (`dic_pos_point`) -- so the mutant is equivalent at
    # the verdict level, and 7-Zip's 16 KiB is kept for fidelity alone.
    (
        "with several threads, decode in sequence",
        "        Threads::Many => decode_mt(prop, input, &mut out, out_size),",
        "        Threads::Many => decode_st(prop, input, &mut out, out_size),",
        [CORPUS, "a_chunk_running_past_the_end_parts_the_two_ways"],
    ),
]

# PPMd: the model must be 7-Zip's to the byte, so most breakage shows as a
# 7-Zip archive that no longer decodes -- above all the one whose 64 KiB
# model restarts several times, where the allocator's choices decide when.
PPMD_RESTARTS = "the_small_model_restarts_and_still_decodes"
PPMD_RC = "the_range_coder_must_start_with_a_zero_and_end_finished"
PPMD7 = [
    (
        "a PPMd range coder may start with any byte",
        "        if self.read_byte() != 0 {",
        "        if self.read_byte() != 0 && false {",
        [PPMD_RC, CORPUS],
    ),
    (
        "a PPMd stream may end with the range coder unfinished",
        "Symbol::Byte(_) => out.len() == out_size && ppmd.code == 0,",
        "Symbol::Byte(_) => out.len() == out_size,",
        [PPMD_RC, CORPUS],
    ),
    (
        "a PPMd stream need not use its packed bytes up",
        "let ok = ok && ppmd.pos == input.len();",
        "let ok = ok;",
        [PPMD_RC, CORPUS],
    ),
    (
        "reading past a PPMd stream's end is not damage",
        "    if ppmd.extra {\n        // CHECK_EXTRA_ERROR",
        "    if false {\n        // CHECK_EXTRA_ERROR",
        [PPMD_RC, CORPUS],
    ),
    (
        "free blocks are never glued together",
        "if self.rd16(node2) != 0 || nu >= 0x10000 || self.broken {",
        "if true {",
        [MADE, PPMD_RESTARTS],
    ),
    (
        "a split block's odd remainder is lost",
        "        let mut i = self.u2i(nu);\n        if self.i2u(i) != nu {",
        "        let mut i = self.u2i(nu);\n        if false {",
        [MADE, PPMD_RESTARTS],
    ),
    (
        "the model keeps going when its text reaches the units",
        "        if self.text >= self.units_start {",
        "        if self.text >= self.units_start && false {",
        [MADE, PPMD_RESTARTS],
    ),
    (
        "rescaling keeps the symbols it halved to nothing",
        "        if self.freq(s) == 0 {",
        "        if false {",
        [MADE, PPMD_RESTARTS],
    ),
    (
        "a binary context's probability does not learn from a hit",
        "*prob = low16(pr + (1 << INT_BITS));",
        "*prob = low16(pr);",
        [MADE, PPMD_RESTARTS],
    ),
    (
        "the escape estimator ignores how many symbols were masked",
        "            + 4 * u32::from(num_masked > non_masked)\n",
        "\n",
        [MADE, PPMD_RESTARTS],
    ),
    (
        "an escape estimator never speeds up",
        "                if u32::from(see.shift) < PERIOD_BITS {",
        "                if false {",
        [MADE, PPMD_RESTARTS],
    ),
]

BCJ2 = [
    (
        "a BCJ2 target is not relative to where it is",
        "v = self.be32(cj).wrapping_sub(ip);",
        "v = self.be32(cj);",
        [MADE],
    ),
    (
        "a BCJ2 stream may end with its range coder unfinished",
        "        && dec.code == 0\n",
        "\n",
        [CORPUS],
    ),
    (
        "bytes left over of a CALL or JUMP word are not damage",
        "            if extra.get(state).copied().unwrap_or(0) != 0 {\n                crit_ok = false;",
        "            if extra.get(state).copied().unwrap_or(0) != 0 && false {\n                crit_ok = false;",
        [CORPUS],
    ),
    (
        "E8 is not a candidate opcode",
        "                if ((b + (0x100 - 0xE8)) & 0xFE) == 0",
        "                if ((b + (0x100 - 0xE8)) & 0xFE) == 1",
        [MADE],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:]
    tables = [
        (SRC / "lzma_dec.rs", LZMA_DEC),
        (SRC / "lzma2_dec.rs", LZMA2_DEC),
        (SRC / "lzma_coder.rs", LZMA_CODER),
        (SRC / "ppmd7.rs", PPMD7),
        (SRC / "bcj2.rs", BCJ2),
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
        results.append(sweep(src, rows, "sevenz", timeout=900, only=mine or None))
    raise SystemExit(max(results))
