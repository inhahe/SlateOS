## TD-FONT-HAS-NO-FALLBACK-MARK-POSITIONING

**Status:** FIXED 2026-08-14 -- the resolution note in the entry below gives the change; on `main` since. Stamped 2026-09-28 by lane F, `gui/font`'s owner since the six-lane split, so that the heading no longer reads as unfixed. Moved to `known-issues-resolved/` on 2026-10-05 (lane F).

**What.** A combining mark is placed on its base by the `GPOS` `mark` feature
(lookup types 4 and 6), which `mark.rs` implements. A face that has no such
lookups gets nothing: the mark keeps its own advance and is drawn *after* the
base at the pen, side by side with it, rather than on top of it.

**Symptom, measured.** This is the single largest positional divergence in the
sweep — 489 of the 559 `misplaced` face×string pairs. `c` + cedilla + acute
disagrees on 222 faces and fully-vowelled Arabic (`بِسْمِ`) on 233. The first
example the sweep prints is exact: on `AGENCYB.TTF` we draw the cedilla at
x=817 with a full advance, HarfBuzz draws it at x=-104, y=1138 with no
advance. The user-visible effect is `ç` rendering as `c` followed by a
free-standing cedilla, and vowelled Arabic rendering as letters interleaved
with their own vowel signs at full width — which is how the text looked before
the mark table was implemented, for every face that lacks one.

**Why it is filed rather than fixed.** It needs glyph *extents*, which nothing
in the shaping path currently asks for. HarfBuzz's fallback
(`hb-ot-shape-fallback.cc`) centres the mark horizontally over the base's ink
box and stacks it above or below according to the mark's canonical combining
class — 230 above, 220 below, and a running "how high have we stacked already"
for a second mark on the same base. All of that is measured from the outline
bounding boxes of two glyphs. We can compute those (`raster.rs` walks the
outline already, and `glyf` stores a bbox per glyph outright) but shaping has
never needed to, so the plumbing is new.

**Proper fix.** A `bbox(gid)` on the face, reading `glyf`'s per-glyph bounding
box directly where it exists and walking the `CFF` charstring where it does
not; then a fallback pass in `attach_marks` that runs for a mark no `GPOS`
lookup positioned: zero the advance, centre on the base's box, and offset
vertically by the base's top or bottom plus the height already consumed by
marks of the same class. The combining class is in `norm_tables.rs`.

**Where.** `gui/font/src/scaled.rs` — `attach_marks`, which today does nothing
for a mark that no lookup matched; `gui/font/src/mark.rs`, which knows which
marks those are.

**Resolved 2026-08-14.** `gui/font/src/fallback.rs` implements it, as a
transcription of `hb-ot-shape-fallback.cc` — see `design-decisions.md` §416
for the three decisions it took. The sketch above was right about the
mechanism and wrong about the trigger in two ways, both of which cost a round
of the sweep to find:

* The trigger is **no `GPOS` table at all**, not "no lookup matched this
  mark". A face that has a `GPOS` and chose not to position this mark has made
  a statement, and Candara on this host is exactly that face; HarfBuzz leaves
  its marks alone and so must we. `Face::has_positioning` carries the
  distinction.
* It is gated **per script**. Running it on Devanagari zeroed the virama's
  advance and centred it, which the sweep caught as 33 *new* misplaced runs.
  `fallback::positions_marks` excludes the 101 tags whose HarfBuzz shaper sets
  `fallback_position = false`.

Measured: the `misplaced` bucket over 556 faces × 19 strings fell from **559
to 98**, and both of the failures quoted above — `c` + cedilla + acute, and
vowelled Arabic — are gone. Two smaller divergences the implementation
knowingly leaves behind are filed below as
`TD-FONT-DOES-NOT-RE-SORT-HEBREW-AND-ARABIC-MARKS` and
`TD-FONT-GATES-THE-MARK-FALLBACK-ON-THE-CHARACTERS-SCRIPT-NOT-THE-FONTS`.
