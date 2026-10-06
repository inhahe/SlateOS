### [C] TD-C-A-RICH-INPUT-LAYS-ITS-RUNS-OUT-LEFT-TO-RIGHT -- 2026-10-05

**Status:** OPEN

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
