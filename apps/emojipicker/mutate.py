"""Mutation test for the emoji picker.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the search field drawn by the toolkit in the theme's shape
(`guitk::field::draw`, lane C's c-e-a-theme-can-shape-the-controls) and what
the pointer lights.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

FIELD = "the_search_field_is_the_toolkits_lit_under_the_pointer_and_focused_at_the_users_width"
LEAVE = "leaving_the_window_puts_out_the_cell_under_the_pointer"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "the field is drawn the same wherever the pointer is",
        "                hovered: self.search_hovered,",
        "                hovered: false,",
        [FIELD],
    ),
    (
        "the field is drawn the same whether it has the keyboard or not",
        "                focused: self.search_focused && !self.show_help,",
        "                focused: false,",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();",
        "        let _ = settings;",
        [FIELD],
    ),
    (
        "the pointer over the field is not noticed",
        "            state.search_hovered = matches!(target, Some(Target::SearchField));",
        "            let _ = &target;",
        [FIELD],
    ),
    (
        "the pointer leaving the window leaves the field lit",
        "            state.hovered_emoji = Option::None;\n            state.search_hovered = false;\n",
        "            state.hovered_emoji = Option::None;\n",
        [FIELD],
    ),
    (
        "the pointer leaving the window leaves its cell lit",
        "            state.hovered_emoji = Option::None;\n            state.search_hovered = false;\n",
        "            state.search_hovered = false;\n",
        [LEAVE],
    ),
]

# The keyboard (2026-10-04): the grid answered only the pointer, so no emoji
# could be chosen from the keyboard; the search box took typing at its end
# and Backspace from it; and F1 did nothing.
ARROWS = "the_arrows_light_an_emoji_and_enter_picks_it"
WORD = "a_word_typed_and_enter_picks_the_best_match"
CARET = "the_search_box_edits_at_a_caret"
TABS = "ctrl_tab_walks_the_tabs"
EVERY = "every_advertised_key_does_something"
REACHES = "the_shortcut_list_reaches_the_window"
QUESTION = "a_question_mark_is_typed_into_the_search_and_f1_still_raises_the_list"
MODAL = "the_shortcut_list_takes_the_keys_and_a_press"
BACKSPACE = "backspace_removes_character"

HELP_ANCHOR = "    if plain && (key.key == Key::F1 || question && !state.search_focused) {\n"
CLOSE_ANCHOR = "        if plain && (matches!(key.key, Key::F1 | Key::Escape) || question) {\n"
STEP_ANCHOR = "        state.step_tab(!key.modifiers.shift);\n"
PRESS_ANCHOR = (
    "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
    "                state.show_help = false;\n"
    "                return EventResult::Consumed;\n"
    "            }\n"
)

MUTATIONS += [
    (
        "an arrow from nothing lit lights nothing",
        "            Option::None => 0,\n            Some(at) if delta < 0",
        "            Option::None => return,\n            Some(at) if delta < 0",
        [ARROWS],
    ),
    (
        "Left goes on rather than back",
        "            Some(at) if delta < 0 => at.saturating_sub(delta.unsigned_abs()),\n",
        "            Some(at) if delta < 0 => at.saturating_add(delta.unsigned_abs()),\n",
        [ARROWS],
    ),
    (
        "a move runs past the last emoji",
        "        let at = index.min(last);\n",
        "        let at = index;\n",
        [ARROWS],
    ),
    (
        "the lit emoji is not scrolled into view",
        "        self.hovered_emoji = Some(at);\n        self.reveal(at);\n",
        "        self.hovered_emoji = Some(at);\n",
        [ARROWS],
    ),
    (
        "the first row is scrolled short of the top",
        "        let (top, bottom) = (cell.y - GRID_PADDING, cell.bottom() + GRID_PADDING);\n",
        "        let (top, bottom) = (cell.y, cell.bottom());\n",
        [ARROWS],
    ),
    (
        "Down is one emoji, not a row",
        "            Key::Down => Some(columns),\n",
        "            Key::Down => Some(1),\n",
        [ARROWS],
    ),
    (
        "Up is one emoji, not a row",
        "            Key::Up => Some(columns.saturating_neg()),\n",
        "            Key::Up => Some(-1),\n",
        [ARROWS],
    ),
    (
        "a page is a column's worth, not the rows in view",
        "        rows.saturating_mul(isize::try_from(layout.columns.get()).unwrap_or(1))\n",
        "        rows\n",
        [ARROWS],
    ),
    (
        "PageDown moves nothing",
        "            Key::PageDown => Some(state.page_cells()),\n",
        "            Key::PageDown => Some(0),\n",
        [ARROWS],
    ),
    (
        "Enter picks nothing",
        "                state.pick_lit();\n                return EventResult::Consumed;\n",
        "                return EventResult::Consumed;\n",
        [ARROWS, WORD],
    ),
    (
        "with nothing lit, Enter picks nothing",
        "        self.pick(self.hovered_emoji.unwrap_or(0));\n",
        "        if let Some(i) = self.hovered_emoji {\n            self.pick(i);\n        }\n",
        [WORD],
    ),
    (
        "Space picks nothing",
        "        Key::Space => state.pick_lit(),\n",
        "        Key::Space => {}\n",
        [ARROWS],
    ),
    (
        "Home goes nowhere",
        "        Key::Home => state.light(0),\n",
        "        Key::Home => {}\n",
        [ARROWS],
    ),
    (
        "End goes nowhere",
        "        Key::End => state.light(usize::MAX),\n",
        "        Key::End => {}\n",
        [ARROWS],
    ),
    (
        "Left does nothing",
        "        Key::Left => state.move_highlight(-1),\n",
        "        Key::Left => {}\n",
        [ARROWS],
    ),
    (
        "Right does nothing",
        "        Key::Right => state.move_highlight(1),\n",
        "        Key::Right => {}\n",
        [ARROWS, MODAL],
    ),
    (
        "Tab does not leave the box",
        "                if state.search_focused {\n                    state.search_focused = false;\n",
        "                if state.search_focused {\n                    state.search_focused = true;\n",
        [WORD],
    ),
    (
        "Tab from the grid does not reach the box",
        "                } else {\n                    state.focus_search();\n                }\n",
        "                } else {\n                }\n",
        [WORD],
    ),
    (
        "Ctrl+F does not reach the box",
        "    if ctrl && key.key == Key::F {\n        state.focus_search();\n",
        "    if ctrl && key.key == Key::F {\n",
        [WORD, QUESTION],
    ),
    (
        "Ctrl+Tab does not step",
        STEP_ANCHOR,
        "        let _ = key;\n",
        [TABS],
    ),
    (
        "Ctrl+Shift+Tab steps on",
        STEP_ANCHOR,
        "        state.step_tab(true);\n",
        [TABS],
    ),
    (
        "AltGr+Tab is Ctrl+Tab",
        "    let ctrl = textline::is_ctrl_chord(key.modifiers);\n",
        "    let ctrl = key.modifiers.ctrl;\n",
        [TABS],
    ),
    (
        "a step back is a step on",
        "            all.len().saturating_sub(1)\n",
        "            1\n",
        [TABS],
    ),
    (
        "a chord reaches the grid",
        "    if !plain {\n        return EventResult::Ignored;\n    }\n    match key.key {\n",
        "    match key.key {\n",
        [TABS],
    ),
    (
        "a new tab keeps the old light",
        "        self.scroll_offset = 0.0;\n        self.hovered_emoji = Option::None;\n        if let Tab::Category(cat) = tab {\n",
        "        self.scroll_offset = 0.0;\n        if let Tab::Category(cat) = tab {\n",
        [TABS],
    ),
    (
        "what is typed does not reach the search",
        "            self.search_query = self.search_editor.text().to_owned();\n",
        "",
        [CARET, WORD],
    ),
    (
        "a cut takes nothing to the clipboard",
        "            self.search_clipboard = copied;\n",
        "            let _ = copied;\n",
        [CARET],
    ),
    (
        "a word typed stays on its tab",
        "            if !self.search_query.is_empty() {\n                self.active_tab = Tab::Search;\n            }\n",
        "",
        [WORD],
    ),
    (
        "a new query keeps the old light",
        "            self.scroll_offset = 0.0;\n            self.hovered_emoji = Option::None;\n        }\n        if edit.handled {\n",
        "            self.scroll_offset = 0.0;\n        }\n        if edit.handled {\n",
        [WORD],
    ),
    (
        "the editor is not reloaded from the query",
        "        if self.search_editor.text() != self.search_query {\n"
        "            self.search_editor.set_text(&self.search_query);\n"
        "        }\n"
        "        let edit = textline::apply_key(\n",
        "        let edit = textline::apply_key(\n",
        [BACKSPACE],
    ),
    (
        "a press puts the caret at the start",
        "            x - field.x - SEARCH_TEXT_INSET,\n",
        "            0.0,\n",
        [CARET],
    ),
    (
        "an empty box with the keyboard has no caret",
        "            if focused {\n                let mut tree = RenderTree::new();\n",
        "            if false {\n                let mut tree = RenderTree::new();\n",
        [CARET],
    ),
    (
        "the caret is drawn under the list of keys",
        "        let focused = self.search_focused && !self.show_help;\n",
        "        let focused = self.search_focused;\n",
        [QUESTION],
    ),
    (
        "an empty box does not say what it is for",
        '                "Search emoji...",\n',
        '                "",\n',
        [CARET],
    ),
    (
        "the list of keys never comes up",
        "        state.show_help = true;\n        return EventResult::Consumed;\n",
        "        return EventResult::Consumed;\n",
        [REACHES],
    ),
    (
        "F1 raises nothing from the box",
        HELP_ANCHOR,
        "    if plain && !state.search_focused && (key.key == Key::F1 || question) {\n",
        [QUESTION],
    ),
    (
        "? raises the list from the box",
        HELP_ANCHOR,
        "    if plain && (key.key == Key::F1 || question) {\n",
        [QUESTION],
    ),
    (
        "Alt+F1 raises the list",
        HELP_ANCHOR,
        "    if key.key == Key::F1 || plain && question && !state.search_focused {\n",
        [REACHES],
    ),
    (
        "the list is not modal for the keys",
        "            state.show_help = false;\n        }\n        return EventResult::Consumed;\n    }\n",
        "            state.show_help = false;\n        }\n    }\n",
        [MODAL],
    ),
    (
        "Escape leaves the list up",
        CLOSE_ANCHOR,
        "        if plain && (matches!(key.key, Key::F1) || question) {\n",
        [REACHES],
    ),
    (
        "Alt+Escape puts the list away",
        CLOSE_ANCHOR,
        "        if matches!(key.key, Key::F1 | Key::Escape) || question {\n",
        [REACHES],
    ),
    (
        "the list of keys is not drawn",
        "        if self.show_help {\n            guitk::shortcut::render_card(\n",
        "        if false {\n            guitk::shortcut::render_card(\n",
        [REACHES],
    ),
    (
        "a press reaches what the list covers",
        PRESS_ANCHOR,
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                state.show_help = false;\n"
        "            }\n",
        [MODAL],
    ),
    (
        "a press leaves the list up",
        PRESS_ANCHOR,
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                return EventResult::Consumed;\n"
        "            }\n",
        [MODAL],
    ),
    (
        "the wheel scrolls what the list covers",
        "            MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n            _ => {}\n",
        "            _ => {}\n",
        [MODAL],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "emojipicker", timeout=600, only=only))
