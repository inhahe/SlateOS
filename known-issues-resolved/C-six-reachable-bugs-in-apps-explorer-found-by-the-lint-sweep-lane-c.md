## Six reachable bugs in `apps/explorer`, found by the lint sweep (lane C)

**Status: FIXED 2026-08-16** (lane C). None was caught by any existing test;
all six now have one, and each test was verified by reverting the fix and
watching it fail. Five come from `thumbs.rs`'s pixel buffer, one from the
column sort order.

**1. A sort that put every non-ASCII filename ahead of every ASCII one.**
`ColumnValue::sort_key` packed a text value into an `i128` as its first sixteen
lowercased bytes, shifting the leading byte left by 120 — into bit 127, the
sign bit. Any name whose first byte is `>= 0x80` — which is *every* name that
does not begin with an ASCII character — therefore produced a **negative** key
and sorted ahead of every ASCII name. In a folder mixing `alpha`, `zulu` and
`éclair`, `éclair` came first, ordered by nothing the user could see.

**2. The same sort violated `Ord`'s contract, so its behaviour was undefined.**
That key stopped after sixteen bytes, so two names sharing a sixteen-byte
prefix compared `Equal` while the derived `PartialEq` called them different.
`Ord` requires `cmp` to return `Equal` exactly when `==` does; a comparator
that breaks it makes `sort_by` free to produce *any* permutation, not merely a
wrong one. Both are fixed by comparing the strings directly —
`textfind::compare(a, b, Insensitive).then_with(|| a.cmp(b))` — which is also
cheaper, since it stops at the first differing character instead of walking
sixteen bytes and allocating a lowercased copy of the whole name. A NaN
`Percentage` had the same contract problem for the same reason (derived
`PartialEq` on `f32` makes NaN unequal to itself); `f32::total_cmp` gives it a
defined place instead.

Note what is *not* fixed: ordering is now by Unicode scalar value after case
folding, so `éclair` sorts after `zulu` rather than between `alpha` and `zulu`.
See `TD-EXPLORER-SORT-IS-CODEPOINT-NOT-COLLATION` below.

**3. A configured thumbnail size below 6 made a text thumbnail write past the
buffer.** `generate_text_thumbnail` computed `size - margin * 2` on `u32` with
`margin = 3`. At `size = 5` that wraps to ~4 billion, which made both of the
loop's bounds checks vacuously true, and every subsequent row was written at an
offset computed from a width the buffer does not have. `ThumbConfig::size` is a
plain public field, so reaching it needs no malformed input at all — just a
caller that asks for a tiny thumbnail. Now `saturating_sub` throughout, and the
writes go through `Canvas::fill_rect`, which clips.

**4. BMP header arithmetic ran unchecked on attacker-chosen numbers.**
`try_bmp_thumbnail` multiplied and added width, height, bits-per-pixel and the
pixel-data offset — all read straight out of the file — to compute row strides
and source indices. A crafted `.bmp` in a browsed directory overflows those and
lands the index anywhere. Every one is now `checked_mul`/`checked_add` and the
pixel read is a `data.get(range)?`, so a malformed file yields no thumbnail
rather than a panic or a wild read.

**5. `draw_block_text` took a size argument that had to agree with the
buffer's, and nothing checked.** It received `&mut Vec<u8>` and a `size`, and
computed every offset from `size`. Two arguments that must agree is a
precondition no signature states; passing a buffer and the wrong size wrote out
of range. It now takes `&mut Canvas`, which carries its own dimensions, so the
pair cannot disagree.

**6. A circle-distance test overflowed `i32` above ~92 000 pixels.**
`generate_default_thumbnail` compared `dx*dx + dy*dy` in `i32`. Promoted to
`i64`. Not reachable at any size a display would ask for, but it was a wrong
proof sitting next to five that were reachable, which is the argument for
fixing the class rather than the instances.
