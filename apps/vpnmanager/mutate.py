"""Mutation test for the VPN manager.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the boxes drawn by the toolkit in the theme's shape
(`guitk::field::draw`, lane C's c-e-a-theme-can-shape-the-controls) and the
pointer the window follows to light them.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

FIELDS = "the_boxes_are_the_toolkits_fields"
SWITCHES = "the_on_off_rows_are_the_toolkits_switches"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "the search box is drawn the same wherever the pointer is",
        "            hovered: app.hover == Some(Target::Focus(Field::Search)),",
        "            hovered: false,",
        [FIELDS],
    ),
    (
        "the pointer is not followed",
        "                    let over = self.hit_test(mouse.x, mouse.y, size);",
        "                    let over = None::<Target>;",
        [FIELDS],
    ),
    (
        "a move that changes nothing redraws",
        "                    if over == self.hover {\n                        Action::None",
        "                    if over == self.hover && false {\n                        Action::None",
        [FIELDS],
    ),
    (
        "the pointer leaving the window leaves its box lit",
        "                    if self.hover.take().is_some() {",
        "                    if self.hover.is_some() {",
        [FIELDS],
    ),
    (
        "the boxes take the toolkit's focus width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();",
        "        let _ = settings;",
        [FIELDS],
    ),
    (
        "a switch is drawn the same wherever the pointer is",
        "        guitk::switch::State {\n            hovered: app.hover == Some(target),",
        "        guitk::switch::State {\n            hovered: false,",
        [SWITCHES],
    ),
    (
        "a switch is drawn the same on and off",
        "        rect,\n        enabled,\n        guitk::switch::Look::accent(pal),",
        "        rect,\n        false,\n        guitk::switch::Look::accent(pal),",
        [SWITCHES],
    ),
    (
        "only the switch takes a press, not its row",
        "    frame.hit(target, toggle_row_hit(x, y));",
        "    frame.hit(target, guitk::switch::hit(rect));",
        [SWITCHES],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "vpnmanager", timeout=600, only=only))
