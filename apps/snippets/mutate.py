"""Mutation test for the snippet editor, the library kept on disk, and asking
before a snippet -- or the library's latest changes -- is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Nothing could change a snippet: a new one was named from the search box and
had no content, and nothing could give it a line of code, a tag or a
description.  Delete deleted without asking, and the library was gone when the
window closed.  The table covers what replaced that; the rest of the suite
predates it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "the_kept_library_reads_back_what_it_wrote_whatever_the_text"
REFUSED = "a_library_that_cannot_be_read_whole_is_refused_and_says_why"
KEPT = "a_snippet_is_written_in_the_editor_and_is_there_next_time"
QUIET = "a_window_made_by_new_keeps_nothing"
DISCARD = "leaving_the_editor_over_changes_asks_and_enter_throws_them_away"
TITLE = "a_snippet_needs_a_title"
INDENT_T = "tab_indents_the_code_and_shift_tab_leaves_it"
MODAL = "the_editor_takes_every_key_and_press_while_it_is_up"
PRESSES = "the_editors_fields_answer_presses"
DRAWN = "the_code_is_drawn_in_the_editor_with_the_rest_of_the_snippet"
CLOSE_EDIT = "closing_while_editing_with_changes_asks_and_save_keeps_them"
FAILING = "closing_while_a_save_fails_asks_first"
BROKEN = "a_library_file_that_cannot_be_read_is_left_as_it_is"
BIG = "a_library_file_too_big_to_read_whole_is_refused"
STAMP = "a_new_snippet_is_stamped_by_the_clock_and_later_than_the_last"
COUNTED = "every_change_is_counted_and_looking_is_not"
DELETE_KEY = "delete_deletes_the_selected_snippet"
DELETE_CLICK = "clicking_delete_removes_the_selected_snippet_and_only_that_one"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ---- every change counted ----
    (
        "a saved edit is not counted",
        "        self.export_note = Some(Ok(note));\n        self.changed();\n",
        "        self.export_note = Some(Ok(note));\n",
        [COUNTED, KEPT],
    ),
    (
        "a favourite is not counted",
        "            snippet.favorite = !snippet.favorite;\n            self.changed();\n",
        "            snippet.favorite = !snippet.favorite;\n",
        [COUNTED],
    ),
    (
        "a use is not counted",
        "        snippet.use_count = snippet.use_count.saturating_add(1);\n        self.changed();\n",
        "        snippet.use_count = snippet.use_count.saturating_add(1);\n",
        [COUNTED],
    ),
    (
        "a folder opened or shut is not counted",
        "                    folder.expanded = !folder.expanded;\n                    self.changed();\n",
        "                    folder.expanded = !folder.expanded;\n",
        [COUNTED],
    ),
    (
        "a delete is not counted",
        "    fn delete_snippet(&mut self, id: SnippetId) {\n        self.changed();\n",
        "    fn delete_snippet(&mut self, id: SnippetId) {\n",
        [COUNTED],
    ),
    # ---- the file ----
    (
        "the file writes the code unescaped",
        "            tsv::escape(&sn.content),",
        "            sn.content.clone(),",
        [ROUND],
    ),
    (
        "the file loses the tags",
        '            out.push_str("tag\\t");',
        '            out.push_str("gone\\t");',
        [ROUND],
    ),
    (
        "the file loses the recently used",
        '        out.push_str("recent\\t");',
        '        out.push_str("gone\\t");',
        [ROUND],
    ),
    (
        "two snippets with one number are read",
        "                if lib.snippets.iter().any(|sn| sn.id == id) {",
        "                if false {",
        [ROUND, REFUSED],
    ),
    (
        "a folder may hang from nothing",
        "                        if !lib.folders.iter().any(|f| f.id == p) {",
        "                        if false {",
        [REFUSED],
    ),
    (
        "a snippet may sit in no folder that is there",
        "                        if !lib.folders.iter().any(|x| x.id == f) {",
        "                        if false {",
        [REFUSED],
    ),
    (
        "a use of no snippet is read",
        "                if !lib.snippets.iter().any(|sn| sn.id == id) {",
        "                if false {",
        [REFUSED],
    ),
    # ---- keeping it ----
    (
        "nothing is written",
        "        if !self.persist || self.revision == self.kept_revision {",
        "        if true {",
        [KEPT],
    ),
    (
        "a window made by new keeps its library",
        "        if !self.persist || self.revision == self.kept_revision {",
        "        if self.revision == self.kept_revision {",
        [QUIET],
    ),
    (
        "the library is not kept after an event",
        "    // Kept after every event that changed it.\n    app.keep();\n",
        "",
        [KEPT],
    ),
    (
        "Save closes although the save failed",
        "                self.quit = !self.unkept();",
        "                self.quit = true;",
        [FAILING],
    ),
    (
        "the close question is not drawn",
        "            question.render(&palette, width, height, &mut tree);\n",
        "",
        [CLOSE_EDIT],
    ),
    (
        "a snippet is stamped with its number",
        "        let at = clock_ms().max(self.last_stamp.saturating_add(1));",
        "        let at = self.last_stamp.saturating_add(1);",
        [STAMP],
    ),
    # ---- the editor ----
    (
        "a snippet needs no title",
        "        let title = ed.title.text().trim().to_owned();\n        if title.is_empty() {",
        "        let title = ed.title.text().trim().to_owned();\n        if false {",
        [TITLE],
    ),
    (
        "a tag is kept twice",
        "            if !tag.is_empty() && !out.iter().any(|t| t == tag) {",
        "            if !tag.is_empty() {",
        [KEPT],
    ),
    (
        "leaving over changes does not ask",
        "            ed.confirm_discard = true;",
        "            self.editing = None;",
        [DISCARD],
    ),
    (
        "Enter keeps the changes it was asked to throw away",
        "                Key::Enter => self.editing = None,",
        "                Key::Enter => ed.confirm_discard = false,",
        [DISCARD],
    ),
    (
        "Tab in the code leaves it",
        "                ed.content.insert(INDENT, MAX_CONTENT_LEN);",
        "                ed.field = SnippetField::Title;",
        [INDENT_T, KEPT],
    ),
    (
        "the editor's keys reach the library",
        "        if self.editing.is_some() {\n            return self.handle_edit_key(ev);\n        }",
        "",
        [MODAL, INDENT_T],
    ),
    (
        "a press reaches the library under the editor",
        "        if self.editing.is_some() {\n            return match ev.kind {",
        "        if false {\n            return match ev.kind {",
        [MODAL, PRESSES],
    ),
    (
        "a press in the code puts no caret",
        "                    ed.content.click(line, (x - code.x).max(0.0), false);",
        "                    let _ = line;",
        [PRESSES],
    ),
    (
        "New does not open the editor",
        "                if let Some(id) = self.selected_snippet_id {\n                    self.edit(id);\n                }",
        "",
        [KEPT],
    ),
    (
        "F2 opens nothing",
        "            Key::F2 => self.press(Target::Edit),\n",
        "",
        [DISCARD, TITLE],
    ),
    (
        "closing over an unsaved edit does not ask",
        "                .is_none_or(|sn| ed.differs_from(sn))\n        {",
        "                .is_none_or(|sn| ed.differs_from(sn))\n            && false\n        {",
        [CLOSE_EDIT],
    ),
    # ---- the delete question ----
    (
        "Delete deletes without asking",
        "                self.pending_delete = Some(id);",
        "                self.delete_snippet(id);",
        [DELETE_KEY, DELETE_CLICK],
    ),
    (
        "Escape deletes",
        "                Key::Escape | Key::N => self.pending_delete = None,",
        "                Key::Escape | Key::N => self.confirm_delete(id),",
        [DELETE_KEY],
    ),
    # ---- the status line ----
    (
        "why the library is not kept is not said",
        "        if let Some(error) = &self.store_error {",
        "        if let Some(error) = None::<&String> {",
        [BROKEN, FAILING],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "snippets", timeout=900, only=only))
