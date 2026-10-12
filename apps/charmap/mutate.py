"""Mutation test for the character map.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the shortcut card's hold on the pointer: a press with the
card up puts it away and reaches nothing under it, and the wheel scrolls
nothing it covers (known-issues
E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a press goes through the shortcut card",
        "            MouseEventKind::Press(_) if app.show_help => {\n"
        "                app.show_help = false;\n"
        "                EventResult::Consumed\n"
        "            }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the card away",
        "            MouseEventKind::Press(_) if app.show_help => {\n",
        "            MouseEventKind::Press(MouseButton::Left) if app.show_help => {\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the card covers",
        "            MouseEventKind::Scroll { .. } if app.show_help => EventResult::Ignored,\n",
        "",
        [CARD],
    ),
]

FIELD = "the_search_box_is_the_toolkits_field"
TYPED = "the_query_is_typed_into_the_box_with_its_caret"
ALTGR = "altgr_types_into_the_search_and_a_command_does_not"
CLOSE = "a_press_opens_the_search_and_the_cross_closes_it"

MUTATIONS += [
    # The search box is a search box: the toolkit's field, with the query
    # typed in it and a x to close it (2026-10-04; lane C,
    # c-e-a-theme-can-shape-the-controls).
    (
        "the box never lights",
        "            hovered: open && self.search_hovered,\n",
        "            hovered: false,\n",
        [FIELD],
    ),
    (
        "the box is never marked",
        "            focused: open && self.search_active,\n",
        "            focused: false,\n",
        [FIELD],
    ),
    (
        "the box shows through the card",
        "        let open = !self.show_help;\n        field::State {\n",
        "        let open = true;\n        field::State {\n",
        [FIELD],
    ),
    (
        "a query that finds nothing is not red",
        "                && self.search_results.is_empty(),\n",
        "                && false,\n",
        [FIELD],
    ),
    (
        "the light stays after the pointer leaves",
        "        let over = at.is_some_and(|(x, y)| self.layout().search.contains(x, y));\n",
        "        let over = at.is_some_and(|(x, y)| self.layout().search.contains(x, y)) || self.search_hovered;\n",
        [FIELD],
    ),
    (
        "moving within the box asks for a repaint",
        "        if over == self.search_hovered {\n            return false;\n        }\n",
        "",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
    (
        "the query is not drawn in its box",
        "        if self.search_active {\n            let mut tree = RenderTree::new();\n",
        "        if false {\n            let mut tree = RenderTree::new();\n",
        [TYPED, CLOSE],
    ),
    # No rows for "AltGr's characters are refused" or "a command's letter is
    # typed": since 2026-10-04 the search's typing is textline::apply_key's,
    # which makes both distinctions itself, in its own crate and with its own
    # tests; altgr_types_into_the_search_and_a_command_does_not still holds the
    # search to them.
    (
        "AltGr+C copies",
        "        let ctrl = textline::is_ctrl_chord(event.modifiers);\n",
        "        let ctrl = event.modifiers.ctrl;\n",
        [ALTGR],
    ),
    (
        "a press in an open search's box closes it",
        "                if !self.search_active {\n                    self.set_search_active(true);\n                }\n",
        "                let open = self.search_active;\n                self.set_search_active(!open);\n",
        [CLOSE],
    ),
    (
        "the x does not close the search",
        "            Target::SearchClose => self.set_search_active(false),\n",
        "            Target::SearchClose => {}\n",
        [CLOSE],
    ),
]

# The search edits at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): it
# took typing at its end and Backspace from it, and nothing else.
EDITS = "the_search_edits_at_a_caret"
COPY = "ctrl_c_copies_the_character_unless_the_query_is_selected"
GRID = "the_grid_keeps_its_keys_beside_the_search"
CTRL_F = "ctrl_f_with_the_search_open_selects_the_query"
SHOWN = "the_search_edits_what_it_shows"

MUTATIONS += [
    (
        "the open search's keys are the grid's",
        "        if self.search_active && self.search_takes(event) {\n",
        "        if false {\n",
        [EDITS, GRID],
    ),
    (
        "Ctrl+C copies the query with none of it selected",
        "                self.search_editor.text() == self.search_query && self.search_editor.has_selection()\n",
        "                self.search_editor.text() == self.search_query\n",
        [COPY],
    ),
    (
        "Ctrl+C never copies the query",
        "                self.search_editor.text() == self.search_query && self.search_editor.has_selection()\n",
        "                false\n",
        [EDITS],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.clipboard = Some(copied);\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "a paste pastes nothing",
        "            self.clipboard.as_deref().unwrap_or(\"\"),\n",
        "            \"\",\n",
        [EDITS, COPY],
    ),
    (
        "an edit of the query is not searched for",
        "            self.search_query = self.search_editor.text().to_owned();\n"
        "            self.perform_search();\n",
        "            self.search_query = self.search_editor.text().to_owned();\n",
        [EDITS],
    ),
    (
        "every key the search answers is a redraw",
        "            ) != before,\n",
        "            ) != before || true,\n",
        [GRID],
    ),
    (
        "a key finds the editor holding another query",
        "        if self.search_editor.text() != self.search_query {\n"
        "            self.search_editor.set_text(&self.search_query);",
        "        if false {\n"
        "            self.search_editor.set_text(&self.search_query);",
        [SHOWN],
    ),
    (
        "Ctrl+F with the search open does not select the query",
        "                self.search_editor.select_all();\n",
        "",
        [CTRL_F],
    ),
    (
        "the query's caret is drawn at its end",
        "            let (cursor, selection_anchor) = self.search_caret();\n",
        "            let (cursor, selection_anchor) = (\n"
        "                guitk::text::TextCursor::from(self.search_query.len()),\n"
        "                None,\n"
        "            );\n",
        [EDITS],
    ),
    (
        "a press puts the caret at the start",
        "            x - area.x,\n",
        "            0.0,\n",
        [EDITS],
    ),
    (
        "a press in the box does not place the caret",
        "                self.press_search(self.layout().search, x);\n",
        "",
        [EDITS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "charmap", timeout=600, only=only))
