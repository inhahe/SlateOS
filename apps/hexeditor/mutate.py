"""Mutation test for the hex editor's saving, its clickable chrome, and asking
before unsaved work is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It could not save at all: its toolbar drew a Save button -- and New, Open,
Undo, Redo, Find and GoTo -- that answered nothing, no key saved, and closing
the window threw away every edit without a word.  The table covers what was
added for that, and -- since 2026-09-28 (C-Q24, §1416) -- the history kept
as a tree, reached with Alt+Z and Alt+Shift+Z, Ctrl+Shift+Z and Ctrl+F4, and
AltGr no longer taken for Ctrl; the rest of the suite predates it.

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
    (
        "saving writes nothing",
        "        match safeio::write_atomically(&path, &doc.data) {",
        "        match Ok::<(), std::io::Error>(()) {",
        ["an_edited_file_is_saved_back_to_itself"],
    ),
    (
        "a file read in part is saved over, and cut short",
        "        if let Some(whole) = doc.whole_len {",
        "        if let Some(whole) = None::<usize> {",
        ["a_file_read_only_in_part_is_never_saved_over"],
    ),
    (
        "a file saved in full still counts as read in part",
        "                doc.whole_len = None;",
        "",
        ["a_file_read_only_in_part_is_never_saved_over"],
    ),
    (
        "the picker's choice always opens",
        "            PickerPurpose::Open => self.last_open = Some(self.open_path(path)),\n",
        "            PickerPurpose::Open => self.last_open = Some(self.open_path(path)),\n"
        "            PickerPurpose::SaveAs if true => {\n"
        "                self.last_open = Some(self.open_path(path));\n"
        "            }\n",
        ["an_untitled_document_is_saved_where_the_picker_says"],
    ),
    (
        "the toolbar answers nothing",
        "                self.toolbar(action);",
        "                let _ = action;",
        ["every_toolbar_button_does_what_it_says"],
    ),
    (
        "a tab is not chosen by a click",
        "                    self.active_tab = i;",
        "                    let _ = i;",
        ["a_tab_is_chosen_by_clicking_it"],
    ),
    (
        "the window closes over unsaved work",
        "        if self.documents.iter().any(|d| d.modified) {",
        "        if false {",
        ["closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "            return if self.request_quit() {\n                Response::Exit\n            } else {\n                Response::KeepOpen\n            };",
        "            return if self.request_quit() {\n                Response::Exit\n            } else {\n                Response::Redraw\n            };",
        ["closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"],
    ),
    (
        "keys and clicks reach the file under the question",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && false",
        [
            "closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept",
            "ctrl_w_asks_before_closing_a_modified_tab",
        ],
    ),
    (
        "saving the untitled document does not carry on closing",
        "                Ok(_) => self.continue_quitting(),",
        "                Ok(_) => {}",
        ["saving_on_close_writes_the_files_and_asks_where_for_the_untitled"],
    ),
    (
        "Ctrl+W closes a modified tab without asking",
        "                Key::W if self.focused_panel != FocusedPanel::SearchBar => {\n                    self.request_close_tab(self.active_tab);",
        "                Key::W if self.focused_panel != FocusedPanel::SearchBar => {\n                    self.close_tab(self.active_tab);",
        ["ctrl_w_asks_before_closing_a_modified_tab"],
    ),
    (
        "Ctrl+W in the search bar closes the tab",
        "                Key::W if self.focused_panel != FocusedPanel::SearchBar => {",
        "                Key::W => {",
        ["ctrl_w_decides_whether_the_search_starts_again_at_the_top"],
    ),
    # -- the history as a tree, and the keys (C-Q24, §1416) ----------------------
    (
        "a journey takes its steps the wrong way",
        "                Travel::Undo(entry) => self.revert(&entry),",
        "                Travel::Undo(entry) => self.reapply(&entry),",
        ["a_new_edit_after_an_undo_starts_a_branch_and_keeps_the_undone_one"],
    ),
    (
        "Alt+Z goes forward in time",
        "        let steps = self.history.earlier();",
        "        let steps = self.history.later();",
        ["a_new_edit_after_an_undo_starts_a_branch_and_keeps_the_undone_one"],
    ),
    (
        "a several-byte edit is spliced short",
        "        self.data.splice(start..end, insert.iter().copied());",
        "        self.data.splice(start..end, insert.iter().copied().take(1));",
        ["a_several_byte_edit_goes_back_and_forth_whole"],
    ),
    (
        "Alt+Z is not a key",
        "        if key.key == Key::Z && key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key",
        "        if false && key.key == Key::Z && key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key",
        ["alt_z_reaches_the_branch_an_undo_left"],
    ),
    (
        "Alt+Shift+Z goes back as Alt+Z does",
        "                doc.later();",
        "                doc.earlier();",
        ["alt_z_reaches_the_branch_an_undo_left"],
    ),
    (
        "AltGr is taken for Ctrl",
        "        if textline::is_ctrl_chord(key.modifiers) {",
        "        if key.modifiers.ctrl {",
        ["altgr_z_does_not_undo"],
    ),
    (
        "Ctrl+Shift+Z undoes",
        "                Key::Z if key.modifiers.shift => {\n                    self.active_doc_mut().redo();",
        "                Key::Z if key.modifiers.shift => {\n                    self.active_doc_mut().undo();",
        ["ctrl_shift_z_redoes"],
    ),
    (
        "Ctrl+F4 closes nothing",
        "                Key::F4 => {\n                    self.request_close_tab(self.active_tab);",
        "                Key::F4 => {\n                    let _ = self.active_tab;",
        ["ctrl_f4_closes_the_tab_even_in_the_search_bar"],
    ),
    # -- the shortcut card is modal, for the keys and the pointer
    (
        "a key that is not the card's writes into the file behind it",
        "        if self.show_help {\n"
        "            // Modal: every other key is the card's while it is up. It was\n",
        "        if false {\n"
        "            // Modal: every other key is the card's while it is up. It was\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "a press goes through the card and the wheel scrolls under it",
        "        if self.show_help {\n"
        "            // The card is modal for the pointer as it is for the keys: a\n",
        "        if false {\n"
        "            // The card is modal for the pointer as it is for the keys: a\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "only the left button puts the card away",
        "            if matches!(ev.kind, MouseEventKind::Press(_)) {\n"
        "                self.show_help = false;\n",
        "            if matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {\n"
        "                self.show_help = false;\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
]

CHORD = "a_commands_letter_is_not_written_into_the_file_or_a_box"

MUTATIONS += [
    # A command's letter is not typing (2026-10-04): an unbound chord fell
    # through to the ASCII pane and wrote its letter into the file.
    (
        "a command's letter is written into the file",
        "            if !textline::types_into_field(key) {\n                return false;\n            }\n            let mut wrote_any = false;\n",
        "            let mut wrote_any = false;\n",
        [CHORD],
    ),
    (
        "Windows+A writes a nibble",
        "                if !textline::is_plain(key.modifiers) {\n",
        "                if key.modifiers.ctrl || key.modifiers.alt {\n",
        [CHORD],
    ),
    # No rows for "a command's letter is typed into the search box" or
    # "into the go-to box": since 2026-10-04 the boxes' typing is
    # textline::apply_key's, which tells a command from AltGr itself, in its
    # own crate and with its own tests; CHORD still holds both boxes to it.
    (
        "AltGr+I sets whether case matters",
        "            if key.key == Key::I && textline::is_ctrl_chord(key.modifiers) {\n",
        "            if key.key == Key::I && key.modifiers.ctrl {\n",
        [CHORD],
    ),
    (
        "AltGr+W sets whether the search wraps",
        "            if key.key == Key::W && textline::is_ctrl_chord(key.modifiers) {\n",
        "            if key.key == Key::W && key.modifiers.ctrl {\n",
        [CHORD],
    ),
    (
        "a Windows chord is the window's",
        "        if textline::is_ctrl_chord(key.modifiers) {\n",
        "        if key.modifiers.ctrl && !key.modifiers.alt {\n",
        [CHORD],
    ),
]

FIND = "the_find_bars_box_is_the_toolkits_field"
LAYOUT = "the_find_bars_parts_never_overlap_at_any_width"
PRESS = "a_press_on_a_bar_does_not_reach_the_byte_under_it"
GOTO = "the_go_to_box_is_red_while_it_is_not_an_offset_and_enter_leaves_it_up"
EDITS = "the_find_box_edits_at_a_caret"
GOTO_EDITS = "the_go_to_box_edits_at_a_caret"

MUTATIONS += [
    # The find bar's box and the go-to box are the toolkit's fields; the bar
    # has two rows; a press on either is theirs (2026-10-04; lane C,
    # c-e-a-theme-can-shape-the-controls).
    (
        "the find bar's box never lights",
        "            hovered: open && self.query_hovered,\n",
        "            hovered: false,\n",
        [FIND],
    ),
    (
        "the find bar's box is never marked",
        "            focused: open && self.focused_panel == FocusedPanel::SearchBar,\n",
        "            focused: false,\n",
        [FIND],
    ),
    (
        "the boxes show through the card",
        "        !self.show_help && self.question.is_none() && !self.picker.is_open()\n",
        "        self.question.is_none() && !self.picker.is_open()\n",
        [FIND],
    ),
    (
        "a search that found nothing is not red",
        "            invalid: self.search.match_count == 0\n"
        "                && self.search.searched.as_deref() == Some(self.search.input_text.as_str()),\n",
        "            invalid: false,\n",
        [FIND],
    ),
    (
        "the box is red before its text is searched for",
        "            invalid: self.search.match_count == 0\n"
        "                && self.search.searched.as_deref() == Some(self.search.input_text.as_str()),\n",
        "            invalid: self.search.match_count == 0,\n",
        [FIND],
    ),
    (
        "the light stays after the pointer leaves",
        "        let inside = |r: Rect| ev.kind != MouseEventKind::Leave && r.contains(ev.x, ev.y);\n",
        "        let inside = |r: Rect| r.contains(ev.x, ev.y);\n",
        [FIND],
    ),
    (
        "moving within a box asks for a repaint",
        "        if (query, goto) == (self.query_hovered, self.goto_hovered) {\n            return EventResult::Ignored;\n        }\n",
        "",
        [FIND],
    ),
    (
        "a press on the find bar's box does not give it the keyboard",
        "                if l.query.contains(x, y) {\n                    self.press_box(FocusedPanel::SearchBar, l.query, x);\n                }\n",
        "",
        [FIND, EDITS],
    ),
    (
        "a press on the find bar reaches the byte under it",
        "            if l.bar.contains(x, y) {\n",
        "            if l.query.contains(x, y) {\n",
        [PRESS],
    ),
    (
        "a press on the go-to dialog reaches the byte under it",
        "            if dialog.contains(x, y) {\n",
        "            if input.contains(x, y) {\n",
        [PRESS],
    ),
    (
        "the options line shares the query's row",
        "            y + Self::ROW + 6.0,\n",
        "            y,\n",
        [LAYOUT],
    ),
    (
        "the go-to box is not red for what is not an offset",
        "            invalid: self.goto_text_is_wrong(),\n",
        "            invalid: false,\n",
        [GOTO],
    ),
    (
        "Enter on what is not an offset closes the box",
        "            self.status_message = format!(\"Not an offset: {}\", self.goto_text.trim());\n            return;\n",
        "            self.status_message = format!(\"Not an offset: {}\", self.goto_text.trim());\n            self.goto_visible = false;\n            return;\n",
        [GOTO],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIND, GOTO],
    ),
]

# The boxes edit at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): they
# took typing at their end and Backspace from it, and nothing else; and the
# window's chords took Ctrl+C, V and Home for the file under a box with the
# keyboard.
MUTATIONS += [
    (
        "the window's chords come before the box's",
        "        if let Some(panel) = self.box_with_keyboard()\n            && let Some(result) = self.box_key(panel, key)\n        {\n            return result;\n        }\n\n        // Global shortcuts",
        "        // Global shortcuts",
        [EDITS, CHORD],
    ),
    (
        "a hidden box takes the keys",
        "            FocusedPanel::SearchBar if self.search.visible => Some(FocusedPanel::SearchBar),\n",
        "            FocusedPanel::SearchBar => Some(FocusedPanel::SearchBar),\n",
        [EDITS],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.box_clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "the editor is kept for the box the keyboard moved to",
        "        if self.box_editor_for != Some(panel) || self.box_editor.text() != self.box_text(panel) {\n",
        "        if self.box_editor.text() != self.box_text(panel) {\n",
        [EDITS],
    ),
    (
        "a press puts the caret at the start",
        "            x - r.x - FIELD_INSET,\n",
        "            0.0,\n",
        [GOTO_EDITS],
    ),
    (
        "a press on the go-to box does not place the caret",
        "                    self.press_box(FocusedPanel::GoToDialog, input, x);\n",
        "                    self.focused_panel = FocusedPanel::GoToDialog;\n",
        [GOTO_EDITS],
    ),
    (
        "the caret is drawn at the end",
        "                cursor: self.box_cursor(panel),\n",
        "                cursor: text::TextCursor::from(typed.len()),\n",
        [EDITS],
    ),
]

# Escape closes the box with the keyboard (2026-10-04): it closed the find bar
# first whichever had it, leaving the go-to box opened over the bar up.
MUTATIONS += [
    (
        "Escape closes the find bar ahead of the go-to box with the keyboard",
        "            let goto_first = self.focused_panel == FocusedPanel::GoToDialog;\n",
        "            let goto_first = false;\n",
        ["escape_closes_the_box_with_the_keyboard"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "hexeditor", timeout=900, only=only))
