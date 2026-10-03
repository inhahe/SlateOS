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
