"""Mutation test for a program's text-field menu.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

CHOSEN = "a_row_chosen_is_handed_back_with_its_field"
PRESSES = "presses_on_a_dimmed_row_outside_and_with_the_right_button"
KEYS = "the_keys_are_the_menus_while_it_is_up"
DRAWN = "it_is_drawn_while_up_and_takes_the_pointer"
CLOCK = "a_dimmed_row_says_why_on_the_clock"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a chosen row leaves the menu up",
        "                        Some(id) => {\n                            self.up = None;\n",
        "                        Some(id) => {\n",
        [CHOSEN],
    ),
    (
        "a press on a dimmed row puts the menu away",
        "                            if !menu.is_visible() {\n                                self.up = None;\n                            }\n",
        "                            self.up = None;\n",
        [PRESSES],
    ),
    (
        "a press outside leaves the menu up",
        "                            if !menu.is_visible() {\n                                self.up = None;\n                            }\n",
        "",
        [PRESSES],
    ),
    (
        "a right press outside asks for no other menu",
        "                        Outcome::RightPressElsewhere(event.x, event.y)\n",
        "                        Outcome::Taken\n",
        [PRESSES],
    ),
    (
        "a right press on the menu puts it away",
        "                    _ if inside => Outcome::Taken,\n",
        "",
        [PRESSES],
    ),
    (
        "Escape leaves the menu up",
        "            Some(MenuAction::Closed) => {\n                self.up = None;\n                Outcome::Taken\n            }\n",
        "            Some(MenuAction::Closed) => Outcome::Taken,\n",
        [KEYS],
    ),
    (
        "a key the menu does nothing with reaches the field",
        "            Some(MenuAction::None) | None => Outcome::Taken,\n",
        "            Some(MenuAction::None) | None => Outcome::NotUp,\n",
        [KEYS],
    ),
    (
        "with no menu up, a press is taken",
        "            return Outcome::NotUp;\n        };\n        let field = *field;\n        match event.kind {\n",
        "            return Outcome::Taken;\n        };\n        let field = *field;\n        match event.kind {\n",
        [KEYS],
    ),
    (
        "the pointer lights no row",
        "                menu.handle_mouse_move(event.x, event.y);\n",
        "",
        [DRAWN],
    ),
    (
        "the menu is never drawn",
        "            .map_or_else(Vec::new, |(menu, _)| menu.render(palette))\n",
        "            .map_or_else(Vec::new, |(_menu, _)| Vec::new())\n",
        [DRAWN],
    ),
    (
        "the clock moves no wait on",
        "        self.up.as_mut().is_some_and(|(menu, _)| menu.tick(now_ms))\n",
        "        self.up.as_mut().is_some_and(|(_menu, _)| false)\n",
        [CLOCK],
    ),
    (
        "no tick is asked for",
        "            .and_then(|(menu, _)| menu.due_in(now_ms))\n",
        "            .and_then(|(_menu, _)| None::<u64>)\n",
        [CLOCK],
    ),
    (
        "a tick of nought is asked for",
        "            .map(|ms| ms.max(1))\n",
        "            .map(|ms| ms)\n",
        [CLOCK],
    ),
]


if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "fieldmenu", timeout=600, only=only))
