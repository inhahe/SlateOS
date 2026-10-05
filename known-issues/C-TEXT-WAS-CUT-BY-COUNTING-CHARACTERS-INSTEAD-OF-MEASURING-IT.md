## C-TEXT-WAS-CUT-BY-COUNTING-CHARACTERS-INSTEAD-OF-MEASURING-IT (lane C, 2026-08-17) - **fixed**

**What.** Twenty-odd places across `gui/` and `apps/` decided how much text
fits by arithmetic on a character count instead of by asking the font. The
pattern always looked the same: pick a number that is roughly how wide a
character is, divide the box by it, cut the string there. It produced four
distinct generations of the same bug, each one a "fix" for the previous:

1. **A hardcoded pixel constant** - `13.2`, `7.0`, `font_size * 0.55`,
   `title_size * 0.62`. Wrong for every string, and wrong in a direction that
   changes with the string: a line of `M`s overflows, a line of `i`s leaves a
   third of the box empty.
2. **`text::digit_advance`** - a digit's advance in the *proportional* UI face,
   used as though it were a cell width. Right only if the face is monospace,
   and the UI face is not.
3. **`chars().count() * text::cell_advance(...)`** - a nominal cell count in the
   mono face. Still not a width: a tab, a CJK ideograph, a combining mark and a
   `.notdef` box are one character each and are not one cell each.
4. **A character budget compared against a byte length** - `max_chars =
   width / 7.0`, then `if s.len() > max_chars` and `&s[..max_chars]`. This one
   is not a layout error: `&str[..n]` **panics** when `n` lands inside a
   multi-byte character, so the *process aborts*. Four such sites existed, and
   at least two were reachable from data the user does not control (an RSS
   article body, a screenshot path).

**What was actually wrong, and the rule that came out of it.** Most of these
callers were pre-truncating text for a `RenderCommand::Text` that already
carried `max_width` + `TextOverflow::Ellipsis` - that is, the renderer was
already going to elide it, correctly, by measurement. A caller that truncates
first is not being careful; it is supplying **a second, worse answer to a
question that is already answered**, and the worse one wins because it cuts
first. So six of those call sites were **deleted**, not repaired
(`gui/desktop/src/taskbar.rs`, `window_peek.rs`, `apps/mindmap`'s sidebar,
`apps/dbviewer` x2, `apps/photomanager`, `apps/slides`).

The exception is a caller where text *follows* the elided part - mindmap's
`Selected: "name" (ID: 7)`, the OSD's `Screenshot: .../shot.png` - because a
renderer can only cut the end of a line. Those now measure the parts that must
survive and give the remainder to the name.

**The missing primitive.** `text::wrap` deliberately never breaks inside a word,
because where to break one is a per-script decision that belongs to a real line
breaker. That is right for a paragraph that can grow taller, and fatal for a box
that cannot: a desktop icon's 72 px cell, or a Japanese file name, which
contains no space at all and so came back as one over-long line at any width.
`text::wrap_hard` (and `ScaledFont::wrap_hard`, `SystemFont::wrap_hard`,
`ShapedRun::hard_breaks`) is the counterpart: it wraps on words first and breaks
the remainder by measured fit. `hard_breaks` shapes each over-long run **once**
and walks it, rather than calling `fit` per piece, which would be quadratic in a
length chosen by whatever feed or file name produced the text. `wrap` remains
the default; `wrap_hard` is for boxes that cannot grow.

**Paint's text tool was a separate, worse instance.** It did not merely
mis-measure - it stamped one *blank filled rectangle* per character onto the
layer, on a `font_size * 0.6` advance, and called that a rasterization. Text
placed on an image and saved to disk came out as a row of blocks. It now
rasterizes through `guitk::text::draw_onto_canvas`, which was added for it: the
existing `Surface` path forces every pixel it touches opaque (right for a
screenshot, wrong for a paint layer), so the new path renders the string to a
white-on-black coverage mask and blends the colour at that coverage, leaving an
anti-aliased glyph on a transparent layer with a transparent edge rather than a
black halo.

**Where the tests are.** Each fix carries a test that states its premise rather
than assuming it - `icons::wrap_long_label_two_lines` asserts the sample label
really is too wide for one line before asserting it takes two, because the
previous version of that test simply guessed and failed when the guess was
wrong. `paint::placed_text_is_as_wide_as_the_font_makes_it_not_as_the_character_count`
compares ten `W`s against ten `i`s, which is the one comparison no
character-count implementation can pass.

**What is left.** Nothing of this class is known to remain in `gui/` or
`apps/`; the sweep ended on a grep for `digit_advance`, `* 0.55`, `* 0.62`,
`/ 7.0` and friends whose surviving hits are all either real geometry (a
calendar's `w / 7.0` is seven days) or comments in the files listed above
explaining what used to be there. `apps/explorer/src/thumbs.rs` keeps a
character count on purpose, and says so: its synthetic text-file minimap has no
font in the path at all, so a character count is the honest unit there.
