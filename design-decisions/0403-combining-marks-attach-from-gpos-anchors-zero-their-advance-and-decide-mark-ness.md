## §403 — Combining marks: attach from GPOS anchors, zero their advance, and decide mark-ness from GDEF ∪ coverage

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** Kerning (§401) and ligatures (§402) are refinements: get them
wrong and text is a fraction of a pixel off, or an `fi` has a visible
collision. Mark attachment is not in that class. A combining mark is drawn
at the pen, and the pen for the mark is the *left edge* of the following
cell — so with no positioning at all, `e` + U+0301 draws the acute in the
gap **before** the `e`, and a second mark draws on top of the first. Every
accented language on the machine renders visibly broken. `hmtx` cannot help:
it says a mark takes no room, never where it goes.

**Decision, in four parts.**

*1. Read MarkBasePos (GPOS type 4, feature `mark`) and MarkMarkPos (type 6,
`mkmk`); do not read MarkLigPos (type 5).* The first two share a
byte-for-byte identical header — coverage for the attachee, coverage for the
mark, a class count, two arrays — so one reader handles both, which is not
a coincidence being exploited but how the spec defines them. MarkLigPos is
left out because it attaches a mark to one *component* of a ligature, which
requires remembering which component each mark belonged to before
substitution collapsed them. A `ShapedRun` glyph knows its cluster, not its
components. A mark on a ligature therefore falls back to MarkBasePos, which
most faces also provide; where they do not it lands unpositioned, which is
wrong in the way the font asked for rather than wrong in a way nobody could
trace back.

*2. Mark-ness is `GDEF` class 3 **or** membership of a mark coverage — the
union, not `GDEF` alone.* This was measured, not chosen: DejaVu Sans Mono
classes `acutecomb` as `GlyphClassDef` 1 (base) while its own `mark` feature
carries an anchor for that very glyph, so a `GDEF`-exclusive reader leaves
every accent in that family unattached. Coverage alone is equally wrong: a
mark the face has no anchor for would read as a base, and the *next* mark
would then stack onto it. The fully correct answer is Unicode general
category `Mn`/`Mc`/`Me`, which is a property of the character and true
whatever the font claims; that needs a category table this crate does not
have. The union covers every face that has anchors to attach with, which is
every face where the answer changes anything.

*3. A mark's advance is zero, whatever `hmtx` says.* Also measured: Segoe UI
gives U+0301 an advance more than half an `e` wide, because the same outline
doubles as the spacing acute U+00B4. Honouring it made `e` + U+0301 measure
25.3 px against the bare `e`'s 16.7 — a gap after every accented letter, and
a caret that lands in it. HarfBuzz zeroes mark advances after positioning
for exactly this reason. Because mark-ness now decides a *width*, not just a
placement, `MarkPositioning` is kept for a face that has `GDEF` classes but
no anchors — it still knows which glyphs are marks, and that alone is worth
having. Hence `Face::has_marks()` rather than `has_mark_positioning()`.

*4. Marks are never kerned.* Real faces set `IgnoreMarks` on their kerning
lookups so that `A` and `V` still kern with an accent between them. This
engine walks a run strictly in order and cannot skip, so it declines
instead: a pair separated by a mark goes unkerned, losing a fraction of a
pixel, where kerning *against* the mark would shove the accent off the
letter it belongs to. (Ligatures likewise may not span a tab — the tab's
width is a layout decision, not a glyph width, and joining across it would
swallow the gap it exists to make.)

**Cost.** `ScaledFont::shape` is now four passes rather than three, each
justified by a data dependency: substitution cannot be decided while
characters are still arriving; kerning applies to the glyphs that *survive*
substitution; and a mark's displacement is measured from its base glyph's
origin, whose distance from the pen is the sum of the advances in between —
which pass three is still adjusting. The fourth pass allocates one `f32` per
glyph to hold the running pen and runs only on faces that know about marks.

**Alternatives rejected.** Decomposing precomposed characters and
recomposing via `ccmp` (needs Unicode normalization data we do not have, and
buys nothing for text that already arrives precomposed); synthesising an
accent placement from the base glyph's bounding box when the font offers no
anchor (invents a number the font never authorised, and it would be wrong in
a way no one could attribute).

**Measured.** Of 556 faces on the development host, 349 can tell a mark from
a letter and 175 place a combining acute on an `e`. The four oracle faces
(Segoe UI, DejaVu Sans, Calibri, Times New Roman) all put the accent's ink
inside the `e`'s advance and above the baseline. Note that the *sign* of the
displacement is not an oracle here, unlike kerning's: Cascadia Code draws
its combining acute already at accent height inside its cell, so its anchors
coincide and the displacement is legitimately `(0, 0)`.

**Where it lives.** `gui/font/src/mark.rs` (`MarkPositioning`, anchor and
`MarkArray`/`BaseArray` reading), `gui/font/src/otl.rs` (the walk shared
with GSUB and kerning), `gui/font/src/sfnt.rs` (`Face::is_mark`,
`mark_on_base`, `mark_on_mark`, `has_marks`), `gui/font/src/shape.rs`
(`ShapedGlyph::offset`), `gui/font/src/scaled.rs` (`shape`'s fourth pass,
`attach_marks`, `glyph_mask`, `draw_text`), `gui/font/src/system.rs`,
`gui/compositor/src/main.rs` (`RenderEngine::draw_text`).
