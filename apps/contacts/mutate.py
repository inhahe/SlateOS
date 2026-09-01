"""Mutation test for the contacts app's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Contacts is the forty-eighth application in this campaign.  `main` was:

    fn main() {
        let mut store = ContactStore::new();
        load_sample_data(&mut store);
        let app = ContactsApp::new(store);
        let _commands = app.render();
    }

It built the address book, drew one frame into a `Vec`, dropped it and exited
zero.  Nobody saw the picture and nothing could be pressed.

What the wiring exposed, in rough order of how badly it would have shown:

  * **Every control in the program was a painted rectangle that answered
    nothing.**  The drawing pass recorded no hit boxes at all, so the `+`
    button, the search bar, the filter and sort cells, the A-Z rail, the
    contact rows, Edit, Star, Delete, Call, Email, Map, the form fields, Save,
    Cancel and Merge were all pictures of controls.
  * **The `Groups` and `Duplicates` panels were unreachable.**  Both were fully
    written and fully drawn, and the only way to select either was to assign to
    `self.view` from code -- no control anywhere set it.
  * **A fifteen-field form had a two-field `Tab` cycle.**  `FormField::next`
    named the second field's successor explicitly rather than wrapping over
    `ALL`, so thirteen fields could not be typed into by any route.
  * **Sixteen `#[allow(dead_code)]` attributes** covered the file, five of them
    hiding palette entries nothing used.
  * **Nineteen runs of text were drawn with `max_width: None`**, so a long
    company name or a paragraph of notes walked straight over the column beside
    it and off the edge of the panel.
  * **The layout was `SIDEBAR_WIDTH` and `self.window_height` written into the
    commands.**  The sidebar could not shrink, and the height was the size the
    app *believed* it was -- which, after a resize, is the size it used to be.

Five more faults were found by the size sweep and the interaction sweep while
these tests were being written:

  * The status strip was placed *after* the header and pushed down to clear it,
    so a thirty-pixel-tall window drew "Ready" below its own bottom edge.
  * `draw_list` advanced its cursor past the bottom of the list and kept
    emitting rows and dividers into a clip that could not show them.  A clip
    makes ink invisible; it does not make it free.
  * Text was emitted with no visibility check at all, so a 40x30 window drew
    "Contacts" bounded to a width of zero.
  * The visibility test asked about the row a run sits in rather than the run
    itself, so a star centred in a visible row was drawn four pixels below
    anything that could show it.
  * A merge reported "Merged into #7" -- the id of a contact the merge had just
    invented and the user had never seen.

Two more were found by the mutation sweep itself, both the same fault in two
places and both invisible to all 230 tests that existed at the time:

  * `draw_contact_fields` walked a cursor down the detail panel and never asked
    whether it still had panel left, so a contact with six phone numbers and a
    paragraph of notes painted its group chips **on top of the Edit, Star and
    Delete buttons** at 640x480.  `draw_edit_form` did the same to Save and
    Cancel at 1024x768.  Neither could be seen by any test phrased over text or
    over controls, because `put_text` declines to emit a run the clip cannot
    show and `Frame::hit` trims a hit box to nothing -- the overrun leaves a
    trace only in the *fills*.  See known-issues.md lesson 107.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- the layout ----------------------------------------------------------
    (
        "a window size that is not a number is laid out rather than zeroed",
        "        let w = if w.is_finite() { w.max(0.0) } else { 0.0 };\n"
        "        let h = if h.is_finite() { h.max(0.0) } else { 0.0 };",
        "        let w = w.max(0.0);\n        let h = h.max(0.0);",
        ["a_size_that_is_not_a_size_still_produces_a_window"],
    ),
    (
        "the header is given its full height before the status line is placed",
        "        let status_h = STRIP_HEIGHT.min(h);\n"
        "        let status = Rect::new(0.0, (h - status_h).max(0.0), w, status_h);\n"
        "        let header_h = HEADER_HEIGHT.min(status.y);",
        "        let header_h = HEADER_HEIGHT.min(h);\n"
        "        let status_h = STRIP_HEIGHT.min(h);\n"
        "        let status = Rect::new(0.0, (h - status_h).max(header_h), w, status_h);",
        # Not `nothing_is_drawn_over_the_status_line`: hanging the strip past
        # the bottom edge does not make anything overlap it -- there is
        # nothing left to overlap. What it does is put a run of text outside
        # the window, which is the sweep's business.
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "the sidebar takes more of the window than it leaves the detail panel",
        "        let wanted_side = (w * 0.45).min(SIDEBAR_WIDTH + ALPHABET_BAR_WIDTH);",
        "        let wanted_side = (w * 0.95).min(SIDEBAR_WIDTH + ALPHABET_BAR_WIDTH);",
        # Not `the_detail_panel_is_given_up_before_the_list_is`: that test is
        # about which half survives a window too narrow for both, and the
        # answer is still the list. What the mutation breaks is the rule
        # governing widths where *both* are shown -- the panel, which holds a
        # paragraph of notes, is meant to be the wider of the two.
        ["the_detail_panel_is_never_narrower_than_the_sidebar_where_both_are_shown"],
    ),
    (
        "the A-Z rail is taken even when it leaves the list narrower than a row",
        "        let alpha_w = if side_w - ALPHABET_BAR_WIDTH >= MIN_LIST_WIDTH {",
        "        let alpha_w = if side_w > 0.0 {",
        ["the_alphabet_rail_is_given_up_before_the_list_falls_below_a_row"],
    ),
    (
        "a strip is taken before the list is left room for a whole row",
        "            if top + wanted + MIN_LIST_HEIGHT > bottom {",
        "            if top + wanted > bottom {",
        ["a_strip_is_only_taken_if_the_list_keeps_a_whole_row"],
    ),
    # -- what is painted -----------------------------------------------------
    (
        "only the list is filled, leaving the rest of the window bare",
        "        f.push(fill(l.window, BASE, 0.0));",
        "        f.push(fill(l.list, BASE, 0.0));",
        ["the_window_is_painted_edge_to_edge_at_every_size"],
    ),
    (
        "text is drawn unbounded, so a long company name walks over the column beside it",
        "        max_width: Some(ink.w),",
        "        max_width: None,",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "a run taller than its box is centred in it anyway",
        "fn ink_box(r: Rect, size: f32) -> Rect {\n    let size = size.min(r.h);",
        "fn ink_box(r: Rect, size: f32) -> Rect {",
        # Not `every_run_of_text_is_bounded_and_inside_the_window`: no call
        # site in the drawn states asks for a run tall enough that centring
        # it in a shorter box pushes it out of the *window*. A box overflows
        # by half the excess height, which for a 14-point run in a 12-point
        # box is a pixel. The property that actually breaks is the one
        # `ink_box` exists to guarantee -- a run stays inside the box it was
        # handed -- so the test that catches it has to ask `ink_box` itself,
        # over box heights the drawing code does not happen to produce.
        ["a_run_of_text_is_never_inked_outside_the_box_it_was_given"],
    ),
    (
        "text is emitted wherever the caller asks, clip or no clip",
        "    if ink.is_empty() || s.is_empty() || !f.is_visible(ink) {",
        "    if false {",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "the list keeps drawing rows after the cursor has run past its bottom",
        "        for contact in &contacts {\n            if cy >= l.list.bottom() {\n"
        "                break;\n            }\n            let letter = contact.first_letter();",
        "        for contact in &contacts {\n            let letter = contact.first_letter();",
        # Not a text test: `put_text` already refuses to emit a run the clip
        # in force cannot show, and `Frame::hit` drops a hit box the clip
        # trims to nothing, so a row drawn past the bottom of the list is
        # invisible *and* unclickable. What survives is the paint -- the
        # row's own fill, which is pushed unconditionally and lands wholly
        # outside the clip. Only a test that looks at fills sees it.
        ["nothing_is_painted_entirely_outside_the_clip_in_force"],
    ),
    (
        "the list's clip is opened and never closed",
        "        f.unclip();\n    }\n\n    /// One row of the contact list, at `cy` before clipping.",
        "    }\n\n    /// One row of the contact list, at `cy` before clipping.",
        ["the_frame_is_balanced_at_every_size_and_state"],
    ),
    (
        "the panel's clip is pushed one row wider than the panel",
        "        f.clip(l.panel);",
        "        f.clip(Rect::new(l.panel.x, l.panel.y, l.panel.w, l.panel.h + 60.0));",
        ["every_clip_lies_inside_the_window"],
    ),
    (
        "the detail panel is run down over the status strip",
        "        let panel = Rect::new(side_w, 0.0, panel_w, status.y);",
        "        let panel = Rect::new(side_w, 0.0, panel_w, h);",
        # Nothing actually *moves* under the strip at any size in the grid:
        # the panel's 24-pixel padding is larger than the 22-pixel strip, so
        # the content it holds stops short every time. What moves under is the
        # panel's own clip -- the permission rather than the symptom -- which
        # is why `nothing_is_drawn_over_the_status_line` now asks about clips
        # as well as about controls and paint.
        ["nothing_is_drawn_over_the_status_line"],
    ),
    (
        "the search box is lifted into the header above it",
        "        let search_outer = take(SEARCH_BAR_HEIGHT, 8.0);",
        "        let search_outer = take(SEARCH_BAR_HEIGHT, -20.0);",
        ["the_boxes_of_the_sidebar_do_not_overlap_each_other"],
    ),
    # -- where a press goes --------------------------------------------------
    (
        "the add button is not a control",
        "            f.hit(Target::AddContact, l.add_button);",
        "            f.hit(Target::Search, l.add_button);",
        ["the_add_button_opens_a_blank_form_with_the_keyboard_in_the_first_field"],
    ),
    (
        "a row answers for the contact below it",
        "        f.hit(Target::Contact(contact.id), row);",
        "        f.hit(Target::Contact(contact.id.saturating_add(1)), row);",
        ["pressing_a_row_selects_the_contact_whose_name_it_shows"],
    ),
    (
        "a row answers over less than the row it draws",
        "        f.hit(Target::Contact(contact.id), row);",
        "        f.hit(\n            Target::Contact(contact.id),\n"
        "            Rect::new(row.x, row.y, row.w, (row.h - 6.0).max(0.0)),\n        );",
        ["no_press_inside_the_list_falls_between_two_rows"],
    ),
    (
        # The obvious form of this mutation -- unclip around the `f.hit` so
        # the row is hit-boxed outside the list -- turns out not to test the
        # clip at all. The row loop skips a contact whose top has run above
        # `l.list.y` before it records anything, so at the scroll offsets a
        # test can reach, the rows that would escape the clip were never
        # drawn. Clipping the list to the *window* is the same fault stated
        # where it bites: every row is drawn and hit-boxed, and the clip that
        # is supposed to stop the ones scrolled past the bottom edge stops
        # nothing, because the window is larger than the list in every
        # direction.
        "the list is clipped to the window rather than to itself",
        "        f.clip(l.list);",
        "        f.clip(l.window);",
        # Not `a_row_scrolled_out_of_sight_is_not_clickable` either, for the
        # reason that killed the first form of this row: a row is only drawn
        # when it is at least partly inside the list, so no row is ever
        # entirely clipped away and every row that answers still answers. The
        # clip's whole job is to decide *how far* a row's hit box reaches, so
        # that is what has to be asked.
        ["every_row_answers_only_inside_the_list"],
    ),
    (
        "the search box is not a control",
        "        f.hit(Target::Search, l.search);",
        "        f.hit(Target::AddContact, l.search);",
        ["the_search_box_takes_the_keyboard_and_typing_narrows_the_list"],
    ),
    (
        "every cell of both strips cycles the filter",
        "            f.hit(target, cell);",
        "            f.hit(Target::CycleFilter, cell);",
        [
            "the_sort_cell_reads_the_order_in_force_and_changes_it",
            "the_view_cells_reach_the_two_panels_nothing_else_could_reach",
        ],
    ),
    (
        "every letter on the rail jumps to A",
        "            f.hit(Target::Letter(letter), cell);",
        "            f.hit(Target::Letter('A'), cell);",
        ["a_letter_on_the_rail_scrolls_the_list_to_that_letter"],
    ),
    (
        "every quick action places a call",
        "            f.hit(Target::Action(action), r);",
        "            f.hit(Target::Action(QuickAction::Call), r);",
        ["the_quick_actions_record_that_the_contact_was_reached"],
    ),
    (
        "Edit, Star and Delete all edit",
        "            put_text(f, inset(r, 8.0), &label, 13.0, fg, FontWeightHint::Bold);\n"
        "            f.hit(target, r);",
        "            put_text(f, inset(r, 8.0), &label, 13.0, fg, FontWeightHint::Bold);\n"
        "            f.hit(Target::EditContact, r);",
        ["edit_star_and_delete_each_do_their_own_job"],
    ),
    (
        "every field of the form focuses the first name",
        "                f.hit(Target::Field(field), box_r);",
        "                f.hit(Target::Field(FormField::FirstName), box_r);",
        ["pressing_a_field_gives_it_the_keyboard_and_typing_reaches_it"],
    ),
    (
        "Save and Cancel both save",
        "            put_text(f, inset(r, 8.0), label, 13.0, fg, FontWeightHint::Bold);\n"
        "            f.hit(target, r);",
        "            put_text(f, inset(r, 8.0), label, 13.0, fg, FontWeightHint::Bold);\n"
        "            f.hit(Target::Save, r);",
        ["save_writes_the_form_and_cancel_does_not"],
    ),
    (
        "the merge button is not a control",
        "                f.hit(Target::Merge(dup.contact_a_id, dup.contact_b_id), merge);",
        "                f.hit(Target::ShowDuplicates, merge);",
        ["the_merge_button_merges_the_pair_its_row_names"],
    ),
    (
        "every group row filters to the first group",
        "            f.hit(Target::Group(*gid), row);",
        "            f.hit(Target::Group(0), row);",
        ["a_group_row_narrows_the_list_to_that_group"],
    ),
    (
        "a press on bare background is answered by the last control that was drawn",
        "        let Some(target) = frame.hit_test(event.x, event.y) else {",
        "        let Some(target) = frame\n            .hit_test(event.x, event.y)\n"
        "            .or_else(|| frame.hits().last().map(|(t, _)| t.clone()))\n        else {",
        ["a_press_on_bare_background_changes_nothing"],
    ),
    # -- what the controls then do -------------------------------------------
    (
        "the form opens carrying the last contact's text",
        "        self.clear_edit_form();\n        self.view = DetailView::NewContact;",
        "        self.view = DetailView::NewContact;",
        ["the_add_button_opens_a_blank_form_with_the_keyboard_in_the_first_field"],
    ),
    (
        "Tab stops at the second field instead of wrapping over the whole form",
        "        Self::ALL\n            .get(i.saturating_add(1))\n"
        "            .or_else(|| Self::ALL.first())\n            .copied()\n            .unwrap_or(self)",
        "        match self {\n            Self::FirstName => Self::LastName,\n"
        "            _ => Self::FirstName,\n        }",
        ["tab_reaches_every_field_and_comes_back_to_the_first"],
    ),
    (
        "saving an edit writes only what the form holds",
        "                    updated.favorite = existing.favorite;",
        "                    updated.favorite = false;",
        ["saving_an_edit_keeps_what_the_form_does_not_carry"],
    ),
    (
        "a letter nothing is filed under scrolls to the top rather than saying so",
        '        self.status = format!("Nothing filed under {letter}");',
        "        self.scroll_offset = 0.0;",
        ["a_letter_nothing_is_filed_under_says_so_rather_than_scrolling_somewhere_else"],
    ),
    (
        "a merge reports the id of a contact nobody has seen",
        '            self.status = format!("Merged {name}");',
        '            self.status = format!("Merged into #{kept}");',
        ["the_merge_button_merges_the_pair_its_row_names"],
    ),
    # -- the entry points the platform calls ---------------------------------
    (
        "the picture is drawn at the size it was launched with rather than the given one",
        "        self.frame(width, height).into_tree()",
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()",
        ["the_picture_is_drawn_at_the_size_render_is_given"],
    ),
    (
        "the size the frame was drawn at is not remembered, so a press is answered blind",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {\n"
        "        self.window_width = width;\n        self.window_height = height;",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {",
        ["a_press_is_answered_against_the_size_the_last_frame_was_drawn_at"],
    ),
    (
        "a resize is not remembered, so a press before the next frame is answered blind",
        "                self.window_width = *width as f32;\n"
        "                self.window_height = *height as f32;",
        "                let _ = (width, height);",
        ["a_resize_moves_where_the_controls_answer"],
    ),
    (
        "the close button is answered with a redraw",
        "            Event::CloseRequested => Response::Exit,",
        "            Event::CloseRequested => Response::Redraw,",
        ["the_close_button_closes_the_window_and_nothing_else_does"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "contacts", timeout=300, only=only))
