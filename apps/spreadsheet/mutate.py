"""Mutation test for the spreadsheet's workbook file, and asking before a
workbook is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Ctrl+S wrote the sheet in front as CSV -- values only, no formula, no format,
no other sheet -- and nothing recorded unsaved changes, so the window closed
over them.  The table covers the workbook file and the question; the rest of
the suite predates them.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "a_workbook_saved_and_opened_again_is_the_same_workbook"
REFUSED = "a_workbook_that_cannot_be_read_whole_is_refused"
KEYS = "ctrl_s_saves_f12_saves_as_and_ctrl_e_exports"
CLOSE = "closing_or_opening_over_unsaved_changes_asks"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "an edit does not mark the workbook",
        "        self.history.record(action);\n"
        "        self.changed = true;\n",
        "        self.history.record(action);\n",
        [KEYS, CLOSE],
    ),
    (
        "freezing panes is not a change",
        "        self.layout_changed = true;\n",
        "",
        ["freezing_panes_is_a_change"],
    ),
    (
        "a save writes nothing",
        "        match safeio::write_str_atomically(path, &workbook_text(&self.sheets)) {",
        "        match Ok::<(), std::io::Error>(()) {",
        [ROUND, KEYS],
    ),
    (
        "the file loses the frozen panes",
        "        if sheet.frozen_rows > 0 || sheet.frozen_cols > 0 {",
        "        if false {",
        [ROUND],
    ),
    (
        "the file loses the column widths",
        '                out.push_str(&format!("width\\t{}\\t{width}\\n", CellAddr::col_letter(col)));',
        "                let _ = (col, width);",
        [ROUND],
    ),
    (
        "the file loses the formats",
        "            for flag in format_flags(&cell.format) {",
        "            for flag in format_flags(&cell.format).into_iter().take(0) {",
        [ROUND],
    ),
    (
        "formulas are not worked out after opening",
        "    for sheet in &mut sheets {\n        recalculate_sheet(sheet);\n    }\n",
        "",
        [ROUND],
    ),
    (
        "a later format is half-read",
        "    if version > WORKBOOK_FORMAT {",
        "    if version > WORKBOOK_FORMAT + 100 {",
        [REFUSED],
    ),
    (
        "a cell written twice is taken twice",
        "                        if !seen.insert(addr) {",
        "                        if !seen.insert(addr) && false {",
        [REFUSED],
    ),
    (
        "a file too big to read whole is read in part",
        "        if read.truncated {",
        "        if false {",
        [REFUSED],
    ),
    (
        "a CSV becomes the workbook's file",
        "        self.document_path = None;\n",
        "",
        ["a_csv_opens_as_a_new_workbook_of_its_own"],
    ),
    (
        "Ctrl+S asks where every time",
        "            Some(path) => self.last_file_action = Some(self.write_workbook(&path)),",
        "            Some(_) => self.ask_where_to_save(PickerFor::Save),",
        [KEYS],
    ),
    (
        "an export counts as a save",
        "            PickerFor::Export => self.write_csv(path),",
        "            PickerFor::Export => self.write_workbook(path),",
        [KEYS],
    ),
    (
        "F12 does nothing",
        "            Key::F12 => {\n                self.ask_where_to_save(PickerFor::Save);",
        "            Key::F12 => {\n                let _ = PickerFor::Save;",
        [KEYS],
    ),
    (
        "a value being typed is dropped when the window closes",
        "        // What is being typed into a cell is part of the workbook.\n"
        "        if matches!(self.mode, InteractionMode::Editing { .. }) {\n"
        "            self.confirm_edit();\n"
        "        }\n",
        "",
        ["a_value_being_typed_when_the_window_closes_is_asked_about"],
    ),
    (
        "the window closes over unsaved changes",
        "            self.confirm_edit();\n        }\n        if !self.dirty() {",
        "            self.confirm_edit();\n        }\n        if true {",
        [CLOSE],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "            } else {\n                Response::KeepOpen\n            };",
        "            } else {\n                Response::Redraw\n            };",
        [CLOSE],
    ),
    (
        "keys reach the sheet under the question",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && false",
        [CLOSE],
    ),
    (
        "Ctrl+O opens over unsaved changes",
        "                    self.unless_unsaved(Pending::Open);",
        "                    self.go_on(Pending::Open);",
        [CLOSE],
    ),
    # 2026-09-28: a new book opened with sample rows in it.
    (
        "a new book opens with the sample rows",
        "    pub fn opened(width: f32, height: f32) -> Self {\n        Self::new(width, height)\n    }",
        "    pub fn opened(width: f32, height: f32) -> Self {\n"
        "        let mut app = Self::new(width, height);\n"
        "        app.seed_sample_content();\n"
        "        app\n"
        "    }",
        ["a_new_book_opens_blank"],
    ),
    # -- the actions as a tree, and the keys (C-Q24, §1416) ----------------------
    (
        "a journey takes its steps the wrong way",
        "                Travel::Undo(action) => self.apply_undo_action(&action, true),",
        "                Travel::Undo(action) => self.apply_undo_action(&action, false),",
        ["an_edit_after_an_undo_keeps_the_undone_one_reachable_with_alt_z"],
    ),
    (
        "Alt+Z goes forward in time",
        "        let steps = self.history.earlier();",
        "        let steps = self.history.later();",
        ["an_edit_after_an_undo_keeps_the_undone_one_reachable_with_alt_z"],
    ),
    (
        "a journey does not mark the workbook",
        "        let steps = self.history.earlier();\n        self.changed |= !steps.is_empty();",
        "        let steps = self.history.earlier();",
        ["an_edit_after_an_undo_keeps_the_undone_one_reachable_with_alt_z"],
    ),
    (
        "Alt+Shift+Z goes back as Alt+Z does",
        "            let moved = if event.modifiers.shift {\n                self.later()",
        "            let moved = if event.modifiers.shift {\n                self.earlier()",
        ["an_edit_after_an_undo_keeps_the_undone_one_reachable_with_alt_z"],
    ),
    (
        "the ends of the history are not said",
        "            if !moved {\n                self.notice = Some(",
        "            if false {\n                self.notice = Some(",
        ["the_ends_of_the_history_are_said"],
    ),
    (
        "AltGr is taken for Ctrl",
        "        if event.modifiers.ctrl && !event.modifiers.alt {",
        "        if event.modifiers.ctrl {",
        ["an_altgr_letter_is_typed_not_taken_for_a_chord"],
    ),
    (
        "Ctrl+Shift+Z undoes",
        "                Key::Z if event.modifiers.shift => {\n                    self.redo();",
        "                Key::Z if event.modifiers.shift => {\n                    self.undo();",
        ["ctrl_shift_z_redoes"],
    ),
    (
        "the history keeps ten actions more",
        "const UNDO_STACK_LIMIT: usize = 200;",
        "const UNDO_STACK_LIMIT: usize = 210;",
        ["test_undo_manager_limit"],
    ),
    (
        "undo on an empty history claims an action",
        "        let action = self.history.undo()?;",
        "        let action = self.history.undo().or(Some(UndoAction::CellEdit {\n"
        "            sheet_idx: 0,\n"
        "            addr: CellAddr::new(0, 0),\n"
        "            old_cell: Cell::empty(),\n"
        "            new_cell: Cell::empty(),\n"
        "        }))?;",
        ["test_undo_manager_limit"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "spreadsheet", timeout=900, only=only))
