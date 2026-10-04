"""Mutation test for sticky notes' saving on the way out.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Ctrl+Q quit without saving, losing what had been typed since the last
autosave; the close button saved and quit whether or not the save worked.  The
table covers the repair; the rest of the suite predates it.

Each note's history is a tree now (C-Q24): an edit after an undo keeps the
undone one as a branch, reached with Alt+Z.  Its rows cover the keys and the
cap, and a title being typed around the history: recorded before the history
moves, and compared, at the next commit, against the title as it was last
recorded or moved to.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

FAILS = "a_close_whose_save_fails_keeps_the_window_once"
TREE = "alt_z_walks_the_active_notes_history"
NOTE_TREE = "a_note_edit_after_an_undo_keeps_the_undone_one_reachable"
ALTGR = "altgr_z_and_windows_alt_z_walk_nothing"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "Ctrl+Q quits without saving",
        "                Key::Q => self.quit_requested(),",
        "                Key::Q => Action::Quit,",
        ["ctrl_q_saves_first"],
    ),
    (
        "the close does not save",
        "            Event::CloseRequested => self.quit_requested(),",
        "            Event::CloseRequested => Action::Quit,",
        ["closing_the_window_saves_first"],
    ),
    (
        "a close whose save failed goes anyway",
        "        if unsaved && !self.quit_despite_failure {",
        "        if false {",
        [FAILS],
    ),
    (
        "a failed save makes the window impossible to close",
        "            self.quit_despite_failure = true;\n",
        "",
        [FAILS],
    ),
    (
        "the declined close reads to the loop as a close",
        "        if matches!(event, Event::CloseRequested) && action != Action::Quit {",
        "        if false {",
        [FAILS],
    ),
    # A note's text is written in the toolkit's multi-line field
    # (requests/c-e-a-multi-line-text-field-for-the-apps-that-edit-text.md).
    (
        "what is written reaches the note only at the end",
        "            KeyEdit::Changed => {\n                self.write_back(id);",
        "            KeyEdit::Changed => {\n                let _ = id;",
        ["typing_into_a_body_line_marks_the_store_dirty", "a_multibyte_note_survives_editing"],
    ),
    (
        "a key the field passes is swallowed",
        "            KeyEdit::Unhandled => None,",
        "            KeyEdit::Unhandled => Some(Action::None),",
        ["ctrl_z_and_ctrl_y_walk_the_undo_stack"],
    ),
    (
        "Enter does not carry a list on",
        '                    self.body.insert_str(&format!("\\n{}", marker_text(&next)));',
        '                    self.body.insert_str(&format!("\\n{}", marker_text(&LineKind::Plain)));',
        ["enter_carries_a_list_on_and_ends_it_on_an_empty_item"],
    ),
    (
        "an empty item does not end the list",
        "                if text.get(words..end).is_some_and(|w| w.trim().is_empty()) {",
        "                if false {",
        ["enter_carries_a_list_on_and_ends_it_on_an_empty_item"],
    ),
    (
        "a ticked box carries on ticked",
        "                        LineKind::Checkbox { .. } => LineKind::Checkbox { checked: false },",
        "                        LineKind::Checkbox { checked } => LineKind::Checkbox { checked },",
        ["enter_carries_a_list_on_and_ends_it_on_an_empty_item"],
    ),
    (
        "a line grows past the cap",
        "            .all(|line| line.len() <= MAX_LINE_LEN)",
        "            .all(|line| line.len() <= usize::MAX)",
        ["a_line_stops_at_the_cap"],
    ),
    (
        "a paste is not capped",
        "            return (event.key == Key::V).then(|| self.body.clipboard().to_owned());",
        "            return None;",
        ["a_line_stops_at_the_cap"],
    ),
    (
        "Up from the first line stays in the text",
        "            Key::Up if bare && self.body.caret_position(&m).0 == 0 => {",
        "            Key::Up if false => {",
        ["up_from_the_first_line_goes_to_the_title_and_tab_leaves"],
    ),
    (
        "Up from any line goes to the title",
        "            Key::Up if bare && self.body.caret_position(&m).0 == 0 => {",
        "            Key::Up if bare => {",
        ["up_from_the_first_line_goes_to_the_title_and_tab_leaves"],
    ),
    (
        "Tab stays in the note",
        "            Key::Tab if bare => {\n                self.commit_focus();\n                self.focus = None;",
        "            Key::Tab if false => {\n                self.commit_focus();\n                self.focus = None;",
        ["up_from_the_first_line_goes_to_the_title_and_tab_leaves"],
    ),
    (
        "the writing is not one undo step",
        "                note.commit_body(before);",
        "                drop(before);",
        ["ctrl_z_and_ctrl_y_walk_the_undo_stack"],
    ),
    (
        "an untouched note gains an undo step",
        "        if old == self.body {\n            return;\n        }\n        let new = self.body.clone();",
        "        let new = self.body.clone();",
        ["opening_a_note_and_leaving_it_changes_nothing"],
    ),
    (
        "the field is filled again at every key",
        "            .is_some_and(|held| held.id == note.id && held.source == note.body_text())",
        "            .is_some_and(|held| held.id == note.id && held.source == note.body_text() && false)",
        ["ctrl_z_and_ctrl_y_walk_the_undo_stack"],
    ),
    (
        "a click on a line puts the caret at the start",
        "            TextPlace::Line(line, col) => offset_of_line(self.body.text(), line, col),",
        "            TextPlace::Line(..) => 0,",
        ["clicking_a_body_line_puts_the_caret_where_the_click_landed"],
    ),
    (
        "a drag does not select",
        "        self.body.drag_to(cx - r.x, cy - r.y, &m);",
        "        let _ = (r, m, cx, cy);",
        ["a_drag_across_the_text_selects_it"],
    ),
    (
        "a release goes on dragging",
        "        self.body_drag = false;\n        if matches!(self.store.drag_state(), DragState::None) {",
        "        if matches!(self.store.drag_state(), DragState::None) {",
        ["a_drag_across_the_text_selects_it"],
    ),
    (
        "a double click is one click",
        "                            self.press_text(id, mouse.x - canvas.x, mouse.y - canvas.y, 2);",
        "                            self.press_text(id, mouse.x - canvas.x, mouse.y - canvas.y, 1);",
        ["a_double_click_selects_a_word"],
    ),
    (
        "the wheel does not scroll the text",
        "            self.body.scroll_by(wheel::pixels(dy, m.line_height()), &m);",
        "            let _ = (dy, &m);",
        ["the_wheel_scrolls_the_text_being_written"],
    ),
    (
        "the text is not a target",
        "        frame.hit(Target::NoteText(note.id), r);",
        "        let _ = r;",
        ["a_drag_across_the_text_selects_it", "a_double_click_selects_a_word"],
    ),
    (
        "Ctrl+L while writing changes a line the caret is not on",
        "            return self.cycle_line_kind_in_text(id);",
        "            let _ = id;",
        ["the_line_kind_key_cycles_the_line_being_written"],
    ),
    (
        "Ctrl+L moves the caret off its letter",
        "            caret.saturating_sub(len).saturating_add(new.len())",
        "            caret",
        ["the_line_kind_key_cycles_the_line_being_written"],
    ),
    (
        "an undo leaves the writing's start where it was",
        "        if moved {\n            self.refill_body();",
        "        if moved {",
        ["the_toolbar_undo_while_writing_takes_the_writing_back"],
    ),
    (
        "the caret's width setting is ignored",
        "        self.caret_width = settings.caret_width();",
        "        let _ = settings;",
        ["the_caret_is_as_wide_as_the_setting_says"],
    ),
    # -- the note's history: a tree, walked with Alt+Z (C-Q24) ----------------
    (
        "Alt+Z goes nowhere",
        "            } else {\n                self.earlier_active()\n            };",
        "            } else {\n                Action::None\n            };",
        # Not the advertised-keys test: its note is being written, and the
        # text field answers Alt+Z itself.
        [TREE],
    ),
    (
        "Alt+Shift+Z goes back too",
        "                self.later_active()\n            } else {",
        "                self.earlier_active()\n            } else {",
        [TREE],
    ),
    (
        "a journey takes its steps back the wrong way",
        "                Travel::Undo(action) => self.revert(&action),",
        "                Travel::Undo(action) => self.replay(&action),",
        [TREE, NOTE_TREE],
    ),
    (
        "Alt+Z only undoes",
        "        let steps = self.undo_history.earlier();",
        "        let steps: Vec<Travel<EditAction>> =\n"
        "            self.undo_history.undo().map(Travel::Undo).into_iter().collect();",
        [TREE, NOTE_TREE],
    ),
    (
        "Alt+Shift+Z only redoes",
        "        let steps = self.undo_history.later();",
        "        let steps: Vec<Travel<EditAction>> =\n"
        "            self.undo_history.redo().map(Travel::Redo).into_iter().collect();",
        [TREE, NOTE_TREE],
    ),
    (
        "a journey does not mark the notes",
        "            self.clamp_caret();\n            self.store.mark_dirty();",
        "            self.clamp_caret();",
        [TREE],
    ),
    (
        "AltGr+Z goes back",
        "if event.key == Key::Z && m.alt && !m.ctrl && !m.super_key {",
        "if event.key == Key::Z && m.alt && !m.super_key {",
        [ALTGR],
    ),
    (
        "Windows+Alt+Z goes back",
        "if event.key == Key::Z && m.alt && !m.ctrl && !m.super_key {",
        "if event.key == Key::Z && m.alt && !m.ctrl {",
        [ALTGR],
    ),
    (
        "Ctrl+Shift+Z undoes",
        "                Key::Z if m.shift => self.redo_active(),\n",
        "",
        ["ctrl_shift_z_redoes"],
    ),
    (
        "redo undoes",
        "        match self.undo_history.redo() {",
        "        match self.undo_history.undo() {",
        ["ctrl_shift_z_redoes", "a_note_undoes_and_redoes_an_edit"],
    ),
    (
        "a note keeps more than a hundred edits",
        "const UNDO_LIMIT: core::num::NonZeroUsize = match core::num::NonZeroUsize::new(100) {",
        "const UNDO_LIMIT: core::num::NonZeroUsize = match core::num::NonZeroUsize::new(120) {",
        ["a_note_keeps_its_last_hundred_edits"],
    ),
    # -- a title being typed, around the history ------------------------------
    (
        "the history moves before the typing is recorded",
        "    fn step_active(&mut self, step: fn(&mut Note) -> bool) -> Action {\n        self.commit_focus();\n",
        "    fn step_active(&mut self, step: fn(&mut Note) -> bool) -> Action {\n",
        ["redo_keeps_a_title_being_typed", "the_toolbar_undo_while_writing_takes_the_writing_back"],
    ),
    (
        "a commit leaves the title's starting point empty",
        "        self.title_before = note.title.clone();\n        self.store.mark_dirty();",
        "        self.store.mark_dirty();",
        # Not the second-undo test: the undo moves the note, and the title's
        # starting point is taken again from where it moved to.
        ["a_save_while_typing_a_title_keeps_its_starting_point", "redo_keeps_a_title_being_typed"],
    ),
    (
        "a title left as it was loses its starting point",
        "        if note.title == before {\n            self.title_before = before;\n            return;",
        "        if note.title == before {\n            return;",
        ["an_undo_with_nothing_typed_keeps_the_titles_starting_point"],
    ),
    (
        "a move under a title being typed is recorded as an edit",
        "            self.refill_body();\n            self.refill_title();",
        "            self.refill_body();",
        ["leaving_a_title_after_undoing_its_typing_keeps_the_redo"],
    ),
    # No row for "a title types a command's letter": the AltGr branch hands a
    # typed key to the line's editor, and the editor itself refuses Alt's and
    # the Windows key's chords (textline::apply_key). The branch used to ask
    # the same question first, so a row that dropped it was an equivalent
    # mutant (2026-10-04) -- and the question went instead.
    (
        "a title and the search refuse what AltGr types",
        "        if let Some(focus @ (Focus::Title(_) | Focus::Search)) = self.focus\n"
        "            && event.types_text()",
        "        if let Some(focus @ (Focus::Title(_) | Focus::Search)) = self.focus\n"
        "            && false",
        ["a_title_and_the_search_take_altgr_letters_and_no_commands_letter"],
    ),
    (
        "Ctrl held with the Windows key is a chord",
        "        if textline::is_ctrl_chord(m) {",
        "        if m.ctrl && !m.alt {",
        ["ctrl_with_the_windows_key_is_no_chord"],
    ),
]

# The search box is the toolkit's field; it and the titles are edited by
# textline's editor (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
SEARCH = "the_search_box_is_the_toolkits_field_and_edits_like_one"
TITLE = "a_title_edits_like_a_field"
LINE_KIND = "the_toolbar_names_the_kind_of_the_line_the_caret_is_on"

MUTATIONS += [
    (
        "the search box never has the keyboard's mark",
        "                focused: searching,\n                disabled: false,\n",
        "                focused: false,\n                disabled: false,\n",
        [SEARCH],
    ),
    (
        "a query that finds nothing is not red",
        "                invalid: !query.is_empty() && self.store.search_results().is_empty(),\n",
        "                invalid: false,\n",
        [SEARCH],
    ),
    (
        "an empty search box with the keyboard has no caret",
        "                textedit::push_caret(&mut tree, tx, ty, line, self.palette.text, self.caret_width);\n",
        "                let _ = (tx, ty, line);\n",
        [SEARCH],
    ),
    (
        "the search's caret is drawn at its start",
        "                        TextCursor::from(self.line_cursor())\n"
        "                    } else {\n"
        "                        TextCursor::default()\n"
        "                    },\n"
        "                    selection_anchor: if searching {",
        "                        TextCursor::default()\n"
        "                    } else {\n"
        "                        TextCursor::default()\n"
        "                    },\n"
        "                    selection_anchor: if searching {",
        [SEARCH],
    ),
    (
        "a press on the search box puts the caret at the end",
        "                    x - rect.x - SEARCH_TEXT_INSET,\n"
        "                )\n"
        "                .byte;\n",
        "                    f32::MAX,\n"
        "                )\n"
        "                .byte;\n",
        [SEARCH],
    ),
    (
        "a line's editor is not reloaded from the line",
        "        if let Some(text) = self.line_text()\n"
        "            && self.line_editor.text() != text\n"
        "        {\n"
        "            self.line_editor.set_text(&text);\n"
        "        }\n",
        "        let _ = self.line_text();\n",
        ["a_tag_chosen_while_typing_a_search_is_what_the_next_key_edits"],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.line_clipboard = copied;\n",
        "            let _ = copied;\n",
        [SEARCH, TITLE],
    ),
    (
        "the search is not written back",
        "        if let Some(query) = changed {\n            self.store.set_search(&query);\n",
        "        if let Some(query) = changed {\n            let _ = query;\n",
        [SEARCH],
    ),
    (
        "a title is not written back",
        "            if let Some(note) = self.store.get_note_mut(id) {\n                note.title = title;\n            }\n",
        "            let _ = title;\n",
        [TITLE],
    ),
    (
        "a title's Ctrl+A, C, X and V are the board's",
        "                Key::A | Key::C | Key::X | Key::V => match self.focus {\n",
        "                Key::Unknown(0) => match self.focus {\n",
        [SEARCH, TITLE],
    ),
    (
        "a title's selection is not drawn",
        "        if let Some(anchor) = anchor\n            && anchor != col\n        {\n",
        "        if let Some(anchor) = anchor\n            && false\n            && anchor != col\n        {\n",
        [TITLE],
    ),
    (
        "the toolbar names the first line's kind",
        "            Some(Focus::Body(at)) if at == id => {\n",
        "            Some(Focus::Body(at)) if at == id && false => {\n",
        [LINE_KIND],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [SEARCH],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "stickynotes", timeout=900, only=only))
