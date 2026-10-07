"""Mutation test for the radio.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the station list's wheel, as it stands since 2026-10-04: it
scrolls the view by the distance turned -- three rows a notch, a touchpad's
fractions added up -- and leaves the picked station picked, where it used to
move the selection one station per event whatever the event's size.  And
the list of keys' hold on the pointer: a press with it up puts it away and
reaches nothing under it, and the wheel scrolls nothing it covers.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

WHEEL = "the_wheel_scrolls_the_list_by_the_distance_turned"
FRACTION = "a_fraction_of_a_notch_does_not_outlive_the_list"
CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # The list of keys takes the pointer (known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        "a press goes through the list of keys",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n"
        "                    return true;\n"
        "                }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the list of keys away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        [CARD],
    ),
    (
        "a press under the list of keys leaves it up",
        "                    self.show_help = false;\n"
        "                    return true;\n",
        "                    return true;\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the list of keys covers",
        "                MouseEventKind::Scroll { .. } => return false,\n",
        "",
        [CARD],
    ),
    (
        "the wheel moves the selection",
        "                self.station_view.scroll_by(rows, len);\n",
        "                if rows < 0 {\n"
        "                    self.station_view.select_prev(len);\n"
        "                } else if rows > 0 {\n"
        "                    self.station_view.select_next(len);\n"
        "                }\n",
        [WHEEL],
    ),
    (
        "a fraction of a notch is truncated",
        "                let rows = self.station_wheel.rows(dy);\n",
        "                let rows = wheel::rows_f(dy) as isize;\n",
        [WHEEL],
    ),
    (
        "a turn that moves nothing says it moved",
        "                self.station_view.first_visible() != before\n",
        "                true\n",
        [WHEEL],
    ),
    (
        "a fraction of a notch outlives the list",
        "        self.station_wheel.reset();\n",
        "",
        [FRACTION],
    ),
]

# The search box is the toolkit's field, edited by textline's editor; the
# keys bound to themselves are plain (2026-10-04; lane C,
# c-e-a-theme-can-shape-the-controls).
FIELD = "the_search_box_is_the_toolkits_field"
CHORD_CTRL = "a_control_chord_does_not_type_into_the_search"
CHORD_ALT = "an_alt_or_windows_chord_is_no_shortcut"

MUTATIONS += [
    (
        "a chord raises the list of keys",
        "        if plain && (event.key == Key::F1 || (event.key == Key::Slash && event.modifiers.shift)) {\n",
        "        if event.key == Key::F1 || (event.key == Key::Slash && event.modifiers.shift) {\n",
        [CHORD_ALT],
    ),
    (
        "a chord works the player's keys",
        "        if !plain {\n            return false;\n        }\n        match event.key {\n",
        "        match event.key {\n",
        [CHORD_ALT],
    ),
    (
        # What the search did before the editor: type the text of any key
        # but a Ctrl chord -- AltGr's refused with it, Alt's letter typed.
        "the search refuses AltGr and types Alt's letter",
        "                _ => return self.edit_search(event),\n",
        "                _ => {\n"
        "                    let typed: String = event.typed().collect();\n"
        "                    if typed.is_empty() || event.modifiers.ctrl {\n"
        "                        return self.edit_search(event);\n"
        "                    }\n"
        "                    self.search_query.push_str(&typed);\n"
        "                    return true;\n"
        "                }\n",
        [FIELD],
    ),
    (
        "the search box never has the keyboard",
        "            focused: self.search_active && !self.show_help,\n",
        "            focused: false,\n",
        [FIELD],
    ),
    (
        "the search box keeps its mark under the list of keys",
        "            focused: self.search_active && !self.show_help,\n",
        "            focused: self.search_active,\n",
        [FIELD],
    ),
    (
        "a search that finds nothing is not red",
        "            invalid: !self.search_query.trim().is_empty() && self.search_results.is_empty(),\n",
        "            invalid: false,\n",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
    (
        "the caret is drawn at the start of the search",
        "                        self.search_editor.cursor()\n",
        "                        text::TextCursor::default()\n",
        [FIELD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "radio", timeout=600, only=only))
