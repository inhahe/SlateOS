## `A-ARCHIVE-LIST-DISCARDS-THE-ZIP-TIMESTAMP-IT-JUST-LEARNED-TO-WRITE` (lane A, 2026-08-27)

**Status:** **FIXED 2026-08-27**, same day it was filed, for steps 1–2.
Steps 3 (offer the decoder to lane C) and 4 (`list_7z`) remain — see below.

**How it was fixed.** `tzrules::unix_from_dos_datetime(u32) -> Option<i64>`
landed as the inverse of `dos_datetime_from_unix`, and `list_zip` now decodes
through it instead of hardcoding `0`. Six tests: a hand-packed known pair
decoded without reference to the encoder, the `0` sentinel refusing to become
1980, a table of seven field patterns the format permits but the calendar
cannot name (month 15, month 0, day 0, February 30, Feb 29 of a common year,
hour 25, minute 61) each refused rather than normalised, the leap-day near-miss
that proves the check consults `days_in_month` rather than a constant, an
odd-second round trip asserting the loss is at most one second and always
downward, both range edges, and a full-range walk round-tripping all ~46,750
representable days. 59 tests pass in `tzrules` (was 53).

`test_mtime_reaches_every_writer` now checks ZIP twice: through `zip::parse`
for the exact packed pair and through `list_format` for the decoded seconds.
Neither subsumes the other — the first passes even if `list_zip` discards the
value, and the second passes if encoder and decoder are wrong in mirror-image
ways.

**Still open from the original plan:**

- **Step 3 — offer it to lane C.** `apps/archivemanager` has an independent
  decoder. §621's condition for hoisting is now met, so this needs a
  `requests/a-c-…` note. Not urgent: theirs works, and it range-checks as part
  of deciding whether to *render* a date, which is a rendering decision this
  function deliberately does not make. The offer should say so rather than
  imply their copy is redundant.
- **Step 4 — `list_7z` still reports `0`.** Honest, not a bug:
  `sevenz::un7z` surfaces no time at all, so there is nothing to discard. It
  becomes a real gap only if the 7z parser is extended to read the header's
  time attributes.
- **`ArchiveEntry::mtime` still cannot say "unknown".** `list_zip` collapses
  both `None` cases — the sentinel and a corrupt pair — to `0`, which is the
  same conflation the writer side was fixed to avoid, now on the reading side.
  Fixing it properly means widening the field to `Option<u64>` and touching
  every `list_*`; worth doing when something actually renders these.

**In short:** `archive create out.zip f.txt` now records when `f.txt` was last
changed, but `archive list out.zip` reports that time as `1970-01-01` — for the
same archive it just wrote. The reading half throws the value away. It is a
one-line hole in a function, not a design gap: `list_tar`, `list_cpio`,
`list_ar` and `list_rar` all pass their `mtime` through; only `list_zip` and
`list_7z` hardcode `0`.

**Where it lives.** `kernel/src/fs/archive.rs`, `list_zip` (~344) and `list_7z`
(~453), both building an `ArchiveEntry` with a literal `mtime: 0`.

**Why it wasn't fixed alongside the writer.** ZIP does not store Unix seconds;
it stores a packed DOS date/time pair, so `list_zip` cannot pass its value
through the way the other four do — it needs a *decoder*, which does not exist
in the kernel tree. Writing one is a change to `tzrules` (a different crate,
with its own tests) and it needs a `-> Option<i64>` shape to reject the pairs
the format allows but the calendar does not (month 0, day 0, February 30). That
is a self-contained second change, and bundling it would have held a tested
writer-side fix behind it.

Note that this makes ZIP's `mtime` *honestly* `0` for a genuinely unstamped
member and *dishonestly* `0` for a stamped one, with no way for a reader to
tell them apart — the same conflation the writer side was fixed to avoid.

**What the proper fix looks like.**

1. Add `pub fn unix_from_dos_datetime(packed: u32) -> Option<i64>` to `tzrules`,
   inverse of `dos_datetime_from_unix` (`37c04848e`). `None` for `0` and for
   any pair naming a date that does not exist — the format's fields are wide
   enough to hold month 15 and day 31 of February, and a decoder that trusts
   them produces a nonsense instant rather than a refusal. Round-trip test it
   against the encoder over the full representable range; the encoder's own
   `every_representable_day_packs_into_a_valid_in_range_field` walk (~46,750
   days) is the natural fixture to reuse.
2. `list_zip` maps `None` to `0` and `Some(s)` to `s as u64`. The lossiness of
   that mapping is the conflation noted above and is only acceptable because
   `ArchiveEntry::mtime` has no richer type; widening it to `Option<u64>` would
   be better and touches every `list_*`.
3. Lane C has an independent decoder in `apps/archivemanager` that also
   range-checks as part of deciding whether to render a date. Once `tzrules`
   has one, §621's condition for hoisting is met — file a request offering it
   rather than leaving two implementations of the same table.
4. `list_7z` is a separate matter: `sevenz::un7z` does not surface a time at
   all, so its `0` is honest until the parser is extended.

**How it was found.** Writing `test_mtime_reaches_every_writer` for the entry
above. The tar/cpio/ar arms could assert through `list_format`; the ZIP arm had
to reach past it to `zip::parse`, which is what exposed that `list` was the
lossy layer rather than the writer.
