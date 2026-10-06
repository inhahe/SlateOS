### [C] TD-C-A-RICH-INPUT-LAYS-ITS-RUNS-OUT-LEFT-TO-RIGHT -- 2026-10-05 — FIXED 2026-10-06

**Status:** FIXED 2026-10-06 on lane C's branch, on main with lane C's next
publish. The shaper's levels were within reach after all: `osfont::bidi`
resolves a paragraph's levels and orders a line's runs (`resolve`,
`render_levels`, `visual_order`), so the toolkit does it across fonts
itself. `richinput::layout` resolves each paragraph's levels once, cuts a
line's pieces where the direction changes as well as where the format
does (each piece knows whether it runs right to left), keeps a line's
trailing spaces at the paragraph's level (L1), and places the pieces in
screen order (L2). Within a piece, `text::caret_x` and `text::cursor_at`
place the caret and answer a click in its own direction. The caret's
existing `upstream` flag says which side of a change of direction it is
drawn on (`layout::x_at`); the arrow keys step through the places a caret
can be on the screen (`RichInput::step_on_screen`, `layout::caret_stops`),
and a selection across a change of direction is drawn as a box for each
piece's part. A line of one direction is laid out exactly as before.
Tests: `a_right_to_left_word_is_a_piece_of_its_own`,
`a_right_to_left_paragraph_starts_at_the_right`,
`a_piece_is_cut_at_a_format_and_at_a_direction`,
`a_click_finds_where_the_caret_is_drawn`,
`a_line_of_one_direction_is_unchanged` (`richinput/layout_tests.rs`),
`right_goes_right_across_a_right_to_left_word`,
`a_selection_across_two_directions_is_drawn_where_its_text_is`
(`richinput_tests.rs`).

**In short:** the toolkit's rich text field (`guitk::richinput`) shapes each
run of formatting -- each stretch alike in size and weight -- on its own and
places the runs across a line in the order they are written. A line in one
writing direction is right; a line mixing a right-to-left language with a
left-to-right one shows each run in its own direction but the runs in written
order, and the arrow keys move by the text's order rather than the screen's.
The plain fields (`textinput`, `textarea`) get both right, because each draws
a line in one font and shapes it whole.

**Where:** `gui/toolkit/src/richinput/layout.rs` (`paragraph`, `line`,
`x_of`, `offset_at`) and `richinput.rs` (`move_left`, `move_right`).

**How to see it:** type a Hebrew or Arabic word between two English words in a
rich field, and make the English words bold: the line reads with the
right-to-left word's runs in written order, and Left steps through the text
backwards rather than leftwards.

**The proper fix:** lay a line out by the Unicode bidirectional algorithm over
the whole line -- resolve its levels once, then reorder the runs' pieces into
visual order -- and step the caret by the reordered pieces, as
`text::caret_left` does within one font. That needs the shaper to report a
run's level and the line's visual order across fonts
(`gui/font`, lane F); until then the field is correct for text in one
direction, which is the case its users -- message bodies and notes -- have.
