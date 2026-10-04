"""Mutation test for the ebook reader's library.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-27, when the reader stopped opening
on five invented books and began opening real ones: how a book file is read
(`shelf::read_book` -- UTF-8, UTF-16, Windows-1252 said out loud, a cap that
cuts a character in two), how the library is written and read back
(`shelf::to_text`/`parse`, refused whole when damaged), and what the window
does with it -- keeps each book's place, never writes over a library that did
not read, says so when a close cannot save, keeps a book whose file has gone,
asks before taking a book out, and lets the Open dialog take the keys.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

ROUND_TRIP = "a_library_is_read_back_as_it_was_written"
REFUSED = "a_file_that_is_not_a_library_is_refused_whole"
ORDER = "bookmarks_are_read_in_book_order_once_each"
LATIN = "text_that_is_not_utf8_is_read_as_windows_1252_and_says_so"
CAP = "a_book_past_the_cap_is_read_in_part_and_says_so"
BREAKS = "line_breaks_of_every_kind_become_one"
LABEL = "a_type_size_is_known_by_its_label"
KEPT = "a_book_opened_is_kept_with_its_place_bookmarks_and_type_size"
CLOSE = "closing_keeps_the_place_in_the_book_being_read"
CANNOT_SAVE = "a_close_that_cannot_save_says_so_once_and_then_goes"
DAMAGED = "a_library_file_that_does_not_read_is_never_written_over"
GONE = "a_book_whose_file_has_gone_keeps_its_place_and_says_why"
TWICE = "a_book_already_in_the_library_is_opened_not_added_again"
UNREADABLE = "a_file_that_cannot_be_read_is_not_added"
DELETE = "delete_asks_and_then_takes_the_book_out_but_not_the_file"
MODAL = "the_question_is_modal_and_its_buttons_answer_a_click"
DIALOG = "ctrl_o_puts_the_open_dialog_up_and_it_takes_the_keys"
BUTTON = "the_open_button_puts_the_dialog_up"
FIT = "a_place_in_a_file_that_changed_moves_to_one_that_exists"

SHELF = [
    (
        "a path is written as it is",
        "        out.push_str(&pathcodec::encode_path(&book.path));",
        "        out.push_str(&book.path.to_string_lossy());",
        [ROUND_TRIP],
    ),
    (
        "a line that is not a book is skipped",
        "        ) else {\n            return Err(bad());\n        };",
        "        ) else {\n            continue;\n        };",
        [REFUSED],
    ),
    (
        "any file is taken for a library",
        "    if lines.next() != Some(HEADER) {",
        "    if lines.next().is_none() {",
        [REFUSED],
    ),
    (
        "bookmarks are kept in the order written",
        "        bookmarks.sort_unstable();\n",
        "",
        [ORDER],
    ),
    (
        "Windows-1252 goes unsaid",
        '            notes.push("Read as Windows-1252: the file is not UTF-8.".to_string());\n',
        "",
        [LATIN],
    ),
    (
        "a character the cap cut in two makes the book Windows-1252",
        "        Err(e) if truncated && e.utf8_error().error_len().is_none() => {",
        "        Err(e) if false && e.utf8_error().error_len().is_none() => {",
        [CAP],
    ),
    (
        "a carriage return is kept",
        '    let text = text.replace("\\r\\n", "\\n").replace(\'\\r\', "\\n");',
        "    let text = text;",
        [BREAKS],
    ),
]

MAIN = [
    (
        "no size is known by its label",
        "            .find(|size| size.label() == label)",
        "            .find(|_| false)",
        [LABEL, ROUND_TRIP],
    ),
    (
        "a title the text gives is ignored",
        "                let title = read.title.unwrap_or_else(named);",
        "                let title = named();",
        [KEPT],
    ),
    (
        "returning to the library does not keep the place",
        "        if let Kept::Failed(why) = self.keep() {",
        "        if let Kept::Failed(why) = Kept::Saved {",
        [KEPT],
    ),
    (
        "a close does not keep the place",
        "                match self.keep() {\n                    Kept::Failed(why) if !self.close_anyway => {",
        "                match Kept::Saved {\n                    Kept::Failed(why) if !self.close_anyway => {",
        [CLOSE],
    ),
    (
        "a close that cannot save goes without a word",
        "                    Kept::Failed(why) if !self.close_anyway => {",
        "                    Kept::Failed(why) if false => {",
        [CANNOT_SAVE],
    ),
    (
        "a library that did not read is written over",
        "        if let Some(why) = &self.shelf_error {",
        "        if let Some(why) = None::<&String> {",
        [DAMAGED],
    ),
    (
        "a book whose file has gone leaves the library",
        "                    app.library.push(book);\n                    app.reading_states.push(state);",
        "                    if book.unreadable.is_none() {\n"
        "                        app.library.push(book);\n"
        "                        app.reading_states.push(state);\n"
        "                    }",
        [GONE],
    ),
    (
        "a place is fitted to a book that could not be read",
        "    if book.unreadable.is_some() {\n"
        "        // Nothing to fit it to: kept as it was, for when the file comes back.\n"
        "        return state;\n"
        "    }\n",
        "",
        [FIT, GONE],
    ),
    (
        "a book already in the library is added again",
        "            .position(|book| book.path.as_deref() == Some(path))",
        "            .position(|_| false)",
        [TWICE],
    ),
    (
        "a file that cannot be read is added",
        '            self.status = format!("{} could not be opened: {why}", path.shown());\n'
        "            return;",
        '            self.status = format!("{} could not be opened: {why}", path.shown());',
        [UNREADABLE],
    ),
    (
        "Delete takes the book out without asking",
        "            Key::Delete => {\n                self.ask_to_remove();",
        "            Key::Delete => {\n                self.remove_book(self.selected_book);",
        [DELETE],
    ),
    (
        "taking a book out is not kept",
        "        self.status = match self.keep() {",
        "        self.status = match Kept::Saved {",
        [DELETE],
    ),
    (
        "a key behind the question reaches the list",
        "                Key::Escape | Key::N => self.confirm_remove = None,\n"
        "                _ => {}\n"
        "            }\n"
        "            return true;\n",
        "                Key::Escape | Key::N => self.confirm_remove = None,\n"
        "                _ => {}\n"
        "            }\n",
        [MODAL],
    ),
    (
        "a click behind the question reaches the list",
        "            if self.confirm_remove.is_some() {\n"
        "                return self.click_confirm(event.x, event.y);\n"
        "            }\n",
        "",
        [MODAL],
    ),
    (
        "the Open dialog lets keys through",
        "            Picked::Handled | Picked::Cancelled => return Response::Redraw,",
        "            Picked::Handled | Picked::Cancelled => {}",
        [DIALOG],
    ),
    (
        "the Open button does nothing",
        "                    if self.open_button().contains(event.x, event.y) {",
        "                    if false {",
        [BUTTON],
    ),
    (
        'an announced theme is not read',
        '            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {',
        '            Event::SettingsChanged { group } if false && group.file_name() == CONFIG_NAME => {',
        ['the_theme_chosen_in_one_window_reaches_the_others'],
    ),
    (
        "every program's announcement is read as the reader's",
        '            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {',
        '            Event::SettingsChanged { group } if !group.file_name().is_empty() => {',
        ['the_theme_chosen_in_one_window_reaches_the_others'],
    ),
    (
        'a reader that keeps nothing follows the file',
        '        if !self.keeps_settings {\n            return false;\n        }\n        let (theme, problem)',
        '        let (theme, problem)',
        ['a_reader_a_test_builds_follows_no_theme'],
    ),
    (
        'a re-read keeps the theme it had',
        '        let mut changed = std::mem::replace(&mut self.theme, theme) != theme;',
        '        let mut changed = self.theme != theme;',
        ['the_theme_chosen_in_one_window_reaches_the_others'],
    ),
    (
        'a theme written by hand that the reader does not know is not said',
        '            changed |= self.status != problem;\n            self.status = problem;',
        '            changed |= self.status != problem;',
        ['the_theme_chosen_in_one_window_reaches_the_others'],
    ),
    (
        'a re-read says it changed nothing',
        '        changed\n    }\n\n    /// Get current theme colors.',
        '        changed && false\n    }\n\n    /// Get current theme colors.',
        ['the_theme_chosen_in_one_window_reaches_the_others'],
    ),
    (
        'a re-read always says it changed',
        '        changed\n    }\n\n    /// Get current theme colors.',
        '        changed || true\n    }\n\n    /// Get current theme colors.',
        ['the_theme_chosen_in_one_window_reaches_the_others'],
    ),
    (
        'a chord raises the keys',
        '        if event.key == Key::F1 && plain {',
        '        if event.key == Key::F1 {',
        ['a_chord_is_neither_a_readers_key_nor_typing'],
    ),
    (
        'a chord answers the question before a book is removed',
        '                _ if !plain => {}\n',
        '',
        ['a_chord_is_neither_a_readers_key_nor_typing'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(event.modifiers) {',
        '        if event.modifiers.ctrl {',
        ['a_chord_is_neither_a_readers_key_nor_typing'],
    ),
    (
        "a command's letter is typed into the search",
        '&& textline::types_into_field(event)',
        '&& event.types_text()',
        ['a_chord_is_neither_a_readers_key_nor_typing'],
    ),
    (
        'a chord works the reader',
        '        if !plain {\n            return false;\n        }\n',
        '',
        ['a_chord_is_neither_a_readers_key_nor_typing'],
    ),
    # -- the shortcut card's hold on the pointer
    (
        'a press goes through the shortcut card',
        '        if self.show_help {\n'
        '            // The card is modal for the pointer as it is for the keys: a\n',
        '        if false {\n'
        '            // The card is modal for the pointer as it is for the keys: a\n',
        ['test_mouse_click_library_under_the_card'],
    ),
    (
        'only the left button puts the card away',
        '            if matches!(event.kind, MouseEventKind::Press(_)) {\n'
        '                self.show_help = false;\n',
        '            if matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {\n'
        '                self.show_help = false;\n',
        ['test_mouse_click_library_under_the_card'],
    ),
]

TABLES = {
    "shelf.rs": SHELF,
    "main.rs": MAIN,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "ebook", timeout=900, only=mine or None))
    raise SystemExit(worst)
