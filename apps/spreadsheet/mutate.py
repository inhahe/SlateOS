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
        "        self.undo_stack.push(action);\n"
        "        self.redo_stack.clear();\n"
        "        self.changed = true;\n",
        "        self.undo_stack.push(action);\n"
        "        self.redo_stack.clear();\n",
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
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "spreadsheet", timeout=900, only=only))
