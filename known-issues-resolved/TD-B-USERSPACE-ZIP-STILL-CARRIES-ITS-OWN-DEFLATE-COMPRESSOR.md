## TD-B-USERSPACE-ZIP-STILL-CARRIES-ITS-OWN-DEFLATE-COMPRESSOR — **FIXED 2026-09-02**

**Status:** **fixed 2026-09-02 (lane B).** `deflate::deflate_level` landed on
lane A (`requests/a-b-deflate-level-has-landed-and-your-local-compressor-was-the-better-one.md`),
which is exactly the unblocker named under "What unblocks it" below, and
`add_file` now calls it. Every item in the "Where" list is deleted.

Two things did **not** go as this entry predicted, and both are worth keeping:

1. **It was not "a pure deletion with no behaviour change at all."** Writing the
   level knob uncovered that the shared crate's LZ77 match-finder had never
   found a match on any input — it inserted into the hash chain before reading
   it, so its own first `candidate < pos` guard was false on every call, and
   `deflate` had silently degraded to Huffman-only coding. Adopting the shared
   crate at any point before 2026-08-30 would have been a real regression. See
   `A-DEFLATE-LZ77-NEVER-FOUND-A-MATCH`. The transferable lesson is in the last
   paragraph below, which needs amending: *the shared implementation being
   shared is not evidence that it is right.*
2. **The claim that "no attacker-supplied bytes reach it" was correct but was
   not the reason this was safe to leave.** What actually made the duplication
   cheap to carry is that the local compressor was the *correct* one of the
   two. That is luck, not a property, and it is not a defence available next
   time.

The replacement tests assert properties rather than sizes, per lane A's
recommendation — including `test_the_level_flag_changes_the_output`, which is
the only test in the file that fails if `add_file` ignores its `level`
argument. Verified by mutation: hardcoding the level makes exactly that one
test fail and leaves the other 40 green.

**Filed:** 2026-08-30 (lane B)
**Where:** `userspace/zip/src/main.rs` — `Token`, `hash3`, `lz77_compress`,
`deflate_compress`, `deflate_compress_stored`, `BitWriter`, `length_code`,
`distance_code`, `fixed_litlen_encode`, `reverse_bits`, and the
`LENGTH_TABLE` / `DISTANCE_TABLE` pair (~380 lines).

**What it is.** Lane A asked for `userspace/zip` to stop carrying a private
copy of RFC 1951 and use the shared `deflate` crate
(`requests/a-b-userspace-zip-carries-a-third-deflate-and-a-second-zip-parser.md`).
The *decompressor* half is done and deleted — see `b5ede6224`, which also
fixed a decompression-bomb hole in the process. The *compressor* half is
still here, and this entry exists so that half-finished state is a recorded
decision rather than something that looks like an oversight to whoever reads
the file next.

**Why it is not just done.** `zip` accepts `-0` through `-9` and maps the
level onto the LZ77 hash-chain depth — 4 at `-1`, 128 at the default `-6`,
1024 at `-9`. `deflate::deflate(data)` takes no level and fixes
`MAX_CHAIN = 16`, which is our `-3`. So the swap would collapse nine
documented flags into one setting *and* silently demote the no-flags default
to a weaker one, producing larger archives with no message and no way for the
user to get the previous behaviour back. Removing duplication is worth a lot;
it is not worth a silent quality regression on the path that runs when nobody
passes a flag.

**What unblocks it.** `deflate::deflate_level(data, level)` — one function,
one constant becoming a parameter. Requested in
`requests/b-a-deflate-cannot-express-a-compression-level.md`. If lane A adopts
the zlib mapping above, this becomes a pure deletion with no behaviour change
at all. If lane A declines, the right move is to close that request, note the
local compressor as a deliberate permanent exception in the original request,
and delete *this* entry — because at that point it is a decision, not debt.

**What is NOT at risk while it sits.** This is duplication, not a bug: the
local compressor is correct and round-trip tested, and it is on the *write*
path, so no attacker-supplied bytes reach it. The bomb hole that made the
decompressor urgent has no analogue on this side. The cost is maintenance —
a bug fixed in `deflate`'s encoder does not reach `zip` — and it does not
worsen with time.
