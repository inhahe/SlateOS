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

# The list of keys, and the keyboard's way to the boxes (2026-10-04). F1 did
# nothing; the search box and the split-tunnel range box could be reached only
# by a press; and Alt+Left switched the tab under a window being dragged.
EVERY = "every_advertised_key_does_something"
REACHES = "the_shortcut_list_reaches_the_window"
QUESTION = "a_question_mark_is_typed_into_the_search_and_f1_still_raises_the_list"
MODAL = "the_shortcut_list_takes_the_keys_and_a_press"
BOXES = "tab_and_ctrl_f_reach_the_boxes"
CHORD = "a_chord_is_not_one_of_this_windows_keys"
RING = "tab_walks_the_dialog_fields_in_a_ring"

HELP_ANCHOR = "        if plain && (key.key == Key::F1 || question && self.focus.is_none()) {\n"
CLOSE_ANCHOR = "            if plain && (matches!(key.key, Key::F1 | Key::Escape) || question) {\n"
CTRL_F = (
    "        if key.key == Key::F && textline::is_ctrl_chord(key.modifiers) && !self.show_add_dialog {\n"
)

MUTATIONS += [
    (
        "the list of keys never comes up",
        "            self.show_help = true;\n            return Action::Redraw;\n",
        "            return Action::Redraw;\n",
        [REACHES],
    ),
    (
        "F1 raises nothing from a box",
        HELP_ANCHOR,
        "        if plain && self.focus.is_none() && (key.key == Key::F1 || question) {\n",
        [QUESTION],
    ),
    (
        "? raises the list from a box",
        HELP_ANCHOR,
        "        if plain && (key.key == Key::F1 || question) {\n",
        [QUESTION],
    ),
    (
        "Alt+F1 raises the list",
        HELP_ANCHOR,
        "        if key.key == Key::F1 || plain && question && self.focus.is_none() {\n",
        [REACHES],
    ),
    (
        "the list is not modal for the keys",
        "                self.show_help = false;\n"
        "            }\n"
        "            return Action::Redraw;\n"
        "        }\n",
        "                self.show_help = false;\n"
        "            }\n"
        "        }\n",
        [MODAL],
    ),
    (
        "Escape leaves the list up",
        CLOSE_ANCHOR,
        "            if plain && (matches!(key.key, Key::F1) || question) {\n",
        [REACHES],
    ),
    (
        "Alt+Escape puts the list away",
        CLOSE_ANCHOR,
        "            if matches!(key.key, Key::F1 | Key::Escape) || question {\n",
        [REACHES],
    ),
    (
        "the list of keys is not drawn",
        "    if app.show_help {\n        frame.discard_hits();\n",
        "    if false {\n        frame.discard_hits();\n",
        [REACHES],
    ),
    (
        "a press reaches what the list covers",
        "            Event::Mouse(mouse) if self.show_help => match mouse.kind {\n",
        "            Event::Mouse(mouse) if self.show_help && !matches!(mouse.kind, MouseEventKind::Press(_)) => match mouse.kind {\n",
        [MODAL],
    ),
    (
        "the wheel scrolls what the list covers",
        "            Event::Mouse(mouse) if self.show_help => match mouse.kind {\n",
        "            Event::Mouse(mouse) if self.show_help && !matches!(mouse.kind, MouseEventKind::Scroll { .. }) => match mouse.kind {\n",
        [MODAL],
    ),
    (
        "Ctrl+F does not reach the search",
        CTRL_F,
        "        if false {\n",
        [EVERY, BOXES],
    ),
    (
        "AltGr+F is Ctrl+F",
        CTRL_F,
        "        if key.key == Key::F && key.modifiers.ctrl && !self.show_add_dialog {\n",
        [BOXES],
    ),
    (
        "a chord reaches the window's keys",
        "        if !plain {\n            return Action::None;\n        }\n",
        "",
        [CHORD],
    ),
    (
        "Tab does not reach the search",
        "            Key::Tab => self.focus_field(Field::Search),\n",
        "",
        [EVERY, BOXES],
    ),
    (
        "Tab from the search never reaches the range box",
        "            Field::Search if ranges_drawn => Field::AllowedIp,\n",
        "",
        [BOXES],
    ),
    (
        "Tab from the range box stays there",
        "            Field::Search | Field::AllowedIp => Field::Search,\n",
        "            Field::Search => Field::Search,\n            Field::AllowedIp => Field::AllowedIp,\n",
        [BOXES],
    ),
    (
        "the range box is reached with no profile chosen",
        "            self.current_tab == DetailTab::SplitTunnel && self.selected_profile.is_some();\n",
        "            self.current_tab == DetailTab::SplitTunnel;\n",
        [BOXES],
    ),
    (
        "the dialog's ring is broken",
        "            Field::Port => Field::Mtu,\n",
        "            Field::Port => Field::Name,\n",
        [RING],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "vpnmanager", timeout=600, only=only))
