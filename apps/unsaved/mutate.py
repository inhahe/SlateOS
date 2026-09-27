"""Mutation test for the unsaved-changes question.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Every document application asks this question through here, so a fault in it
is a fault in all of them at once: an answer mapped to the wrong choice loses
someone's work in every editor.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

KEYS = "each_key_gives_its_answer"
BUTTONS = "each_button_gives_its_answer_where_it_is_drawn"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "S answers nothing",
        "                (Key::S, _) | (_, Some('s')) => return Some(Choice::Save),\n",
        "",
        [KEYS],
    ),
    (
        "D answers nothing",
        "                (Key::D, _) | (_, Some('d')) => return Some(Choice::Discard),\n",
        "",
        [KEYS],
    ),
    (
        "Save's button throws the changes away",
        "            DialogResult::Yes => Choice::Save,",
        "            DialogResult::Yes => Choice::Discard,",
        [KEYS, BUTTONS],
    ),
    (
        "Don't save's button keeps the document open",
        "            DialogResult::No => Choice::Discard,",
        "            DialogResult::No => Choice::Cancel,",
        [BUTTONS, "tab_moves_between_the_answers"],
    ),
    (
        "a stray click takes the question away",
        "            .with_click_outside_dismiss(false);",
        "            .with_click_outside_dismiss(true);",
        ["a_click_outside_answers_nothing"],
    ),
    (
        "the question fades in from nothing, with no clock to finish it",
        "        dialog.tick(FADE_DONE_MS);\n",
        "",
        ["it_is_drawn_at_once"],
    ),
    (
        "several names read as the first",
        "        [one] => format!(\"{one} has changes that are not saved.\"),\n"
        "        [init @ .., last] => format!(",
        "        [one, ..] => format!(\"{one} has changes that are not saved.\"),\n"
        "        [init @ .., last] if false => format!(",
        ["the_message_names_whose_changes"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "unsaved", timeout=600, only=only))
