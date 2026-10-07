"""Mutation test for xz.

The port from liblzma 5.2.5: each row puts back one way of not being liblzma
-- a check it makes skipped, a rule it keeps broken -- and names the test that
has to notice. Most are caught by the tests that hold the decoder to
liblzma's own verdicts: on XZ Utils' test files, on files xz 5.2.5 made, and
on every one-byte corruption of four small streams.

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

SRC = Path(__file__).parent / "src"

CORPUS = "a_corrupted_file_is_refused_exactly_when_liblzma_refuses_it"
OWN_FILES = "xz_utils_own_test_files"
MADE = "files_xz_made"
# The encoder's: xz's bytes for 136 settings and inputs, and for the files
# in tests/data/made.
ENC = "what_xz_writes_for_every_preset_and_setting_is_written_byte_for_byte"
ENC_MADE = "files_xz_made_are_written_byte_for_byte"

# (name, old, new, [tests that must fail])
LZMA = [
    (
        "the range coder's first byte may be anything",
        "if first != 0 {",
        "if first != 0 && false {",
        [CORPUS, "the_first_range_coder_byte_must_be_zero"],
    ),
    (
        "a chunk may end with the range coder unfinished",
        "        if !rc.is_finished() {\n            return Err(Error::InvalidData);\n        }\n        Ok(End::Size)",
        "        Ok(End::Size)",
        [CORPUS],
    ),
    (
        "the end-of-payload marker is allowed with a known size",
        "if size.is_some() {",
        "if size.is_some() && false {",
        [CORPUS],
    ),
    (
        "a distance may reach past the dictionary",
        "if usize::try_from(rep0).unwrap_or(usize::MAX) >= dict.full(out) {",
        "if usize::try_from(rep0).unwrap_or(usize::MAX) > dict.full(out) {",
        # Corruption finds it only where a check then fails too; a stream
        # written to the edge isolates it.
        ["a_distance_must_lie_inside_the_dictionary"],
    ),
    (
        "a repeated match is allowed before any output",
        "if dict.full(out) == 0 {",
        "if dict.full(out) == 0 && false {",
        ["a_repeated_match_needs_output_to_repeat"],
    ),
]

LZMA2 = [
    (
        "a chunk may use fewer bytes than it declares",
        "if used != compressed {",
        "if used > compressed {",
        [CORPUS],
    ),
    (
        "the first chunk need not reset the dictionary",
        "        } else if need_dictionary_reset {\n            return Err(Error::InvalidData);",
        "        } else if need_dictionary_reset && false {\n            return Err(Error::InvalidData);",
        ["the_first_chunk_must_reset_the_dictionary"],
    ),
]

STREAM = [
    (
        "a block's check is not verified",
        "if computed != stored {",
        "if computed != stored && false {",
        [OWN_FILES, CORPUS],
    ),
    (
        "stream padding need not be a multiple of four",
        "return if padding.is_multiple_of(4) {",
        "return if true {",
        # Padding cut short at the very end of the file: XZ Utils'
        # bad-0pad-empty.xz and made/badly-padded-at-end.xz.
        [OWN_FILES, MADE],
    ),
    (
        "the index is not compared with the blocks",
        "if unpadded != block_unpadded || uncompressed != block_uncompressed {",
        "if (unpadded != block_unpadded || uncompressed != block_uncompressed) && false {",
        # A one-byte change to a record fails the index's CRC first; XZ
        # Utils' bad-2-index-1 and -2 have wrong records under a right CRC.
        [OWN_FILES],
    ),
    (
        "the footer's record of the index size is not checked",
        "if backward_size != index_size as u64 {",
        "if backward_size != index_size as u64 && false {",
        # The footer's CRC covers it, so only bad-0-backward_size.xz, wrong
        # under a right CRC, isolates the rule.
        [OWN_FILES],
    ),
    (
        "a later stream's bad magic is called a different format",
        "            Error::NotXz\n        } else {\n            Error::InvalidData",
        "            Error::NotXz\n        } else {\n            Error::NotXz",
        [OWN_FILES],
    ),
    (
        "reserved block flags are accepted",
        "if flags & 0x3c != 0 {",
        "if flags & 0x3c != 0 && false {",
        # made/reserved-flag.xz: the flag set, the header's CRC made right.
        [MADE],
    ),
]

FILTERS = [
    (
        "a chain is undone first filter first",
        "for filter in chain.iter().rev() {",
        "for filter in chain.iter() {",
        [MADE],
    ),
    (
        "a misaligned start offset is accepted",
        "if start & arch.alignment().wrapping_sub(1) != 0 {",
        "if start & arch.alignment().wrapping_sub(1) != 0 && false {",
        ["a_start_offset_must_keep_the_alignment"],
    ),
]

VLI = [
    (
        "a padded integer is accepted",
        "if b == 0 && i > 0 {",
        "if b == 0 && i > 0 && false {",
        ["a_padded_or_overlong_integer_is_refused", OWN_FILES],
    ),
]

ALONE = [
    (
        "bytes after an .lzma stream are accepted",
        "if rc.pos != input.len() {",
        "if rc.pos != input.len() && false {",
        ["bytes_after_an_lzma_or_raw_stream_are_refused"],
    ),
]

# --- the encoder ---------------------------------------------------------------

ENC_RC = [
    (
        "a carry out of the range coder is dropped",
        "out.push(self.cache.wrapping_add(carry));",
        "out.push(self.cache);",
        [ENC, ENC_MADE],
    ),
]

ENC_PRICE = [
    (
        "a price-table entry is off by one",
        "    16, 16, 16, 15, 15, 15, 14, 14,",
        "    17, 16, 16, 15, 15, 15, 14, 14,",
        ["the_price_table_is_price_tablegens", ENC],
    ),
    (
        "direct bits are priced as free",
        "    bits << BIT_PRICE_SHIFT_BITS",
        "    bits",
        [ENC],
    ),
    (
        "a distance slot ignores its second bit",
        "        (i + i) + ((dist >> (i - 1)) & 1)",
        "        i + i",
        ["distance_slots_are_fastposs", ENC, ENC_MADE],
    ),
]

ENC_MF = [
    (
        "the binary trees search deeper by default",
        "            16 + nice_len / 2",
        "            16 + nice_len",
        [ENC],
    ),
    (
        "the hash chains search deeper by default",
        "            4 + nice_len / 4",
        "            4 + nice_len / 2",
        [ENC],
    ),
    (
        "the four-byte hash ignores the fourth byte",
        "(t3 ^ (crc(b3) << 5)) & self.hash_mask",
        "t3 & self.hash_mask",
        [ENC, ENC_MADE],
    ),
    (
        "a match of nice_len is not extended",
        "            if len_best == self.nice_len {",
        "            if false && len_best == self.nice_len {",
        [ENC],
    ),
    (
        "normalisation leaves the position where it was",
        "        self.pos = self.pos.wrapping_sub(subvalue);",
        "",
        ["normalisation_changes_no_match"],
    ),
    (
        "a hash chain reaches one past the dictionary",
        "            if depth == 0 || delta >= self.cyclic_size {\n                return count;",
        "            if depth == 0 || delta > self.cyclic_size {\n                return count;",
        [ENC],
    ),
    (
        "a tree reaches one past the dictionary",
        "            if depth == 0 || delta >= self.cyclic_size {\n                self.set_son(ptr0, EMPTY);",
        "            if depth == 0 || delta > self.cyclic_size {\n                self.set_son(ptr0, EMPTY);",
        [ENC],
    ),
]

ENC_LZMA = [
    (
        "the length prices are refreshed after every length",
        "                if *c == 0 {",
        "                if true {",
        [ENC],
    ),
    (
        "the distance price count does not grow",
        "        self.match_price_count = self.match_price_count.wrapping_add(1);",
        "",
        [ENC],
    ),
    (
        "the align price count does not grow",
        "                self.align_price_count = self.align_price_count.wrapping_add(1);",
        "",
        [ENC],
    ),
    (
        "the literal coder ignores the position bits",
        "((pos & self.lp_mask) << self.lc)",
        "(0u32 << self.lc)",
        [ENC],
    ),
    (
        "plain LZMA ends without its marker",
        "                self.encode_eopm(position);",
        "",
        [ENC, ENC_MADE],
    ),
    (
        "an LZMA2 chunk may fill its 64 KiB",
        ">= u64::from(LZMA2_CHUNK_MAX - LOOP_INPUT_MAX);",
        ">= u64::from(LZMA2_CHUNK_MAX);",
        [ENC],
    ),
]

ENC_OPTIMUM = [
    (
        "a short repeat loses a tie to a literal",
        "            if short_rep_price <= next.price {",
        "            if short_rep_price < next.price {",
        [ENC],
    ),
    (
        "the fast chooser's distance test moves its edge",
        "    (big_dist >> 7) > small_dist",
        "    (big_dist >> 7) >= small_dist",
        [ENC],
    ),
    (
        "a two-byte match far back is taken",
        "            if len_main == 2 && back_main >= 0x80 {",
        "            if false && len_main == 2 && back_main >= 0x80 {",
        [ENC],
    ),
    (
        "distance prices are refreshed after a few matches",
        "            if self.match_price_count >= 1 << 7 {",
        "            if self.match_price_count >= 1 << 3 {",
        [ENC],
    ),
    (
        "align prices are refreshed after every one",
        "            if self.align_price_count >= ALIGN_SIZE as u32 {",
        "            if self.align_price_count >= 1 {",
        [ENC],
    ),
    (
        "a literal then a repeat is not priced",
        "        if !next_is_literal && match_byte != current_byte {",
        "        if false {",
        [ENC],
    ),
    (
        "backward forgets a two-step path",
        "                if here.prev_2 {\n                    if let Some(o) = self.opt_mut(pos_mem.wrapping_sub(1)) {",
        "                if false {\n                    if let Some(o) = self.opt_mut(pos_mem.wrapping_sub(1)) {",
        [ENC],
    ),
]

ENC_LZMA2 = [
    (
        "a chunk that did not compress is kept compressed",
        "        if chunk.len() >= uncompressed {",
        "        if false {",
        [ENC],
    ),
    (
        "the state is not reset after a stored chunk",
        "            need_state_reset = true;\n",
        "",
        [ENC],
    ),
    (
        "a chunk may take 2 MiB of input and a symbol more",
        "UNCOMPRESSED_MAX - MATCH_LEN_MAX as usize",
        "UNCOMPRESSED_MAX",
        [ENC],
    ),
]

ENC_CONTAINER = [
    (
        "a block header is not padded",
        "    let size = (2 + body.len() + 4 + 3) & !3;",
        "    let size = 2 + body.len() + 4;",
        [ENC, ENC_MADE],
    ),
    (
        "the index is not padded",
        "    while out.len().saturating_sub(index_start) % 4 != 0 {",
        "    while false {",
        [ENC, ENC_MADE],
    ),
    (
        "an .lzma dictionary size is not rounded up",
        "    let mut d = opt.dict_size.saturating_sub(1);\n    d |= d >> 2;\n    d |= d >> 3;\n    d |= d >> 4;\n    d |= d >> 8;\n    d |= d >> 16;\n    if d != u32::MAX {\n        d = d.wrapping_add(1);\n    }",
        "    let d = opt.dict_size;",
        [ENC],
    ),
    (
        "a branch converter's start offset is not recorded",
        "                if start == 0 {",
        "                if true {",
        [ENC_MADE],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [
        (SRC / "lzma.rs", LZMA),
        (SRC / "lzma2.rs", LZMA2),
        (SRC / "stream.rs", STREAM),
        (SRC / "filters.rs", FILTERS),
        (SRC / "vli.rs", VLI),
        (SRC / "alone.rs", ALONE),
        (SRC / "encode" / "rc.rs", ENC_RC),
        (SRC / "encode" / "price.rs", ENC_PRICE),
        (SRC / "encode" / "mf.rs", ENC_MF),
        (SRC / "encode" / "lzma.rs", ENC_LZMA),
        (SRC / "encode" / "optimum.rs", ENC_OPTIMUM),
        (SRC / "encode" / "lzma2.rs", ENC_LZMA2),
        (SRC / "encode" / "container.rs", ENC_CONTAINER),
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
        results.append(sweep(src, rows, "xz", timeout=900, only=mine or None))
    raise SystemExit(max(results))
