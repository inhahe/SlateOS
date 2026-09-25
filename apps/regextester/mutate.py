"""Mutation test for the regex tester's fields, its library and its pointer
layer.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The tester drew three tabs, three flag buttons, three fields, a match list,
three sub-tabs, eight library chips and a library, and handled no pointer
event (known-issues, TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-
CLICKED).  Behind that, half of it was out of reach by any route: the test
input could not take a newline, the fields could be edited only at their end,
nothing scrolled, two of the three sub-tabs were drawn with nothing behind
them, and the library could neither load a pattern nor save one.

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
    # -- the test input --------------------------------------------------------------
    (
        "a newline cannot be typed into the test input",
        '            Key::Enter => self.input.insert("\\n", Self::capacity(ActiveField::Input)),',
        "            Key::Enter => false,",
        [
            "the_test_input_takes_new_lines_and_can_be_edited_anywhere",
            "multiline_can_be_tried_on_text_typed_in_the_window",
        ],
    ),
    (
        "the test input is edited only at its end",
        "        self.text.insert_str(self.caret, &taken);",
        "        self.text.push_str(&taken);",
        ["the_test_input_takes_new_lines_and_can_be_edited_anywhere"],
    ),
    (
        "Up and Down lose the column",
        "        let within = text::cursor_at(line, goal, NORMAL_TEXT, FontWeightHint::Regular).byte;",
        "        let within = 0;",
        ["the_test_input_takes_new_lines_and_can_be_edited_anywhere"],
    ),
    (
        "a press in the test input puts the caret nowhere",
        "        self.input.click(line, x - area.x + hscroll, extend);",
        "        let _ = (line, x, area, hscroll, extend);",
        ["a_press_in_the_test_input_puts_the_caret_under_it_and_a_drag_selects"],
    ),
    (
        "a drag selects nothing",
        "                if self.dragging {\n                    let before",
        "                if false {\n                    let before",
        ["a_press_in_the_test_input_puts_the_caret_under_it_and_a_drag_selects"],
    ),
    (
        "the test input does not follow its caret",
        "        let line = self.input.line_index(self.input.caret);\n"
        "        self.input_scroll = keep_in_view(self.input_scroll, line, self.input_rows());",
        "        let _ = self.input.caret;",
        ["the_test_input_scrolls_and_follows_its_caret"],
    ),
    (
        "the highlight counts characters as bytes",
        "    text.char_indices()\n        .map(|(b, _)| b)",
        "    text.char_indices()\n        .enumerate()\n        .map(|(i, _)| i)",
        ["the_highlight_covers_what_matched_after_a_wide_character"],
    ),
    # -- the one-line fields ------------------------------------------------------------
    (
        "a press in a field does not move its caret",
        "                Self::place_line_caret(input, rect, x);\n                true",
        "                let _ = (input, rect, x);\n                true",
        ["a_press_in_a_field_gives_it_the_keyboard_and_puts_the_caret_there"],
    ),
    (
        "copy does not reach the other fields",
        "        Key::C if ctrl => {\n            if input.has_selection() {\n"
        "                copied = Some(input.selected_text().to_string());",
        "        Key::C if ctrl => {\n            if input.has_selection() {\n"
        "                copied = None;",
        ["copy_and_paste_carry_text_between_the_fields"],
    ),
    (
        "a chord nobody bound types its letter",
        "        _ => {\n            if key.text.is_empty() || ctrl {\n                return LineEdit {",
        "        _ => {\n            if key.text.is_empty() {\n                return LineEdit {",
        ["a_chord_nobody_bound_types_nothing"],
    ),
    (
        "a keystroke into a full field still redraws",
        "                edited.handled && (typed || moved || copied)",
        "                edited.handled",
        ["a_field_stops_accepting_at_its_limit"],
    ),
    # -- matches --------------------------------------------------------------------------
    (
        "F3 does nothing",
        "        if key.key == Key::F3 {",
        "        if key.key == Key::F3 && false {",
        ["stepping_through_matches_brings_the_current_one_into_both_views"],
    ),
    (
        "Enter in the pattern does not step",
        "                    Key::Down | Key::Enter => {",
        "                    Key::Down => {",
        ["enter_and_the_arrows_step_through_matches_from_the_pattern"],
    ),
    (
        "the match list does not follow the current match",
        "        *slot = keep_in_view(*slot, current, rows);",
        "        let _ = (slot, current, rows);",
        ["stepping_through_matches_brings_the_current_one_into_both_views"],
    ),
    (
        "the test input does not follow the current match",
        "            let line = self.input.line_index(byte);\n"
        "            self.input_scroll = keep_in_view(self.input_scroll, line, self.input_rows());",
        "            let _ = byte;",
        ["stepping_through_matches_brings_the_current_one_into_both_views"],
    ),
    (
        "a match row records no hit box",
        "            f.hit(Target::MatchRow(mi), row);",
        "",
        ["the_match_buttons_and_rows_step_through_the_matches"],
    ),
    (
        "a match of a line break is drawn without it",
        """        .replace('\\n', "\\\\n")""",
        "",
        ["a_match_that_spans_lines_is_shown_with_its_break"],
    ),
    # -- the results panel ---------------------------------------------------------------
    (
        "the sub-tabs are drawn with nothing behind them",
        "                self.results_view = view;\n                true",
        "                let _ = view;\n                true",
        ["the_sub_tabs_show_what_they_name"],
    ),
    (
        "the breakdown stops at a screenful",
        "            .explanations\n            .iter()\n            .enumerate()\n            .skip(scroll)",
        "            .explanations\n            .iter()\n            .enumerate()\n            .skip(0)",
        ["a_long_explanation_can_be_read_to_its_end"],
    ),
    (
        "the groups button does nothing",
        "            Target::GroupsInline => {\n                self.show_groups = !self.show_groups;",
        "            Target::GroupsInline => {\n                let _ = self.show_groups;",
        ["the_groups_button_shows_and_hides_the_groups_in_the_list"],
    ),
    (
        "nothing scrolls",
        "        *slot = slot.saturating_add_signed(step).min(limit);",
        "        *slot = (*slot).min(limit);",
        [
            "the_chips_filter_the_library_and_the_list_scrolls_to_its_end",
            "a_long_explanation_can_be_read_to_its_end",
            "a_long_result_scrolls",
            "the_reference_scrolls_in_a_short_window",
        ],
    ),
    # -- the library ------------------------------------------------------------------------
    (
        "a library pattern cannot be used",
        "        self.pattern.set_text(&pattern);",
        "        let _ = &pattern;",
        ["a_library_pattern_can_be_used_by_its_button_by_a_double_press_or_by_enter"],
    ),
    (
        "a double press on a library row does nothing",
        "                    Some(Target::LibraryRow(i)) => self.use_library_entry(i),",
        "                    Some(Target::LibraryRow(_)) => false,",
        ["a_library_pattern_can_be_used_by_its_button_by_a_double_press_or_by_enter"],
    ),
    (
        "Enter in the library does nothing",
        "            Key::Enter => self\n                .selected_library_entry\n"
        "                .is_some_and(|i| self.use_library_entry(i)),",
        "            Key::Enter => false,",
        ["a_library_pattern_can_be_used_by_its_button_by_a_double_press_or_by_enter"],
    ),
    (
        "Delete in the library does nothing",
        "            Key::Delete => self\n                .selected_library_entry\n"
        "                .is_some_and(|i| self.delete_library_entry(i)),",
        "            Key::Delete => false,",
        ["a_saved_pattern_can_be_deleted_and_stays_deleted"],
    ),
    (
        "a saved pattern is not kept",
        '        doc.set_str(&[LIBRARY_KEY, &name, "pattern"], self.pattern.text());',
        "",
        ["a_pattern_saved_to_the_library_is_kept_with_its_flags"],
    ),
    (
        "a saved pattern loses its flags",
        '        doc.set_str(&[LIBRARY_KEY, &name, "flags"], &self.flags.letters());',
        "",
        ["a_pattern_saved_to_the_library_is_kept_with_its_flags"],
    ),
    (
        "using a saved pattern leaves the flags as they were",
        "            self.flags = flags;",
        "            let _ = flags;",
        ["a_pattern_saved_to_the_library_is_kept_with_its_flags"],
    ),
    (
        "saving a name again adds a second entry",
        "            .position(|e| e.category == PatternCategory::Custom && e.name == name);",
        "            .position(|_| false);",
        ["saving_under_a_saved_name_replaces_it"],
    ),
    (
        "a deleted pattern comes back",
        "        doc.remove(&[LIBRARY_KEY, &name]);",
        "",
        ["a_saved_pattern_can_be_deleted_and_stays_deleted"],
    ),
    (
        "the library has no limit",
        "        if existing.is_none() && self.library.len() >= MAX_LIBRARY_ENTRIES {",
        "        if false {",
        ["the_library_says_when_it_is_full"],
    ),
    (
        # The backdrop is the dialog's whole modality: a guard in `activate`
        # said the same, survived this row's first form, and was deleted.
        "the save dialog lets presses through",
        "        f.hit(\n            Target::ModalBackdrop,\n"
        "            Rect::new(0.0, 0.0, self.window_width, self.window_height),\n        );",
        "",
        ["the_save_dialog_is_modal_and_cancel_saves_nothing"],
    ),
    (
        "typing on the library tab types into the pattern",
        "            ActiveTab::Library => self.handle_library_key(key),",
        "            ActiveTab::Library => self.handle_tester_key(key),",
        ["typing_in_the_library_or_the_reference_goes_nowhere"],
    ),
    (
        "a library chip records no hit box",
        "            f.hit(Target::Chip(ci), rect);",
        "",
        ["the_chips_filter_the_library_and_the_list_scrolls_to_its_end"],
    ),
    # -- the window ----------------------------------------------------------------------
    (
        "a tab is drawn and records no hit box",
        "            f.hit(Target::Tab(tab), rect);",
        "",
        ["the_tabs_are_buttons"],
    ),
    (
        "a flag button is drawn and records no hit box",
        "            f.hit(Target::Flag(*flag), rect);",
        "",
        ["the_flag_buttons_toggle_their_flags"],
    ),
    (
        "the pointer lights nothing",
        "                if over != self.hover {\n                    self.hover = over;",
        "                if over != self.hover {\n                    let _ = over;",
        ["the_pointer_lights_what_it_is_over_and_the_status_bar_says_what_it_does"],
    ),
    (
        # The card's hit box is its whole modality: a second rule in
        # `handle_mouse` said the same, survived this row's first form, and
        # was deleted.
        "a press goes through the shortcut card",
        "            f.hit(\n                Target::HelpCard,\n"
        "                Rect::new(0.0, 0.0, self.window_width, self.window_height),\n            );",
        "",
        ["a_press_puts_the_shortcut_card_away_and_reaches_nothing_under_it"],
    ),
    (
        "a press on the shortcut card leaves it up",
        "            Target::HelpCard => {\n                self.show_help = false;",
        "            Target::HelpCard => {\n                let _ = self.show_help;",
        ["a_press_puts_the_shortcut_card_away_and_reaches_nothing_under_it"],
    ),
    (
        "the toolbar is laid out at the size the window opened at",
        "        let mut right = self.window_width - PADDING;",
        "        let mut right = WINDOW_WIDTH - PADDING;",
        ["the_toolbar_is_laid_out_at_the_size_it_is_given"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "regextester", timeout=900, only=only))
