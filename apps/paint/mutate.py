"""Mutation test for paint's record of unsaved changes, its saving, and the
question before a picture is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Nothing recorded whether the picture had changed, so nothing could ask: the
window closed over it, and Ctrl+N and Ctrl+O replaced it, without a word.
Ctrl+S asked where every time, and what a save did was drawn nowhere, so a
failed one looked like one that worked.  The table covers the repair; the rest
of the suite predates it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

MARKS = "an_edit_marks_the_picture_and_a_save_clears_it"
CLOSE = "closing_over_unsaved_changes_asks_and_each_answer_is_kept"
REPLACE = "new_and_open_ask_before_replacing_the_picture"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "an edit does not mark the picture",
        "        self.history.push(snapshot);\n        self.dirty = true;\n",
        "        self.history.push(snapshot);\n",
        [MARKS, CLOSE],
    ),
    (
        "an undo does not mark the picture",
        "            self.active_layer = prev.active_layer;\n            self.dirty = true;\n",
        "            self.active_layer = prev.active_layer;\n",
        [MARKS],
    ),
    (
        "a save leaves the picture marked",
        "                self.document_path = Some(path.to_path_buf());\n"
        "                self.dirty = false;\n"
        '                format!("Saved {}", path.display())',
        "                self.document_path = Some(path.to_path_buf());\n"
        '                format!("Saved {}", path.display())',
        [MARKS],
    ),
    (
        "Ctrl+S asks where every time",
        "            Some(path) => self.file_status = Some(self.save_file(&path)),",
        "            Some(_) => self.ask_where_to_save(PickerFor::Save),",
        [MARKS],
    ),
    (
        "the window closes over unsaved changes",
        "            if !self.dirty {\n                return Response::Exit;\n            }",
        "            if true {\n                return Response::Exit;\n            }",
        [CLOSE],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "            self.unless_unsaved(Pending::Close);\n            return Response::KeepOpen;",
        "            self.unless_unsaved(Pending::Close);\n            return Response::Redraw;",
        [CLOSE],
    ),
    (
        "keys reach the picture under the question",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && false",
        [CLOSE],
    ),
    (
        "saving on close does not close",
        "                    self.file_status = Some(self.save_file(&path));\n"
        "                    if !self.dirty {\n"
        "                        self.go_on(pending);\n"
        "                    }",
        "                    self.file_status = Some(self.save_file(&path));",
        [CLOSE],
    ),
    (
        "a picture saved where the picker said does not go on",
        "                let said = self.save_file(path);\n"
        "                if !self.dirty {\n"
        "                    self.go_on(pending);\n"
        "                }",
        "                let said = self.save_file(path);",
        ["an_untitled_picture_is_saved_where_the_picker_says_before_closing"],
    ),
    (
        "Ctrl+N replaces the picture without asking",
        "                    self.unless_unsaved(Pending::New);",
        "                    self.go_on(Pending::New);",
        [REPLACE],
    ),
    (
        "Ctrl+O opens over unsaved changes",
        "                    self.unless_unsaved(Pending::Open);",
        "                    self.go_on(Pending::Open);",
        [REPLACE],
    ),
    (
        "a blank canvas counts as unsaved",
        "                // is still one undo away, and undoing marks it again.\n"
        "                self.dirty = false;\n",
        "                // is still one undo away, and undoing marks it again.\n",
        [REPLACE],
    ),
    (
        "what a save did is drawn nowhere",
        "        if let Some(status) = &self.file_status {",
        "        if let Some(status) = None::<&String> {",
        ["the_status_bar_says_what_the_last_save_did"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "paint", timeout=900, only=only))
