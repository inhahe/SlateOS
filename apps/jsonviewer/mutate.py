"""Mutation test for the JSON viewer's saving, its clickable chrome, and asking
before unsaved work is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It could not save at all: the tree changed values and deleted nodes and marked
the tab modified, and every change went when the window closed -- which it did
on any close request, without asking.  Its toolbar's buttons and its tabs'
close marks were drawn and answered nothing.  The table covers what was added
for that; the rest of the suite predates it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

CLOSE = "closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"
LAYOUT = "a_tree_edit_keeps_the_texts_layout"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "saving writes nothing",
        "        match safeio::write_atomically(&path, doc.input.as_bytes()) {",
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
        "                doc.whole_len = None;\n",
        "",
        ["a_file_read_only_in_part_is_never_saved_over"],
    ),
    (
        "a tree edit pretty-prints a one-line file",
        r"        let one_line = !self.input.trim().contains('\n');",
        "        let one_line = false;",
        ["a_one_line_file_is_saved_on_one_line", LAYOUT],
    ),
    (
        "a tree edit re-indents the file at two spaces",
        "                IndentStyle::detect(&self.input).unwrap_or(IndentStyle::Spaces2),",
        "                IndentStyle::Spaces2,",
        ["an_edited_file_is_saved_back_to_itself", LAYOUT],
    ),
    (
        "a tree edit drops the final newline",
        r"            (true, false) => text.push('\n'),",
        "            (true, false) => {}",
        [LAYOUT],
    ),
    (
        "a tree edit adds a final newline the text lacked",
        "                text.pop();\n",
        "",
        [LAYOUT],
    ),
    (
        "Ctrl+W closes a modified tab without asking",
        "                    self.request_close_tab(self.active_tab);",
        "                    self.close_tab(self.active_tab);",
        ["a_tab_with_unsaved_changes_asks_before_it_closes"],
    ),
    (
        "the window closes over unsaved work",
        "        if self.documents.iter().any(|d| d.dirty) {",
        "        if false {",
        [CLOSE],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "            } else {\n                Response::KeepOpen\n            };",
        "            } else {\n                Response::Redraw\n            };",
        [CLOSE],
    ),
    (
        "keys edit the document under the question",
        "        if self.close_prompt.is_some() {\n            let typed = text.map",
        "        if false {\n            let typed = text.map",
        [CLOSE],
    ),
    (
        "the question's buttons take no click",
        "        if self.close_prompt.is_some() {\n            if button == MouseButton::Left",
        "        if false {\n            if button == MouseButton::Left",
        [CLOSE],
    ),
    (
        "saving the untitled document does not carry on closing",
        "                Ok(_) => self.continue_quitting(),",
        "                Ok(_) => {}",
        ["saving_on_close_writes_the_files_and_asks_where_for_the_untitled"],
    ),
    (
        "a value being typed is dropped when the window closes",
        "    fn request_quit(&mut self) -> bool {\n"
        "        if self.editing_path.is_some() {\n"
        "            self.commit_edit();\n"
        "        }\n",
        "    fn request_quit(&mut self) -> bool {\n",
        ["a_value_being_typed_when_the_window_closes_is_asked_about"],
    ),
    (
        "Ctrl+S saves without the value being typed",
        "        // A value half typed is part of what the user means to save.\n"
        "        if self.editing_path.is_some() {\n"
        "            self.commit_edit();\n"
        "        }\n",
        "",
        ["saving_keeps_the_value_being_typed"],
    ),
    (
        "Save As starts in $HOME",
        "            .map_or_else(FilePicker::default_start, Path::to_path_buf);",
        "            .map_or_else(FilePicker::default_start, |_| FilePicker::default_start());",
        ["save_as_starts_beside_the_documents_own_file"],
    ),
    (
        "the toolbar answers nothing",
        "                self.toolbar(action);",
        "                let _ = action;",
        ["every_toolbar_button_does_what_it_says"],
    ),
    (
        "a tab's close mark only selects the tab",
        "                self.request_close_tab(i);\n            } else {",
        "                self.switch_to(i);\n            } else {",
        ["a_tabs_close_mark_closes_that_tab_and_the_active_one_stays"],
    ),
    (
        "closing a tab before the active one moves the selection",
        "        if index < self.active_tab {\n            self.active_tab -= 1;",
        "        if false {\n            self.active_tab -= 1;",
        ["a_tabs_close_mark_closes_that_tab_and_the_active_one_stays"],
    ),
    (
        "a click on the find bar goes through to the tree",
        "        if self.search_visible\n            && (content_y..content_y + SEARCH_BAR_HEIGHT).contains(&y)",
        "        if false\n            && (content_y..content_y + SEARCH_BAR_HEIGHT).contains(&y)",
        ["the_find_bar_takes_its_own_clicks"],
    ),
    (
        "a value being typed follows the user into the next tab",
        "        if self.editing_path.is_some() {\n"
        "            self.commit_edit();\n"
        "        }\n"
        "        self.active_tab = index;",
        "        self.active_tab = index;",
        ["a_value_being_typed_stays_with_its_own_tab"],
    ),
    (
        "an empty tab shows the last tab's matches",
        "            _ => {\n"
        "                self.search_results.clear();\n"
        "                self.search_index = 0;",
        "            _ => {\n"
        "                self.search_index = 0;",
        ["the_matches_belong_to_the_document_on_screen"],
    ),
    (
        "the redraw test cannot see a tab close",
        "            tabs: self\n"
        "                .documents\n"
        "                .iter()\n"
        "                .map(|d| (d.title.clone(), d.dirty))\n"
        "                .collect(),",
        "            tabs: Vec::new(),",
        ["closing_a_tab_is_a_redraw_even_when_the_next_looks_the_same"],
    ),
    (
        "the redraw test cannot see the status line",
        "            note: self.note.clone(),",
        "            note: None,",
        ["an_edited_file_is_saved_back_to_itself"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "jsonviewer", timeout=900, only=only))
