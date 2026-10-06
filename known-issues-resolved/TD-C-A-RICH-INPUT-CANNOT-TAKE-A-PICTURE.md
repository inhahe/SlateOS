### [C] TD-C-A-RICH-INPUT-CANNOT-TAKE-A-PICTURE -- 2026-10-05 — FIXED 2026-10-06

**Status:** FIXED 2026-10-06 on lane C's branch, on main with lane C's next
publish (`design-decisions.md` §1486). A picture is one character of the
document -- U+FFFC, `richinput::OBJECT` -- with the picture kept beside it at
that offset (`RichDoc::picture`, `pictures`), so it is selected, deleted,
cut, copied, pasted and undone as a character is. It is laid out as a piece
of its own, standing on the baseline, no wider than the box
(`layout::Piece::picture`), drawn by naming it (`RenderCommand::Image`), the
program handing the window each picture shown (`picture::Uploads`). It comes
in from the program (`RichInput::insert_picture`, `Picture::decode` for a
file), from a paste -- the program's clipboard carries a picture now
(`clipboard::set_picture`, `picture`) -- and from a drop
(`RichInput::drop_data`). A picture copied in another *program* still waits
on the system clipboard (C-Q29,
`TD-C-NOTHING-CAN-ACTUALLY-COPY-AND-PASTE-BETWEEN-PROGRAMS`), which is that
issue's and not this one's. Tests: `picture_tests.rs`; `doc_tests.rs`'s
pictures; `layout_tests.rs`'s `a_picture_is_a_piece_standing_on_the_baseline`,
`a_wide_picture_is_shown_at_the_boxs_width`,
`a_line_breaks_either_side_of_a_picture`,
`a_caret_beside_a_picture_is_at_its_edge`; `richinput_tests.rs`'s
`a_picture_is_put_in_and_taken_out_as_a_character`,
`a_copy_with_a_picture_pastes_whole`,
`a_picture_selected_alone_is_copied_as_itself`,
`a_dropped_picture_lands_where_it_was_dropped`,
`a_picture_is_drawn_standing_on_the_baseline`,
`a_selected_picture_between_two_directions_is_boxed_where_it_is`,
`a_click_on_a_picture_goes_to_its_nearer_edge`; `editmenu`'s
`a_picture_alone_is_pasted_only_where_pictures_go`.

**In short:** `roadmap-detailed.md` §3.5 asks for a rich input "with
formatting and image paste". The toolkit's rich field (`guitk::richinput`)
takes formatting, but no picture: nothing can paste one, because the program's
clipboard (`guitk::clipboard`) holds text only, and the system's clipboard --
which a picture copied in another program would come from -- is not yet
reachable at all (`TD-C-NOTHING-CAN-ACTUALLY-COPY-AND-PASTE-BETWEEN-PROGRAMS`,
`open-questions.md` C-Q29).

**Where:** `gui/toolkit/src/richinput.rs` (`paste`) and `richinput/doc.rs`
(a document has characters and formats, and no object in the text).

**The proper fix:** once the clipboard carries more than text -- a
multi-format clipboard, `guitk::dnd::DataObject`'s formats being the shape it
already has inside a program -- a picture becomes an object in the document:
the object replacement character (U+FFFC) in the text, with the picture's id
and size kept beside it, laid out as a piece as tall as the picture, and
pasted, dragged in, copied and undone as any other stretch of the document is.
