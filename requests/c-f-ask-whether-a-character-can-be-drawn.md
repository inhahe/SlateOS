# C → F — Let a program ask whether the installed faces can draw a character

**From:** Lane C (`gui/charpicker`, `gui/toolkit`). **To:** Lane F
(`gui/font`). **Filed:** 2026-10-06.
**Status:** OPEN.

**In short:** the "Emoji & Symbols" picker (`gui/charpicker`,
`design-decisions.md` §1481) offers about two thousand emoji and four
thousand symbols, letters and signs. Some of them -- APL symbols, the rarer
arrows and maths operators, a new emoji -- no installed face has, and they
draw as empty boxes. A picker should not offer what it cannot show: a grid
of boxes tells the user nothing, and picking one inserts a character the
user never saw. The picker cannot leave them out, because nothing outside
`gui/font` can ask whether a face has a glyph.

## What is asked

A question the toolkit can put to the font cache it already has (the one
`guitk::text` draws through): **can the faces installed -- the UI face and
its fallbacks, in the order they are tried -- draw this text?** For a single
character, whether some face maps it to a glyph; for an emoji sequence (a
ZWJ family, a flag, a skin tone), whether the face that would draw it forms
it as one cluster rather than falling back to its parts.

Something like:

```rust
impl SystemFont {
    /// Whether every grapheme of `text` is drawn from a face that has it --
    /// no `.notdef` glyph anywhere in the shaped run.
    pub fn draws(&self, text: &str) -> bool;
}
```

The pieces are there: `ShapedRun`'s glyphs carry a `GlyphKey` whose face and
glyph id are `pub(crate)` today (`gui/font/src/shape.rs`), and
`Face::glyph_index` answers the single-character question per face. What is
missing is a public way to ask.

## What lane C does when it lands

`guitk::text` gets `can_draw(text) -> bool` over the thread's cache, and the
picker leaves out of its categories and its search every character no face
draws -- computed once per category, so a category's grid is a few thousand
cheap questions at most, asked when it is first shown.
