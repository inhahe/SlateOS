## 1059. `file -z` inflates with zlib's semantics, in a decoder of its own

**Date:** 2026-10-02
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `file -z` describes what is inside a gzip or zlib stream, and
for a damaged one it prints the error zlib gives, word for word --
`ERROR:[zlib: invalid distance too far back]`. The system's decoder, the
`deflate` crate, cannot give those answers: it folds a dozen of zlib's
messages into one error, treats a truncated stream as an error where zlib
hands back what it decoded, and stops at a full buffer where zlib reads on
to the next block's header. So `libmagic` has its own small decoder
(`src/zlib.rs`) written to answer as zlib's `inflate` answers. That is a
second DEFLATE decoder in the tree, which the `deflate` crate's documentation
argues against.

| Option | For | Against |
|---|---|---|
| **A. A decoder with zlib's semantics in libmagic** (chosen) | `file -z`'s output is upstream's, damaged and truncated streams included: `scripts/file-diff.sh` reaches every message zlib gives on 2,000 crafted streams with no difference. Small (one file), safe Rust, no `unsafe`. | A second parser of untrusted compressed data to keep correct; the `deflate` crate's doc warns against exactly this. |
| B. Use the `deflate` crate | One decoder. | Different words for errors, and different *answers* for truncated input -- `file -z` on a cut-off download would print an error where upstream describes the contents. |
| C. Give the `deflate` crate zlib's error detail and partial-output behaviour | One decoder, exact. | Changes a crate the kernel and three other lanes use, for one caller's fidelity, and its table-driven decoder's structure differs from zlib's enough that "the point zlib raises it" would be a redesign, not a variant. |

**Mitigations.** The decoder reads bit by bit with every access checked
(`get`), and it is exercised by the harness's crafted streams in a debug
build with overflow checks. It decodes only for identification -- never
into anything kept.

**Revisit when** the `deflate` crate grows a zlib-compatible mode for other
reasons; then this should become a call to it.
