"""Mutation test for podcast.

Its keys: Ctrl+S and Ctrl+O are Ctrl chords, not AltGr; the named keys are
taken with nothing but Shift held; a letter counts only as a character typed.
And the list of keys is modal for the pointer too: a press with it up puts it
away and reaches nothing under it.

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

SRC = Path(__file__).parent / 'src' / 'main.rs'

CARD = 'the_shortcut_card_takes_a_press_rather_than_passing_it_on'

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # The list of keys takes a press rather than letting it reach the row
    # under it (known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        'a press goes through the list of keys',
        '        if self.show_help\n'
        '            && matches!(\n'
        '                event.kind,\n'
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_)\n'
        '            )\n'
        '        {\n'
        '            self.show_help = false;\n'
        '            return true;\n'
        '        }\n',
        '',
        [CARD],
    ),
    (
        'only the left button puts the list of keys away',
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_)\n'
        '            )\n'
        '        {\n',
        '                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_)\n'
        '            )\n'
        '        {\n',
        [CARD],
    ),
    (
        'a press under the list of keys leaves it up',
        '            )\n'
        '        {\n'
        '            self.show_help = false;\n'
        '            return true;\n',
        '            )\n'
        '        {\n'
        '            return true;\n',
        [CARD],
    ),
    (
        'a chord raises the keys',
        '        if plain && (event.key == Key::F1 || (event.key == Key::Slash && event.modifiers.shift)) {',
        '        if event.key == Key::F1 || (event.key == Key::Slash && event.modifiers.shift) {',
        ['a_chord_is_neither_a_player_key_nor_its_letter'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(event.modifiers) {',
        '        if event.modifiers.ctrl {',
        ['a_chord_is_neither_a_player_key_nor_its_letter'],
    ),
    (
        'a chord works the named keys and its letter',
        '        if !plain {\n            return textline::types_into_field(event) && self.handle_typed(event);\n        }\n',
        '',
        ['a_chord_is_neither_a_player_key_nor_its_letter'],
    ),
    (
        "a chord's letter is a shortcut",
        '            return textline::types_into_field(event) && self.handle_typed(event);',
        '            return self.handle_typed(event);',
        ['a_chord_is_neither_a_player_key_nor_its_letter'],
    ),
]

# The search takes typing, its box is the toolkit's field, and its results
# are a list the arrows and the pointer reach (2026-10-04; lane C,
# c-e-a-theme-can-shape-the-controls).
TYPING = "the_search_takes_typing_and_finds_as_it_is_typed"
RESULTS = "the_searchs_results_are_a_list"
FIELD = "the_search_box_is_the_toolkits_field"

MUTATIONS += [
    (
        "the search takes no typing",
        "        if self.main_view == MainView::Search && self.handle_search_key(event) {\n",
        "        if false && self.handle_search_key(event) {\n",
        [TYPING, RESULTS, FIELD],
    ),
    (
        "Escape leaves a query in the search",
        "        if event.key == Key::Escape\n"
        "            && textline::is_plain(event.modifiers)\n"
        "            && !self.search_query.is_empty()\n"
        "        {\n"
        "            self.set_search_query(String::new());\n"
        "            return true;\n"
        "        }\n",
        "",
        [TYPING],
    ),
    (
        "the query is not searched again as it is typed",
        "        self.search_query = query;\n        self.perform_search();\n",
        "        self.search_query = query;\n",
        [TYPING],
    ),
    (
        "the search box's editor is not reloaded",
        "        if self.search_editor.text() != self.search_query {\n"
        "            self.search_editor.set_text(&self.search_query);\n"
        "        }\n"
        "        let edit = textline::apply_key(\n",
        "        let edit = textline::apply_key(\n",
        [TYPING, RESULTS],
    ),
    (
        "a cut takes nothing to the clipboard",
        "            self.search_clipboard = copied;\n",
        "            let _ = copied;\n",
        [TYPING],
    ),
    (
        "a key the box does not answer is taken",
        "        edit.handled\n    }\n",
        "        true\n    }\n",
        [TYPING],
    ),
    (
        "the results are not the list",
        "        if self.main_view == MainView::Search {\n"
        "            return self.search_results.clone();\n"
        "        }\n",
        "",
        [RESULTS],
    ),
    (
        "a result's details play nothing",
        "        if self.main_view == MainView::EpisodeDetail {\n"
        "            return self\n",
        "        if false {\n"
        "            return self\n",
        [RESULTS],
    ),
    (
        "Escape from a result's details goes to the episode list",
        "                    self.main_view = self.detail_from;\n",
        "                    self.main_view = MainView::EpisodeList;\n",
        [RESULTS],
    ),
    (
        "Enter does not open a result",
        "                    MainView::Search => self.selected_episode().is_some(),\n",
        "                    MainView::Search => false,\n",
        [RESULTS],
    ),
    (
        "a press on a result does nothing",
        "        if matches!(self.main_view, MainView::EpisodeList | MainView::Search) {\n",
        "        if matches!(self.main_view, MainView::EpisodeList) {\n",
        [RESULTS],
    ),
    (
        "a press finds the result under the episode list's rows",
        "        if self.main_view == MainView::Search {\n            SEARCH_LIST_TOP\n",
        "        if false {\n            SEARCH_LIST_TOP\n",
        [RESULTS],
    ),
    (
        "the search box never has the keyboard's mark",
        "            focused: self.main_view == MainView::Search\n"
        "                && !self.show_help\n"
        "                && !self.picker.is_open(),\n",
        "            focused: false,\n",
        [FIELD],
    ),
    (
        "the search box keeps its mark under the list of keys",
        "            focused: self.main_view == MainView::Search\n"
        "                && !self.show_help\n"
        "                && !self.picker.is_open(),\n",
        "            focused: self.main_view == MainView::Search\n"
        "                && !self.picker.is_open(),\n",
        [FIELD],
    ),
    (
        "the search box keeps its mark under the file dialog",
        "            focused: self.main_view == MainView::Search\n"
        "                && !self.show_help\n"
        "                && !self.picker.is_open(),\n",
        "            focused: self.main_view == MainView::Search\n"
        "                && !self.show_help,\n",
        [FIELD],
    ),
    (
        "a search that finds nothing is not red",
        "            invalid: !self.search_query.is_empty() && self.search_results.is_empty(),\n",
        "            invalid: false,\n",
        [FIELD],
    ),
    (
        "an empty box with the keyboard has no caret",
        "                textedit::push_caret(\n"
        "                    &mut tree,\n"
        "                    x,\n"
        "                    y,\n"
        "                    line,\n"
        "                    self.palette.text,\n"
        "                    textedit::CARET_WIDTH,\n"
        "                );\n",
        "                let _ = (x, y, line);\n",
        [FIELD],
    ),
    (
        "the search's caret is at its start",
        "                    cursor: self.search_cursor(),\n",
        "                    cursor: TextCursor::default(),\n",
        [FIELD],
    ),
    (
        "a press leaves the caret where it was",
        "        self.search_editor.set_cursor(cursor);\n",
        "        let _ = cursor;\n",
        [FIELD],
    ),
    (
        "a press in the search box does nothing",
        "            self.press_search_box(event.x);\n",
        "            let _ = event.x;\n",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "podcast", timeout=600, only=only))
