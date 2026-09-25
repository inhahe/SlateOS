"""Mutation test for `textarea`, the multi-line text field applications share.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The field began in `apps/regextester`, whose table carried the first three of
these rows until the field moved here.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a newline cannot be typed",
        "            Key::Enter => edited.changed = self.insert(\"\\n\", capacity),",
        "            Key::Enter => {}",
        ["typing_and_line_breaks_go_in_at_the_caret"],
    ),
    (
        "the field is edited only at its end",
        "        self.text.insert_str(self.caret, &taken);",
        "        self.text.push_str(&taken);",
        ["typing_and_line_breaks_go_in_at_the_caret"],
    ),
    (
        "Up and Down lose the column",
        "        let within = text::cursor_at(line, goal, self.font_size, FontWeightHint::Regular).byte;",
        "        let within = 0;",
        ["up_and_down_keep_the_column"],
    ),
    (
        "a paste lets control characters in",
        "            .filter(|c| matches!(c, '\\n' | '\\t') || !c.is_control())",
        "            .filter(|_| true)",
        ["typing_and_line_breaks_go_in_at_the_caret"],
    ),
    (
        "the capacity counts bytes",
        "        let room = capacity.saturating_sub(self.text.chars().count());",
        "        let room = capacity.saturating_sub(self.text.len());",
        ["the_capacity_is_characters"],
    ),
    (
        "a cut leaves the selection",
        "                    edited.changed = self.delete_selection();",
        "                    edited.changed = false;",
        ["a_selection_is_made_copied_cut_and_typed_over"],
    ),
    (
        "a caret can land inside a character",
        "            .find(|i| self.text.is_char_boundary(*i))",
        "            .find(|_| true)",
        ["deletion_takes_whole_characters"],
    ),
    (
        "Tab is taken from the application",
        "            Key::Tab => edited.handled = false,",
        "            Key::Tab => {}",
        ["tab_and_unknown_chords_are_left_to_the_application"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "textarea", timeout=300, only=only))
