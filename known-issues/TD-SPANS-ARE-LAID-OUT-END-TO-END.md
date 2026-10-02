## TD-SPANS-ARE-LAID-OUT-END-TO-END (lane C, 2026-08-17)

**What.** Three more places have the defect `TD-EDITOR-IS-NOT-BIDIRECTIONAL`
item 4 just had fixed — two widgets in `gui/toolkit/src/textview.rs` and
`apps/markdowneditor` — and all three were left alone deliberately: the fix
that worked for the editor does not fit them, for a reason worth writing down
before someone tries it.

`RichTextView` draws a wrapped line by walking its `RichSpan`s, emitting a
`RenderCommand::Text` per span at a running `x` and advancing
`x += self.span_width(span, heading)` (`textview.rs:1821` for selection boxes,
`:2541` for the drawing itself; `span_width` at `:1751` is
`crate::text::measure`). Laying the pieces out end to end **is** the assumption
that screen order equals byte order — the same assumption, with the same
consequence: on a line containing a right-to-left run the spans' glyphs belong
interleaved with each other, so there is no `x` at which a span can be drawn as
one left-to-right piece.

`SimpleTextView` (the ANSI/terminal view) advances by
`columns(&span.text) * self.config.char_width` (`:1125`) instead. That is
narrower than a bug: a terminal *is* a grid of cells and advancing by cells is
the terminal's own model, not a mis-measurement. It is still wrong under a
right-to-left run — a terminal has to reorder within the line to display one —
but fixing it is a terminal-bidi question (BiDi in a cell grid), not the same
question as `RichTextView`'s.

`apps/markdowneditor` has the same shape as `RichTextView` and one extra fault
on top. It emits a `RenderCommand::Text` per `HighlightSpan`
(`markdowneditor/src/main.rs:2881–2898`) positioned by
`col_x(line, span.start, text_x)` — and `col_x` (`:139`) is
`text_x + prefix.chars().count() * char_width()`, a *nominal* character width
multiplied by a character count. That is wrong before bidi ever enters: it
assumes every glyph is exactly `char_width()` wide, so on a proportional face
the error compounds along the line and every span after the first lands in the
wrong place. The same expression positions the caret, so the caret and the text
drift together and the editor looks self-consistent while both are wrong. Fix
the nominal width first — that one is a plain bug with a plain fix
(`text::measure`, exactly as `apps/editor`'s caret comment already argues) —
and the span layout with the rest of this entry.

**Why `RichText` does not simply apply.** The editor's spans differ only in
*colour*, which is why a `TextSpan { end, color }` could carry them. These do
not:

| Carrier | Per-span attributes beyond colour |
|---|---|
| `RichSpan` → `RichSpanStyle` (`:1215`) | `weight`, `font_style`, `font_size`, `bg_color`, `underline`, `strikethrough`, `link` |
| `SimpleTextView` → `AnsiStyle` (`:161`) | `bg`, `bold`, `dim`, `italic`, `underline`, `reverse` |
| `markdowneditor` → `HighlightSpan` (`:2197`) | `weight` |

A span that changes the *size* or the *weight* changes the shaping, so it is
not merely a colour attribute painted onto glyphs that were shaped uniformly —
it is a genuine style run, and shaping a line whose runs differ in face means
shaping each run and then ordering the results, which is what a real text
engine (HarfBuzz + a bidi pass + a line breaker) does. `underline`,
`strikethrough` and `bg_color` are worse still: each needs the *rectangles* the
span occupies on screen, and under bidi one span occupies several disjoint
rectangles — exactly the problem `text::selection_boxes` already solves for
selections, and the same machinery is the answer here.

**The proper fix.** Widen the render primitive from "colour per byte range" to
"style per byte range" — a `RichTextSpan { end, color, bg, weight, size,
decorations }` — and have the renderer shape each style run, order the runs by
the bidi algorithm over the whole line, then emit glyphs and, for each span,
the set of rectangles its glyphs cover (which the decorations and the
background are drawn into). That is a larger change than §455's, and it wanted
`TD-EDITOR-IS-NOT-BIDIRECTIONAL` step (d) done first: (d) establishes
shape-the-whole-line-and-clip in the editor, which is the same shape this
needs, and doing them in the other order means designing the multi-run
machinery against no caller.

**That prerequisite is met as of 2026-08-17** — step (d) landed, and the
pattern it establishes is worth copying verbatim here: shape the whole line,
translate it under a clip rectangle rather than slicing it, and measure
`max_width` from the *shifted* x. `apps/editor`'s `draw_tokens` is the worked
example.

**Severity.** Low today for the bidi part, same as the editor's:
uni-directional text lays out correctly under the present code, and
`RichTextView`'s callers are help text and markdown-ish rendering. It bites on
a document with a quoted Arabic or Hebrew phrase — which is precisely what a
markdown view exists to display. **`markdowneditor`'s nominal character width
is not low severity** and is not really part of this entry's question: it is
visibly wrong on any proportional face, today, in English.

**Where.** `gui/toolkit/src/textview.rs`: `RichTextView::span_width` (~1751),
`selection_boxes_of_cols` (~1796–1821), the draw loop (~2470–2545); and
`SimpleTextView`'s draw loop (~1090–1126) for the terminal-grid variant.
`apps/markdowneditor/src/main.rs`: `col_x` (~139), the span draw loop
(~2881–2898), and the caret at (~2902).

**Status 2026-08-17: the nominal-width half is fixed everywhere; the
end-to-end/bidi half is untouched.** The two are separable, and only the
second one is what this entry is really about.

The sweep that closed the first half started from this entry's own sentence
that `markdowneditor`'s `col_x` "is not low severity", and found the same
defect in five places, in three generations:

| Generation | Shape | Sites |
|---|---|---|
| 1 | a hardcoded pixel constant | `apps/launcher/src/main.rs`, `gui/desktop/src/launcher.rs` (`INPUT_FONT_SIZE * 0.55`) |
| 2 | `text::digit_advance` — a *digit's* advance in the proportional UI face | `apps/filediff`, `apps/snippets`, `apps/tmux`, `apps/hexeditor`, `SimpleTextView` (each a previous round's "fix" for generation 1; the doc comment at each site records the sequence) |
| 3 | `chars().count() * text::cell_advance(..)` — a nominal cell count | `apps/markdowneditor`, `apps/snippets`, `apps/filediff`, `SimpleTextView` |

All now call `text::measure_in(text, size, weight, family)` — the renderer's
own answer, so there is no second number left to keep in step. `grep -rn
"chars().count() as f32" apps/ gui/` returns nothing that positions text.
`apps/tmux` and `apps/hexeditor` were examined and correctly left alone: both
really are grids of known-ASCII content.

**The finding worth keeping is the tab.** Generation 3 looks defensible — a
mono face *is* a grid — and survived two previous rounds of "fixing" this bug
for that reason. It is still wrong for a **tab**: one `char`, drawn four cells
wide. A test written to assert the general claim
(`the_pen_advances_by_what_is_drawn`, in `apps/snippets`) failed on
`"\t": drawn 33.6, nominal 8.4`, which
means every token on a tab-indented line in the snippets viewer had been drawn
~25 px left of its highlight. The same reasoning covers a CJK ideograph, a
combining mark, and any `.notdef` substitution. **A nominal cell count is not a
safe approximation even in a monospace face** — that is the sentence the two
earlier fixes were missing.

Two adjacent bugs fell out of the same reading:

- `SimpleTextView::line_char_count` returned `s.text.len()` — **bytes** — so
  every column past a non-ASCII character was wrong independently of the
  advance question. It counts characters now.
- Four sites sliced a `str` at a caret byte offset (`self.query[..cursor]`,
  `line[..col]`), which **aborts the process** off a character boundary. All
  four now floor to a boundary or use `get(..).unwrap_or("")`. A caret is
  drawn mid-keystroke; it must draw wrong rather than take the app down.

`SimpleTextView` now derives every position from `column_offsets`, which
accumulates the renderer's per-character advance (O(n), not the O(n²) of
measuring a prefix per column — this view holds log output). That substitution
is only exact if measurement is additive on this font stack, so
`simple_view_advances_are_additive` asserts it and will fail loudly if a
kerning face ever arrives.

Commits `8b039f47d` (markdowneditor, snippets, filediff) and `796a5ddd0`
(`SimpleTextView`, both launchers). What remains open here is unchanged: spans
are still laid out end to end, which is still the assumption that screen order
equals byte order, and still needs the widened render primitive described
above.
