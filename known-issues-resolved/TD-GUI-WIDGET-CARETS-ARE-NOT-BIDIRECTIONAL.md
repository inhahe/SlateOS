## TD-GUI-WIDGET-CARETS-ARE-NOT-BIDIRECTIONAL

**Status: FIXED 2026-08-16.** All four steps of the proper fix below landed.
`gui/toolkit/src/text.rs` gained `TextCursor { byte, affinity }`, `caret_x`,
`selection_boxes` and `cursor_at` (each with a `_in` sibling taking a
`FontFamily`, for callers drawing inside a `PushFont` scope);
`gui/font/src/shape.rs` gained `ShapedRun::selection_rects`; `pathbar`'s
`width_before` and `textview`'s `x_of_col` are deleted, along with `pathbar`'s
`byte_to_char_offset`/`char_to_byte_offset`, which existed only to bridge the
character-index hit test to the byte-index cursor.

Three things came out differently from the plan written below:

* **`selection_rects` returns a `Vec`, not an `impl Iterator`.** The whole
  logical→selected map has to exist before the first box can be emitted,
  because the glyph drawn first may be the last one logically. Laziness would
  buy a caller nothing and cost the signature its clarity.
* **`TextCursor` is deliberately not `Ord`.** Two cursors at one offset with
  different affinities are one place in the text and two on the screen, so
  neither "comes first". Code wanting an order compares `.byte`.
* **`textview` needed the *selection* half, not the caret half.** It is a
  read-only view: it has selection and search highlights and no insertion
  caret at all, so `x_of_col` was replaced by `selection_boxes_of_cols`, which
  emits one box per span-and-direction run and merges the ones that abut.
  Its sibling `col_at_x` was left counting characters, which is correct — a
  selection endpoint is a logical position, and the affinity is a question for
  whoever draws, which for a selection is `selection_boxes_of_cols`.

Two further sites turned up that hold a text cursor but never draw one —
`widget.rs`'s `TextInput` and `modal.rs`'s `InputDialog` — so neither needed an
affinity. Both did, however, hold a *byte* offset and move it by *one byte* per
keypress, which is a separate and worse bug; see
`GUI-TEXT-INPUT-CURSORS-STEP-BY-BYTES` below.

The arrow-key half — **visual-order motion**, the left arrow moving left on
the screen whichever way the text runs — was out of scope here. On 2026-08-17
the *machinery* for it was built (`ShapedRun::caret_left`/`caret_right` in the
font crate, `text::caret_left`/`caret_right` in the toolkit), but the arrow keys
were **not** switched over to it: whether they should move visually or logically
is an open operator question, `open-questions.md` → **C-Q2**, and both answers
are defensible. See `TD-GUI-ARROW-KEYS-MOVE-IN-LOGICAL-ORDER` below, which
remains OPEN and now records exactly what changes when C-Q2 is answered.

The affinity added here turned out to be a prerequisite in a stronger sense than
"useful": a caret rebuilt from its byte offset each keypress does not merely
land on the wrong side of a boundary, it steps over an entire right-to-left run
in one press. That is why `TextInput` and `InputDialog`, called out four
paragraphs above as needing no affinity *because they never draw one*, in fact
needed one as soon as they had to **move** — and why they were converted to
`TextCursor` on 2026-08-17 even though their motion is still logical.

The original entry follows.

**Status:** ~~OPEN~~ FIXED

**What.** The font layer now places a caret correctly in bidirectional text
(`TD-FONT-CARETS-ARE-NOT-BIDIRECTIONAL`, fixed 2026-08-16): `ShapedRun::x_of`
measures across the screen and takes an `Affinity`. None of the widgets that
draw a caret call it. They all still place it by measuring the *width of the
text before it*, which is `ShapedRun::width_upto` under another name — the
number that entry renamed precisely because it is not a caret position.

Three sites:

* `gui/toolkit/src/pathbar.rs` — `width_before(text, bytes)`, which is
  `text::width` of a prefix slice. Used for the caret **and** for the selection
  rectangle.
* `gui/toolkit/src/textview.rs` — `x_of_col`, which walks the spans of a
  wrapped line summing `span_width` and then adds `text::measure(prefix)` for
  the partial span.
* `gui/toolkit/src/text.rs` — `char_index_at` returns `Hit::offset` and
  discards the affinity, because its callers count characters. That is correct
  for what it returns, but it means the affinity the click produced is thrown
  away at the boundary of the toolkit and cannot reach whoever draws the caret.

**Symptom.** In a path or a document containing a right-to-left run — a Hebrew
or Arabic directory name, a quoted Arabic phrase in a markdown file — the caret
is drawn at the distance *into the text*, not the position *across the line*.
Click at the visual right end of a Hebrew word and the caret appears at its
left end; type there and the character is inserted where the caret was drawn
rather than where the click was. Selection highlighting in `pathbar` is wrong
in the same way, and additionally cannot be right in its current shape: a
logically-contiguous selection that spans a direction change is two or more
*disjoint* rectangles on the screen, and `width_before` can only describe one.

**Why it is filed rather than fixed.** The font-level fix is complete and this
is a separate change with a design question in it: a widget cursor is a byte
index today, and a correct caret needs a byte index *plus* an affinity. That
value has to be stored in each widget's cursor state, updated by every
operation that moves it (click sets it from the `Hit`; arrow keys set it from
the direction of travel; typing and deletion set it downstream), and preserved
across the undo stack in `textview`. Bolting `x_of(.., Affinity::Downstream)`
onto the existing sites would fix the drawn position for exactly half of all
boundary crossings and would silently keep the other half wrong — which is the
failure mode the font entry argued against.

**Proper fix.**

1. A `TextCursor { byte: usize, affinity: Affinity }` in `gui/toolkit`, and
   cursor fields changed from `usize` to it. `Affinity::Downstream` is the
   right default, so `TextCursor::from(byte)` covers the sites that genuinely
   have no opinion.
2. `char_index_at` grows a sibling that returns the whole `Hit` — or better,
   returns `TextCursor` — leaving the character-counting one for callers that
   only want an index.
3. `pathbar::width_before` and `textview::x_of_col` become `x_of` calls against
   the shaped run, with the cursor's affinity. Both already shape the text to
   measure it, so no extra shaping is introduced.
4. Selection: a `selection_rects(from, to) -> impl Iterator<Item = (f32, f32)>`
   on `ShapedRun`, walking the drawn order and emitting one box per maximal
   stretch of the logical range that is contiguous on screen. This is the piece
   with real work in it; the caret parts above are mechanical once (1) exists.

Arrow-key motion is deliberately *not* in this list. Visual-order cursor
movement (left arrow moves left on the screen, whichever direction the text
runs) is a further question, and the affinity is a prerequisite for it rather
than a solution to it.

**Where.** `gui/toolkit/src/pathbar.rs` — `width_before` and its callers;
`gui/toolkit/src/textview.rs` — `x_of_col`, `col_at_x`;
`gui/toolkit/src/text.rs` — `char_index_at`; `gui/font/src/shape.rs` —
`x_of`/`offset_at`/`Affinity`, which is everything the fix needs from the font.
