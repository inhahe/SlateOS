"""Mutation test for sevenz.

The port of 7-Zip's decoding -- LZMA and LZMA2 (`LzmaDec.c`, `Lzma2Dec.c`
and the `Lzma2Decoder` / `Lzma2DecMt` / `MtDec` drivers), PPMd, BCJ2, the
ARM64 and RISC-V converters, 7z AES -- the method names 7-Zip lists, and the
Deflate decoder written to 7-Zip's rules: each row puts back one way of not
being 7-Zip -- a check it makes skipped, a rule it keeps broken -- and names
the tests that have to notice. Many are caught by the tests that hold the
reader to 7-Zip 26.00's own verdicts: on every one-byte corruption of ten
small archives and on the chunk headers and edges of two LZMA2 archives of
several chunks, tested by 7-Zip with several threads and with one
(`tests/data/mutations*.txt`), and on the crafted archives no mutant makes
(`tests/data/crafted.txt`).

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
CRAFTED = "crafted_archives_fail_where_7zip_says"
METHODS = "methods_are_named_as_7zip_names_them"

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

INF_PAST = "input_past_the_end_reads_as_ones_until_a_check"
INF_MEGABYTE = "a_megabyte_boundary_checks_for_a_bit_used_past_the_end"
INF_FULL = "a_full_output_must_end_its_block"
INF_CUT = "a_match_running_past_the_output_is_cut"
INF_CODES = "an_incomplete_code_is_kept_and_an_over_full_one_refused"
INF_286 = "codes_286_and_287_are_matches_of_three"
INF_DIST = "distances_reach_back_no_further_than_the_output_or_the_window"
INF_285 = "deflate64_reads_sixteen_bits_after_length_285"
INF_HDIST = "a_dynamic_block_may_declare_32_distance_codes_only_in_deflate64"
INF_USED = "the_input_used_ends_with_the_final_block"
INF_TYPE3 = "a_block_type_of_3_is_refused"
INF_REPEAT = "a_repeat_with_nothing_before_it_is_refused"
INF_ZLIB = "a_stream_zlib_wrote_is_read"
INF_STORED = "a_stored_block_is_read"
# Not rows, and why:
# - the check at the top of a request's loop for a bit used past the end,
#   and those inside a block header: past the end every bit is 1, so the
#   next block header read is type 3 and fails whatever is checked, and the
#   check at the end of a request catches the rest -- only a stream ending
#   inside a header whose code-length code decodes 1-bits could tell, and
#   neither the corpus nor a test makes one;
# - the input step that ends a request early at a block boundary: it moves
#   the megabyte checks only for a stream that takes 2 MiB of input in one
#   request, more than twice its output;
# - a stored block after a full output that still holds data: with the
#   check gone the request loops forever on it, as 7-Zip would -- the check
#   is what stops it;
# - a code-length repeat running past the lengths: no test makes one.
INFLATE = [
    (
        "input past the end reads as zeros",
        "            .unwrap_or(0xFF)",
        "            .unwrap_or(0)",
        [INF_PAST],
    ),
    (
        "the lookahead is drawn five bytes ahead",
        "let want = (self.taken / 8).saturating_add(4);",
        "let want = (self.taken / 8).saturating_add(5);",
        [INF_PAST],
    ),
    (
        "nothing is checked before a symbol",
        "                if self.bits.over_drawn() {",
        "                if false {",
        [INF_PAST],
    ),
    (
        "the check before a symbol is the full one",
        "                if self.bits.over_drawn() {",
        "                if self.bits.over_read() {",
        [INF_PAST],
    ),
    (
        "over-drawing is counted from three bytes past the end",
        "        self.drawn > self.len().saturating_add(4)",
        "        self.drawn > self.len().saturating_add(3)",
        [INF_PAST],
    ),
    (
        "a megabyte's end is not checked",
        "        if self.bits.over_read() {\n            return Err(Fail);\n        }\n        Ok(())\n    }\n\n    /// The length and distance",
        "        Ok(())\n    }\n\n    /// The length and distance",
        [INF_MEGABYTE],
    ),
    (
        "a full output need not end its block",
        "                if self.lit.decode(&mut self.bits) != Some(END_OF_BLOCK) {",
        "                if self.lit.decode(&mut self.bits).is_none() {",
        [INF_FULL],
    ),
    (
        "a match running past the output is written whole",
        "                let now = len.min(want);",
        "                let now = len;",
        [INF_CUT],
    ),
    (
        "the final block is not looked for",
        "                if self.final_block {\n                    self.finished = true;",
        "                if false {\n                    self.finished = true;",
        [INF_ZLIB, INF_STORED],
    ),
    (
        "codes 286 and 287 are refused",
        "        let (base, extra) = if self.deflate64 && i == 28 {",
        "        if i > 28 {\n            return Err(Fail);\n        }\n        let (base, extra) = if self.deflate64 && i == 28 {",
        [INF_286, CRAFTED],
    ),
    (
        "Deflate64's length 285 takes no extra bits",
        "        let (base, extra) = if self.deflate64 && i == 28 {",
        "        let (base, extra) = if false && i == 28 {",
        [INF_285],
    ),
    (
        "Deflate allows 32 distance codes",
        "        if !self.deflate64 && n_dist > DIST_SYMBOLS_DEFLATE {",
        "        if !self.deflate64 && n_dist > DIST_SYMBOLS {",
        [INF_HDIST],
    ),
    (
        "Deflate's window is 64 KiB",
        "        window: if deflate64 { 1 << 16 } else { 1 << 15 },",
        "        window: 1 << 16,",
        [INF_DIST],
    ),
    (
        "a distance may reach the byte before the first",
        "        if distance >= reach {",
        "        if distance > reach {",
        [INF_DIST],
    ),
    (
        "an over-full code is kept",
        "        if sum > 1 << max_bits {",
        "        if false {",
        [INF_CODES],
    ),
    (
        "an incomplete code is refused",
        "        if sum > 1 << max_bits {",
        "        if sum != 1 << max_bits && sum != 0 {",
        [INF_CODES, CRAFTED],
    ),
    (
        "the code-length lengths come in another order",
        "    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,",
        "    15, 1, 14, 2, 13, 3, 12, 4, 11, 5, 10, 6, 9, 7, 8, 0, 18, 17, 16,",
        [INF_ZLIB],
    ),
    (
        "a repeat with nothing before it repeats a zero",
        "let last = i.checked_sub(1).and_then(|j| lengths.get(j)).ok_or(Fail)?;",
        "let last = lengths.get(i.saturating_sub(1)).ok_or(Fail)?;",
        [INF_REPEAT],
    ),
    (
        "a stored block's length need not match its complement",
        "            if len != !nlen {",
        "            if false {",
        [INF_STORED],
    ),
    (
        "the input used counts the lookahead",
        "        self.taken.div_ceil(8)\n",
        "        self.drawn\n",
        [INF_USED],
    ),
    (
        "block type 3 is read as dynamic",
        "        if kind > 2 || self.bits.over_read() {",
        "        if self.bits.over_read() {",
        [INF_TYPE3],
    ),
]

BRANCH = [
    (
        "ARM64 BL is left as encoded",
        "        if v.wrapping_sub(0x9400_0000) & 0xFC00_0000 == 0 {",
        "        if false {",
        [MADE],
    ),
    (
        "ARM64 ADRP is left as encoded",
        "            if v & 0x9F00_0000 == 0 {",
        "            if false {",
        [MADE],
    ),
    (
        "RISC-V JAL is taken for AUIPC",
        "        if a & 8 == 0 {\n            // JAL",
        "        if false {\n            // JAL",
        [MADE],
    ),
    (
        "RISC-V AUIPC with x0 or x2 is left as encoded",
        "            if riscv_check_2(v, r) {",
        "            if false {",
        [MADE],
    ),
    (
        "RISC-V AUIPC pairs are left as encoded",
        "            if riscv_check_1(v, b) {",
        "            if false {",
        [MADE],
    ),
]

AES = [
    (
        "the key is one round short",
        "    for round in 0..rounds {",
        "    for round in 1..rounds {",
        ["the_hashed_key_is_sha256_over_its_rounds", MADE],
    ),
    (
        "the round counter is not hashed",
        "        sha.update(&round.to_le_bytes());\n",
        "",
        ["the_hashed_key_is_sha256_over_its_rounds", MADE],
    ),
    (
        "blocks are not chained",
        "            *b ^= p;",
        "            let _ = p;",
        [MADE],
    ),
    (
        "a short last block is no damage",
        "    let ok = whole == input.len();",
        "    let ok = true;",
        ["a_short_last_block_is_damage"],
    ),
    (
        "a rounds power past 24 is taken",
        "    if props.cycles_power > CYCLES_POWER_MAX && props.cycles_power != CYCLES_POWER_RAW {",
        "    if false {",
        ["properties_are_read_as_7zip_reads_them"],
    ),
    (
        "the password is UTF-8",
        "    password.encode_utf16().flat_map(u16::to_le_bytes).collect()",
        "    password.as_bytes().to_vec()",
        [MADE],
    ),
    (
        "the raw key leaves out the salt",
        ".zip(props.salt.iter().chain(password))",
        ".zip(password)",
        ["the_raw_key_is_salt_and_password"],
    ),
]

NAMES = [
    (
        "coders are named first to last",
        "    parts.reverse();\n",
        "",
        [METHODS],
    ),
    (
        "LZMA's lc is always named",
        "                    if lc != 3 {",
        "                    if true {",
        [METHODS],
    ),
    (
        "the AES rounds keep their flag bits",
        '                .map_or_else(String::new, |&b| alloc::format!("{}", b & 0x3F)),',
        '                .map_or_else(String::new, |&b| alloc::format!("{b}")),',
        [METHODS],
    ),
    (
        "an odd LZMA2 dictionary is named as a power",
        "    } else if d & 1 == 0 {",
        "    } else if true {",
        ["sizes_are_written_as_7zip_writes_them"],
    ),
    (
        "sizes in megabytes are named in kilobytes",
        "    if val & ((1 << 20) - 1) == 0 {",
        "    if false {",
        ["sizes_are_written_as_7zip_writes_them"],
    ),
]

DECODE = [
    (
        "BZip2 data after the end is not flagged",
        "                    d.after_end = used < data.len();",
        "                    d.after_end = false;",
        [CRAFTED],
    ),
    (
        "Deflate data after the end is not flagged",
        "            d.after_end = r.ok && r.used < data.len();",
        "            d.after_end = false;",
        [CRAFTED],
    ),
    (
        "BZip2 takes properties",
        "            // 23.00 a coder given some it cannot take is refused.\n            if !props.is_empty() {",
        "            // 23.00 a coder given some it cannot take is refused.\n            if false {",
        [CRAFTED],
    ),
    (
        "Deflate takes properties",
        "        method::DEFLATE | method::DEFLATE64 => {\n            if !props.is_empty() {",
        "        method::DEFLATE | method::DEFLATE64 => {\n            if false {",
        [CRAFTED],
    ),
    (
        "Deflate64 is read as Deflate",
        "        let r = inflate::decode(&data, size, method == method::DEFLATE64);",
        "        let r = inflate::decode(&data, size, false);",
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
        (SRC / "inflate.rs", INFLATE),
        (SRC / "branch.rs", BRANCH),
        (SRC / "aes7z.rs", AES),
        (SRC / "names.rs", NAMES),
        (SRC / "decode.rs", DECODE),
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
