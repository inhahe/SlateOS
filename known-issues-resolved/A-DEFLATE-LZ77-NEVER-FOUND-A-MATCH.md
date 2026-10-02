## A-DEFLATE-LZ77-NEVER-FOUND-A-MATCH (lane A, 2026-08-30) — **FIXED 2026-08-30**

**In short:** the compressor's match-finder — the part that notices "these
fifty bytes are the same as fifty bytes I saw earlier, so say *copy them*
instead of spelling them out again" — never found a single match, on any
input, ever. Every `.zip`, `.tar.gz` and compressed filesystem block this
system has produced was compressed by frequency coding alone, at roughly a
third of the ratio it should have got. Repairing it makes archives about
three times smaller and changes nothing about how they are read: the streams
we produced were valid DEFLATE, just needlessly large, and both old and new
streams decode with the same `inflate`.

### The defect

`deflate/src/lib.rs`. `insert_hash` links position `pos` into the hash chain
and returns the chain's *previous* head — the most recent earlier position
with the same three-byte hash — which is precisely where a search for a match
must begin. Its doc comment said so: "return the previous head of that chain
(for match searching)".

The caller threw the value away:

```rust
insert_hash(data, pos, &mut head, &mut prev);
let (cur_len, cur_dist) = find_best_match(data, pos, &prev, max_chain);
```

and `find_best_match` re-derived its starting point by reading `head[h]`. But
the insert had *just* set `head[h] = pos`. So the search began at `pos`
itself, and the loop's first guard —

```rust
while candidate < pos && chain_count < max_chain { … }
```

— was false on the first iteration. Every call returned `(0, 0)`. Every
position of every input emitted a literal. `lz77_tokenize` was an expensive
identity function over the byte stream, and `deflate` silently degraded to
Huffman-only coding.

### Why it survived

Nothing tested it. The round-trip tests all passed, because a stream of
literals is a completely valid DEFLATE stream — the encoder was producing
*correct* output, merely three times too much of it. The ratio tests that
existed asserted "smaller than the input", which Huffman coding alone
satisfies comfortably on text.

It was found while implementing `deflate_level` for lane B
(`requests/b-a-deflate-cannot-express-a-compression-level.md`): a test that
level 1 and level 9 must produce *different* output failed with the two
byte-identical. The first two failures were misread as bad test corpora —
first "the phrases repeat so early that the first candidate already hits
`MAX_MATCH`", then "uniform random data has no redundancy to find" — and both
diagnoses were individually plausible. The third identical result falsified
the premise, and a temporary instrumented build settled it in one line:

```
PROBE: 2000 tokens, 0 matches, 2000 literals
```

on maximally repetitive input. The lesson worth keeping: a compression test
that only asserts "output got smaller" cannot tell an LZ77 stage from a
`memcpy`. The level test caught it because it asserted the *effort knob has
an effect*, which is a property no degenerate encoder can fake.

### The fix

The chain head is now a parameter, so the invariant is in the signature
rather than in prose:

```rust
let chain_start = insert_hash(data, pos, &mut head, &mut prev) as usize;
let (cur_len, cur_dist) = find_best_match(data, pos, chain_start, &prev, max_chain);
```

`find_best_match`'s doc records why it cannot go back to reading `head[h]`.

### Measured effect

Same corpus, same code, before and after (bytes):

| Input | Before | After (default) | After (level 9) |
|---|---:|---:|---:|
| LZ77-friendly corpus, 49170 raw | 26558 | 9292 | 6045 |
| 16-symbol random, 32768 raw | 18998 | 16647 | 16647 |
| 4-symbol random (DNA-shaped), 32768 raw | 9218 | 9217 | 9217 |
| 8-bit random, 32768 raw | 32816 | 32773 | 32773 |

### Two further defects the repair exposed

Both were only reachable *because* matches started being found, and both are
fixed in the same change.

1. **`deflate` could produce output larger than its input.** 32 KiB of random
   bytes — i.e. any already-compressed file, a JPEG inside a `.zip` being the
   everyday case — came out at 32816 bytes. The stored-block path was
   reachable only for inputs of 64 bytes or fewer, so every longer input was
   forced through Huffman coding that had nothing to exploit.
   `deflate_with_chain` now prices a stored candidate (arithmetic, not an
   encode: `stored_size`) and takes it when it wins, bounding output at the
   input plus five bytes per 65535-byte block, which is the format's own
   floor. Pinned by `incompressible_input_is_never_inflated`.

2. **Matches can cost more than the literals they replace.** On a four-symbol
   alphabet a literal is two bits while the shortest match is a length symbol
   plus a distance symbol plus up to thirteen extra bits, so every match is a
   net loss and a greedy parser takes them anyway. That is what made the
   4-symbol row above 10298 bytes at the first attempt — worse than the
   accidentally-Huffman-only encoder it replaced. zlib's `TOO_FAR` rule
   (reject a bare 3-byte match beyond 4096 bytes) is implemented and helps,
   but does not cover it, because with a four-symbol alphabet the damaging
   matches are at *short* distances. `deflate_with_chain` now also prices a
   literals-only candidate — the same bytes with the LZ77 stage switched off —
   which makes the encoder provably never worse than order-0 Huffman coding.
   zlib carries this weakness; we no longer do. Pinned by
   `lz77_never_loses_to_plain_huffman`.

   The candidate costs nothing on data that actually compresses: it is
   *estimated* first by `literal_only_size_estimate`, which runs the real
   `build_code_lengths` over the byte histogram and so returns the exact
   payload size plus a header bound, and is only encoded when the estimate
   beats the best LZ77 result. It is also skipped outright when the token
   stream contains no match, since then the two streams are identical.

### And one latent one, fixed while it was still unreachable

`deflate_stored` wrote `data.len() as u16`. Above 65535 bytes that declares a
truncated LEN and then emits the full payload behind it — a silently corrupt
stream. It was unreachable while the only caller was the 64-byte fast path;
adding the stored candidate made it reachable at any size. It now emits as
many blocks as the payload needs, with `bfinal` on the last only, and
`stored_blocks_split_above_the_sixteen_bit_length` round-trips 0, 1, 65535,
65536, 131070 and 200000 bytes through it and checks `stored_size` predicts
each length exactly.

### Where

| | |
|---|---|
| The defect | `deflate/src/lib.rs`, `lz77_tokenize` / `find_best_match` |
| The two ratio guards | `deflate/src/lib.rs`, `deflate_with_chain` |
| The stored-block split | `deflate/src/lib.rs`, `deflate_stored` |
| Tests | `incompressible_input_is_never_inflated`, `lz77_never_loses_to_plain_huffman`, `stored_blocks_split_above_the_sixteen_bit_length`, `levels_are_distinguishable_and_monotonic_in_ratio` |
| Downstream | `ziparchive`, `kernel/src/fs/compress.rs`, `gzip`/`zlib_deflate` — all get the ratio for free, no call-site change |
