"""Mutation test for the whiteboard's undo, its own file, and asking before a
board is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It could export the page in front as an SVG, which nothing read, so a board's
other pages could not be kept at all; nothing recorded unsaved changes, and the
window closed over them.  Undo could not undo a deletion, and replayed one
window-wide history onto whichever page was showing.  The table covers the
repairs; the rest of the suite predates them.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "a_board_saved_and_opened_again_is_the_same_board"
REFUSED = "a_file_that_is_not_a_board_is_refused"
KEYS = "ctrl_s_saves_and_ctrl_e_exports"
CLOSE = "closing_or_opening_over_unsaved_changes_asks"
DELETED = "undo_puts_a_deleted_shape_back_where_it_was"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "undoing a deletion deletes again",
        "            Action::DeleteShape { shape, index } => Action::RestoreShape {",
        "            Action::DeleteShape { shape, index } => Action::DeleteShape {",
        [DELETED],
    ),
    (
        "an undeleted shape goes on top instead of back in its place",
        "                let at = (*index).min(page.shapes.len());\n"
        "                page.shapes.insert(at, shape.clone());",
        "                let _ = index;\n"
        "                page.shapes.push(shape.clone());",
        [DELETED],
    ),
    (
        "a deleted layer comes back empty",
        "                for (i, shape) in shapes {\n"
        "                    let at = (*i).min(page.shapes.len());\n"
        "                    page.shapes.insert(at, shape.clone());\n"
        "                }",
        "                let _ = shapes;",
        ["undo_puts_a_deleted_layer_back_with_its_shapes"],
    ),
    (
        "undo answers from the first page's history whatever page is showing",
        "        if let Some(action) = self.current_page_mut().undo_stack.pop_back() {",
        "        if let Some(action) = self.pages[0].undo_stack.pop_back() {",
        ["undo_acts_only_on_the_page_it_is_about"],
    ),
    (
        "what was left out is not said",
        "                if left_out == 0 {",
        "                if true {",
        ["what_is_not_understood_is_left_out_and_the_rest_read"],
    ),
    (
        "a change does not mark the board",
        "        page.undo_stack.push_back(action);\n        self.dirty = true;\n",
        "        page.undo_stack.push_back(action);\n",
        [KEYS, CLOSE],
    ),
    (
        "a new page is not a change",
        "        self.pages.push(Page::new(name));\n        self.dirty = true;\n",
        "        self.pages.push(Page::new(name));\n",
        [KEYS],
    ),
    (
        "a save writes nothing",
        "        match safeio::write_str_atomically(path, &self.board_document().to_text()) {",
        "        match Ok::<(), std::io::Error>(()) {",
        [ROUND, KEYS],
    ),
    (
        "the file loses a stroke's points",
        '                        doc.set_seq(&at("points"), &pts);\n',
        "",
        [ROUND],
    ),
    (
        "the file loses a note's colour",
        '                        doc.set_str(&at("background"), &colour_hex(*bg_color));\n',
        "",
        [ROUND],
    ),
    (
        "a locked layer comes back unlocked",
        '            layer.locked = doc.get_bool(&at("locked")).unwrap_or(false);',
        "            layer.locked = false;",
        [ROUND],
    ),
    (
        "a later format is half-read",
        "        Some(v) if v > BOARD_FORMAT => {",
        "        Some(v) if v > BOARD_FORMAT + 100 => {",
        [REFUSED],
    ),
    (
        "clashing shape ids are taken as they come",
        "            if !shape_ids.insert(id) {",
        "            if !shape_ids.insert(id) && false {",
        [REFUSED],
    ),
    (
        "a file too big to read whole is read in part",
        "        if read.truncated {",
        "        if false {",
        [REFUSED],
    ),
    (
        "new shapes take the ids of opened ones",
        "        page.next_shape_id = shape_ids.iter().max().map_or(1, |m| m.saturating_add(1));",
        "        page.next_shape_id = 1;",
        ["what_is_not_understood_is_left_out_and_the_rest_read"],
    ),
    (
        "Ctrl+S asks where every time",
        "            Some(path) => self.status_message = Some(self.write_board(&path)),",
        "            Some(_) => self.ask_where_to_save(PickerFor::Save),",
        [KEYS],
    ),
    (
        "an export counts as a save",
        "            PickerFor::Export => self.write_svg(path),",
        "            PickerFor::Export => self.write_board(path),",
        [KEYS],
    ),
    (
        "the window closes over unsaved changes",
        "    fn request_close(&mut self) -> bool {\n        if !self.dirty {",
        "    fn request_close(&mut self) -> bool {\n        if true {",
        [CLOSE],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "                } else {\n                    Response::KeepOpen\n                }",
        "                } else {\n                    Response::Redraw\n                }",
        [CLOSE],
    ),
    (
        "keys reach the canvas under the question",
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
    (
        "what a save did is drawn nowhere",
        "        if let Some(note) = &self.status_message {",
        "        if let Some(note) = None::<&String> {",
        ["the_status_bar_says_what_the_last_save_did"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "whiteboard", timeout=900, only=only))
