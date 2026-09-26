"""Mutation test for the text editor's close: asking before unsaved work is
lost, with a tab's close or the window's.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Closing a modified tab was refused with a status message offering
Ctrl+Shift+W to discard -- a key nothing bound -- and closing the window went
at once whatever it held.  Both ask now, and the window stays open while they
do (`Response::KeepOpen`, which lane F added for this).

The table covers the close only; the rest of the editor's suite predates it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

MAIN_SRC = Path(__file__).parent / "src" / "main.rs"
INPUT_SRC = Path(__file__).parent / "src" / "input.rs"

# (name, old, new, [tests that must fail])
MAIN_MUTATIONS = [
    (
        "the window closes over unsaved work",
        "        if self.tabs.iter().any(|d| d.modified) {",
        "        if false {",
        ["closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"],
    ),
    (
        "Save on close saves nothing",
        "            (CloseScope::Window, Choice::Save) => self.continue_quitting(),",
        "            (CloseScope::Window, Choice::Save) => self.quit = true,",
        ["saving_on_close_writes_what_has_a_file_and_asks_where_for_the_rest"],
    ),
    (
        "an untitled document is not asked about",
        "                && doc.path.is_some()\n",
        "",
        ["saving_on_close_writes_what_has_a_file_and_asks_where_for_the_rest"],
    ),
]

INPUT_MUTATIONS = [
    (
        "the question is drawn into a window the loop has closed",
        "                    Response::KeepOpen",
        "                    Response::Redraw",
        ["closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"],
    ),
    (
        "keys go to the document under the question",
        "            return self.question_event(&Event::Key(key.clone()));",
        "            let _ = self.question_event(&Event::Key(key.clone()));",
        ["closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept"],
    ),
    (
        "the question's buttons take no click",
        "            return self.question_event(&Event::Mouse(mouse.clone()));",
        "            let _ = mouse;",
        ["the_questions_buttons_answer_the_pointer"],
    ),
    (
        "Ctrl+W closes a modified tab without asking",
        "    fn close_active_tab(&mut self) {\n        let idx = self.tabs.active_index();\n        self.request_close_tab(idx);",
        "    fn close_active_tab(&mut self) {\n        self.tabs.close_active();",
        ["closing_a_modified_tab_asks_first"],
    ),
    (
        "a modified tab's close box closes it",
        "                self.request_close_tab(index);",
        "                self.tabs.set_active(index);\n                self.tabs.close_active();",
        ["a_modified_tabs_close_box_asks_first"],
    ),
    (
        "the last answer from the dialog does not close the window",
        "            return if self.quit { Response::Exit } else { response };",
        "            return response;",
        ["saving_on_close_writes_what_has_a_file_and_asks_where_for_the_rest"],
    ),
    (
        "saving the untitled document does not carry on closing",
        "                        self.continue_quitting();",
        "",
        ["saving_on_close_writes_what_has_a_file_and_asks_where_for_the_rest"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    results = [
        sweep(MAIN_SRC, MAIN_MUTATIONS, "editor", timeout=900, only=only),
        sweep(INPUT_SRC, INPUT_MUTATIONS, "editor", timeout=900, only=only),
    ]
    raise SystemExit(max(results))
