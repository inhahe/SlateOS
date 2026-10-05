## §440 — Device corrections are folded into the value at read time, against a size the face itself does not have

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** OpenType lets a font say "this accent sits a bit lower when
you're drawing at 11 pixels tall" — a *device table*, a correction stated in
whole pixels that applies only inside a range of sizes it names. This crate
used to skip past those. Reading them raises three questions with real answers
on both sides: where the correction gets added, what a sizeless object should
answer, and what to do about a second, unrelated feature that occupies the same
eight bytes. What follows is what was chosen and why. Nothing here is
user-visible except at small sizes on the five host faces that ship the
tables — where it is the difference between an Arabic fatha sitting on its
letter and sitting inside it.

**The correction is added where the value is read, not carried alongside it.**
`Value::read` and `mark::anchor` return a coordinate with the delta already in
it; nothing downstream knows a device table was involved.

*The alternative* was to return the correction separately — a `Value` plus a
`Device` — and apply it at draw time. That would let one parse serve two sizes,
which matters for a cache keyed on the face rather than on the scaled font.
It was rejected because the seam it creates is exactly the one this crate has
already been bitten by: two readers of one table, which is what
`kern.rs`'s own doc warns about ("a pair that measures at one correction and
draws at another puts the caret in the wrong place"). Positioning is not cached
across sizes here — `Run` is produced by a `ScaledFont`, which *is* a size — so
the flexibility bought nothing and the risk was live.

**A `Face` answers `Ppem::NONE`, and that is an answer rather than a
fallback.** `Face` is the parsed bytes; it has no size, so it has no device
corrections, and every device table reads as zero through it.
`Face::kern_across` keeps its old signature and its old meaning — design units
— and a new `kern_across_at` carries a size for `ScaledFont::kern_across` to
call.

*The alternative* was to make `Face`'s positioning API take a size, forcing
every caller to state one. Rejected: it would make callers that legitimately
want design-unit metrics (layout at nominal size, measurement for a cache key)
invent a pixel size to get them, and an invented size is a silently wrong
correction rather than an absent one. Two entry points, one of which is
explicitly sizeless, says what is true.

**`deltaFormat` is checked before `startSize`/`endSize`, and this is measured,
not stylistic.** The same eight bytes are also used for a `VariationIndex`
(`deltaFormat == 0x8000`), whose first four bytes are indices into a variable
font's `ItemVariationStore` rather than a size range. A reader that
range-checks first reads those indices as a start and end size, and applies a
delta out of the wrong array whenever they happen to bracket the size being
drawn.

*How often that happens here:* of this host's 9,215 `VariationIndex` records,
3,146 would bracket 9 ppem, 3,086 would bracket 12, and 2,832 would bracket 16
(`gui/font/tools/device_survey.py`). The wrong answer is not a slightly wrong
correction; it is an arbitrary one. Variation indices outnumber real device
tables 60 to 1 on this machine, so this is the *common* path through the
reader, not the edge case. That arm no longer declines: §451 gave it the
instance as well as the size, and `TD-FONT-DOES-NOT-READ-VARIATION-STORES` is
closed. The ordering rule above is what makes the two cases distinguishable in
the first place, so it stays exactly as written.

**Pixels are converted to font units by truncation, not rounding.** A delta is
in pixels and the value it corrects is in font units, so it is multiplied by
`upem / ppem`. HarfBuzz's `Device::get_delta` does `pixels * (int64_t)scale /
ppem`, which truncates toward zero.

*The alternative* — rounding — is arguably more accurate and was rejected
anyway, because the oracle this crate is measured against truncates, and a
reader that is more accurate than its oracle produces a sweep full of
one-unit differences that have to be explained every time someone reads it.
Matching the reference implementation is worth more than a half-unit.

**Where it lives.** `gui/font/src/device.rs` — `Ppem`, `pixel_delta`;
`gui/font/src/gpos.rs` — `Value::read` and `Run::ppem`;
`gui/font/src/mark.rs` — `anchor`; `gui/font/src/sfnt.rs` — `Face::ppem` and
`kern_across_at`; `gui/font/tools/device_survey.py` — the numbers above;
`gui/font/tools/harfbuzz_sweep.py` — `--ppem`, which is the only way to
measure any of it.
