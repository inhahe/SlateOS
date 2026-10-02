## §455 — Multi-coloured text crosses the wire as byte-ranged spans, not as positioned glyphs

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the text editor colours its keywords, strings and comments by
drawing each one as a separate little piece of text, placed left to right at
the point where the previous piece stopped. That is fine for English, and wrong
for Hebrew or Arabic, where a quoted phrase's letters end up *interleaved* with
the punctuation around them rather than sitting in a block of their own — so
there is no single place on the screen where "the string literal" can be drawn.
The fix makes colour a property of a *letter* instead of a property of a *piece
of text to draw*. The question this entry settles is who works out which letter
gets which colour: the app that knows the keywords, or the compositor that
actually draws them. The answer is the compositor, and the app tells it "bytes
0 to 2 are blue, bytes 3 to 9 are green" — ranges of the *text*, not positions
on the *screen*.

**Context.** `known-issues.md` → `TD-EDITOR-IS-NOT-BIDIRECTIONAL` item 4.
`apps/editor`'s `draw_tokens` walked the syntax token list emitting one
`RenderCommand::Text` per token at a running `x`. Cutting a line into pieces
and laying them end to end is the assumption that screen order equals byte
order, applied once per token — which is exactly what bidirectional reordering
violates.

**The options.**

**(1) The app shapes the line and sends positioned, coloured glyphs.** The
editor calls `text::shape`, walks the resulting glyphs, gives each the colour
of the token containing its source byte, and sends a list of
`(glyph_id, x, y, colour)`.

*What changes:* nothing visible if it worked — but it cannot work here. The
compositor's `draw_text` **re-shapes the string itself** (`let run =
font.shape(text)`) and resolves the family from its own `font_stack`, which
`PushFont`/`PopFont` commands manipulate. The app does not know which face will
be used, so any glyph ids it produces are for the wrong font. It is also the
largest thing on the wire: a glyph record per character rather than a span
record per token.

**(2) The command carries the string plus byte-ranged colour spans, and the
renderer resolves colour per glyph.** `RenderCommand::RichText { text, spans,
color, … }`, where a span is `{ end: u32, color }`. The renderer is already
shaping; `ShapedGlyph` already carries `cluster`, the source byte offset. So
the byte→glyph translation costs a lookup per glyph and nothing else.

*What changes:* the app sends what it actually knows (where its tokens are in
the text) and the renderer does what only it can do (decide where those bytes
land on the screen).

**Chosen: (2).** Not on elegance — on the fact that the app *cannot* correctly
shape, because the family is resolved on the far side of the wire. Option (1)
would require moving font-stack resolution into the app, which inverts the
whole point of the `PushFont` command. (2) is additionally the compact
encoding, and it reuses the `max_width` / `overflow` / clipping machinery the
`Text` command already has, because it is the same command with one extra
field.

**Sub-decision: span ends are cumulative, not start/end pairs.** A span is
`{ end, color }` and begins where the previous one ended (at 0 for the first).

- *Start/end pairs* let a caller emit two spans that disagree about a byte, or
  leave a byte in no span at all. Both are representable, so both have to be
  defined — and the definition ("the first span wins", say) is a rule nobody
  reads, whose violation looks like a colouring bug rather than an error.
- *Cumulative ends* make gaps and overlaps **unrepresentable**. A gap the
  caller wants must be written down as a span in the fallback colour, which is
  a decision made visibly rather than a hole fallen into.

The editor's `draw_tokens` shows the effect: it fills a gap the tokenizer might
leave with an explicit plain-coloured span, and
`every_byte_of_a_line_is_covered_by_some_token` asserts the fill never fires.
Under start/end pairs that gap would have been silently dropped — which is how
a byte of the user's text disappears off the screen.

**Sub-decision: bytes past the last span take the command's own `color`, and
`TextSpan::color_at` returns `Option<Color>` rather than a fallback.** The
fallback is per-command, not per-span, so a caller does not have to describe
the tail. `color_at` returning `Option` rather than taking a fallback is for
the compositor, which works in packed ARGB: taking a `Color` fallback would
force an ARGB→`Color`→ARGB round trip once per glyph purely to hand back
something it already had.

**Sub-decision: lookup is a binary search, not a forward walk.** Tempting to
walk the spans alongside the glyphs, since both are in order — but glyphs are
*drawn* in visual order (`ShapedRun::draw_order()`), and under bidi that order
does not ascend by cluster. A forward walk would mis-colour exactly the case
this whole change exists for. `partition_point` costs log(spans) and is order
independent; `resolution_does_not_depend_on_query_order` in
`gui/toolkit/src/render.rs` is the test that says so.

**Sub-decision: a new wire tag (0x0D), not a `PROTOCOL_VERSION` bump.** Same
argument already recorded for `FontFamilyTag`: old frames still decode
identically because nothing about them changed, and an old decoder meeting the
new tag fails cleanly with `DecodeError::BadTag` rather than misreading the
bytes. A version bump would additionally reject every old frame, which buys
nothing.

**The cost, which turned out to be negative.** The old comment above
`draw_tokens` accepted losing kerning across token boundaries as a small price.
Measured against the shaping instrument in `gui/toolkit/src/text.rs`, the
decomposition was not a price at all but a loss: each shaping carries a fixed
cost of ~3.6 us on top of ~0.75 us per character, so cutting a line into *n*
pieces pays the fixed cost *n* times. An 80-character line of 40 tokens — an
ordinary line of code — cost **2.3x** what shaping it whole costs. So this
change makes the output correct under bidi *and* the common case faster, and
the lost kerning comes back for free.

**What it does not fix.** `draw_tokens` still slices the line at `scroll_col`
and hands the renderer the tail, so the string being shaped is still a
substring — and the visible portion of a bidirectional line is not the shaping
of a substring of it. That is item 3 / step (d) of
`TD-EDITOR-IS-NOT-BIDIRECTIONAL`, deliberately left separate.
`gui/toolkit/src/textview.rs` has the same end-to-end layout in two widgets and
is *not* covered by this decision, because its spans carry size and weight as
well as colour — see `TD-SPANS-ARE-LAID-OUT-END-TO-END`.

**Where.** `gui/toolkit/src/render.rs` (`TextSpan`, `RenderCommand::RichText`,
`RenderTree::rich_text` / `rich_text_clipped`), `gui/compositor/src/main.rs`
(`blit_run` / `draw_text` take `spans`), `gui/remote/src/lib.rs`
(`Tag::RichText`, `MAX_SPANS`, `write_spans` / `read_spans`),
`apps/editor/src/main.rs` (`draw_tokens`, `rebase`).
