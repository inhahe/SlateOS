## 1320. Face fallback: a line is drawn from as many faces as it needs, chosen a grapheme cluster at a time, shaped per face with the paragraph's levels

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** text was drawn from exactly one face per family, so every
character the UI face lacks -- an emoji, an Arabic name, a CJK file name, an
arrow -- came out as a box, whatever other fonts were installed. Now a font
carries an ordered list of fallback faces, and each character the face lacks
is drawn from the first fallback that has it. The toolkit and the compositor
still shape, measure and draw one run, so nothing that walks text had to
change, and a line the UI face covers is shaped exactly as before.

**Decision.**

* **Where:** in `osfont::system::SystemFont` (`with_fallbacks`) and
  `FontCache` (`set_fallbacks`), the one type the toolkit and the compositor
  both draw with -- not in the toolkit, so the two cannot fall back
  differently. Which faces are the fallbacks is `FontDb`'s question and so
  lane C's: `requests/f-cd-the-os-image-ships-no-fonts...` asks for an
  `install_fallback_faces` beside `install_ui_faces`.
* **The unit is the grapheme cluster** (`itemize.rs`, UAX #29's rules that
  matter here): a letter and its accents, and an emoji sequence -- a flag, a
  skin tone, a ZWJ family -- each go to one face, the first that has every
  character in it, or failing that the first that has its first.
* **Text or emoji presentation** decides the order faces are tried in: U+FE0F,
  U+FE0E, then `Emoji_Presentation` (a generated table, `gen_emoji_tables.py`,
  Unicode 16.0 like the crate's others). A colour face -- one with `COLR`,
  `CBDT` or `sbix` -- goes first for a cluster asking for emoji and last
  otherwise.
* **One paragraph, several shapings.** The bidi levels are resolved over the
  whole line; each face's stretch is shaped in its face with its share of
  them (`ScaledFont::shape_leveled`, split out of `shape_with`), and the
  drawing order is recomputed over the joined run. Within a stretch the
  recomputed order is the stretch's own (a reversal swaps two glyphs according
  to the levels between them only), so what each stretch's shaping did with
  its order -- kerning charged across a reversal, marks placed against moving
  pens -- stays right.
* **A glyph names its face** in the key's high byte (`GlyphKey::in_face`), 0
  being the font's own face: a font with no fallbacks makes the same keys as
  before, bit for bit. Hence at most 255 fallbacks.
* **Metrics** -- line height, ascent -- stay the font's own face's, as in every
  browser: a fallback glyph that is taller overflows its line rather than
  moving it.
* **The built-in bitmap face takes no fallbacks**: it is keyed by character,
  not glyph, and is what draws before there are faces to fall back to.

**Alternatives.**

| | For | Against |
|---|---|---|
| Per character, not per cluster | simpler | splits `e` + accent and every emoji sequence across faces |
| Shape each stretch as its own paragraph | no `shape_leveled` | an Arabic word in an English sentence resolves as its own RTL paragraph |
| Fallback in the toolkit | the font list lives there | the compositor draws with its own cache; two implementations to keep agreeing |

**How to reverse.** `SystemFont::with_fallbacks` and `FontCache::set_fallbacks`
are the only entry points; a cache never given fallbacks behaves as before.
