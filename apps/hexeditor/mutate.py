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
        "            PickerPurpose::SaveAs => {\n                let idx = self.active_tab;",
        "            PickerPurpose::SaveAs if false => {\n                let idx = self.active_tab;",
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
        "        if key.modifiers.ctrl && !key.modifiers.alt {",
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

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "hexeditor", timeout=900, only=only))
