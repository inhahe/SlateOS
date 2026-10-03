## TD-C-TWO-TOOLKIT-TEXT-FIELDS-DRAW-NO-CARET-AT-ALL — RESOLVED 2026-08-24

**Both fields are done.** `WidgetKind::TextInput` and `InputDialog` now share one
implementation of a single-line field's caret, selection and scrolling —
`gui/toolkit/src/textedit.rs`, written for this fix precisely so that the five
text fields in the tree stop each having their own answer. `InputDialog`
converts its offsets through `drawn_offset` first, so a password field's caret
and selection are measured in mask characters and never leak the secret's byte
length. Reasoning in `design-decisions.md` §546.

**One piece was deliberately not done at the time: clicking in an `InputDialog`
did not move its caret.** It could not, and the reason was a separate defect —
`TD-C-NO-MODAL-DIALOG-KNOWS-WHERE-IT-IS-WHEN-IT-IS-CLICKED` below. The widget
text field did support click-to-place, because a `Widget` has a layout box.
*(Closed later the same day: that entry is now fixed, and `InputDialog` places
its caret from a click like any other field. The shared arithmetic lives in
`textedit::cursor_at_click`, which both call.)*

The original report follows.

---

**`WidgetKind::TextInput` was done first**: it now has a focus, a caret gated on it, a
selection anchor with Shift+Arrow and painted selection boxes, click-to-place-
caret, and a horizontal scroll offset that keeps the caret inside the box. See
`design-decisions.md` §546, and the twenty tests from
`a_focused_field_draws_a_caret_and_an_unfocused_one_does_not` onwards in
`gui/toolkit/src/widget.rs`. **`InputDialog` in `gui/toolkit/src/modal.rs` is
still untreated** — the rest of this entry is about it.

`WidgetKind::TextInput` (`gui/toolkit/src/widget.rs`, render arm at ~line 618)
and `InputDialog` (`gui/toolkit/src/modal.rs`, ~line 1376) both **track** a
caret and move it, but neither one ever draws it. Each pushes a single
`RenderCommand::Text` for the field's contents and nothing else; the only
`Line` commands in either file are a `Separator` and a resize grip.

So a user typing into either of these sees the text change with no indication
of where the next character will go, and the arrow keys appear to do nothing
at all.

### Why it surfaced now

§541 switched both fields' arrows to *visual* caret motion, which put the
question "where is the caret drawn?" in front of me for all five text fields in
the tree. Three answer it — the path bar (`pathbar.rs:835`), the launcher and
the Run dialog all place theirs with `text::caret_x`. These two do not answer
it because they never asked it.

That also means these two fields are the only ones whose §541 conversion cannot
be tested end to end: `the_arrows_move_by_the_screen_and_keep_the_side_they_are_on`
and `a_plain_input_dialog_moves_its_caret_by_the_screen` can assert where the
cursor *went*, never where it is *shown*, because there is nothing shown. The
equivalents for the other three (`the_drawn_caret_moves_rightwards_every_time_
the_right_arrow_does`, `the_run_dialogs_drawn_caret_only_ever_moves_rightwards_
under_the_right_arrow`) are the tests that catch a caret that moves correctly
and draws wrongly — a defect these two cannot have, only because they draw
nothing.

### The proper fix

Push a vertical `RenderCommand::Line` at
`text::caret_x(value, *cursor, self.style.font_size, FontWeightHint::Regular)`
in each render arm, exactly as the launcher does, gated on the widget having
focus. Two further things fall out of it and should land in the same change
rather than after it:

- **Selection.** Neither field has a selection anchor at all, so neither can
  show a selection, and Shift+Arrow does nothing. The Run dialog's
  `selection_anchor: Option<usize>` is the shape to copy — deliberately a plain
  byte, since a selection is a range of text and has no side of a boundary to
  be on.
- **Horizontal scrolling.** Both draw with `TextOverflow::Ellipsis` and no
  scroll offset, so text longer than the field is cut at the right edge and a
  caret past that edge would be painted outside the field even once it exists.
  A caret without a scroll offset is a caret that leaves the box.

### Why it is not fixed in the same change as §541

It is a feature these widgets never had, not a regression §541 introduced —
their arrows were equally invisible before the switch. Bundling a
caret/selection/scrolling implementation into the change that moved five
fields' arrows would have made a reviewable diff unreviewable. Nothing depends
on it: `pathbar` is the toolkit field the shell actually uses for typing.
