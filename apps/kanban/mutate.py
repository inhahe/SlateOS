"""Mutation test for the kanban boards file: what is kept, how it is read back,
the ids an import brings, and the close.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The app kept nothing: every card was gone when the window closed, and a
board's JSON export was the only way to keep one.  An import also kept the ids
its file carried without moving the id counter past them, so the next card
made could replace an imported one.  The table covers the file it keeps now
(design-decisions §1205's kind), the import, the status line and the close;
the rest of the suite predates them.

The rows after the file's (2026-09-27): nothing in the running program chose a
card -- only the tests did -- so opening, moving, archiving and deleting cards
were all unreachable; and the open card's editing keys, the checklist's tick,
the archive's restore, the column keys and the filter keys were modelled with
no key reaching them.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "boards_written_and_read_again_are_the_same_boards"
REFUSED = "boards_that_cannot_be_read_whole_are_refused_and_say_where"
KEPT = "what_is_put_on_a_board_is_there_next_time"
FAILING = "closing_while_a_save_fails_asks_first"
CHOOSE = "the_arrows_choose_a_card_and_enter_opens_it"
WALK = "walking_down_a_long_column_keeps_the_chosen_card_drawn"
CARD_KEYS = "the_card_keys_work_on_a_card_the_arrows_chose"
NEW_CARD = "a_new_card_is_chosen"
LEVEL = "left_and_right_choose_the_card_level_with_it_in_the_next_column"
TITLE = "e_edits_the_title_starting_from_what_is_there"
DESC = "d_edits_the_description_and_emptying_it_clears_it"
COMMENT = "c_adds_a_comment_signed_by_whoever_is_writing"
TICK = "l_adds_a_checklist_item_and_tab_and_space_tick_it"
TAB_SCROLL = "tab_brings_a_checklist_item_below_the_fold_into_sight"
RESTORE = "an_archived_card_is_restored_to_the_column_it_came_from"
NO_COLUMN = "unarchiving_into_no_column_leaves_the_card_archived"
ORIGIN_KEPT = "where_a_card_was_archived_from_is_kept"
FORMAT_ONE = "a_format_one_boards_file_still_reads"
COLLAPSE = "z_collapses_a_column_and_its_cards_are_not_drawn_or_chosen"
NEXT_ORDER = "shift_t_sorts_by_the_next_order_and_says_which"
DUE_LAST = "sorting_by_due_date_puts_undated_cards_last"
REMOVE_COL = "shift_delete_removes_an_empty_column_and_refuses_one_with_cards"
MARKED = "the_chosen_column_is_marked"
FILTERS = "the_filter_bar_sets_priority_assignee_and_label"
LIST_CURSOR = "the_board_list_opens_on_the_open_board_and_keeps_the_column"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a save writes nothing",
        "            .and_then(|()| safeio::write_str_atomically(&path, &text));",
        "            .and_then(|()| Ok::<(), std::io::Error>(()));",
        [KEPT],
    ),
    (
        "the file loses the comments",
        "            for comment in &card.comments {",
        "            for comment in card.comments.iter().take(0) {",
        [ROUND],
    ),
    (
        "the file loses a column's cards",
        "            push_ids(&mut out, &column.card_ids);\n",
        "",
        [ROUND],
    ),
    (
        "the file loses the archive",
        "        push_ids(&mut out, &board.archived_card_ids);\n",
        "",
        [ROUND],
    ),
    (
        "the file loses a colour's transparency",
        "    if c.a == 255 {",
        "    if true {",
        [ROUND],
    ),
    (
        "the file forgets which board was open",
        r'    let mut out = format!("{BOARDS_MAGIC}\t{BOARDS_FORMAT}\nactive\t{active}\n");',
        r'    let mut out = format!("{BOARDS_MAGIC}\t{BOARDS_FORMAT}\nactive\t0\n");',
        [ROUND],
    ),
    (
        "a later format is half-read",
        "    if version > BOARDS_FORMAT {",
        "    if version > BOARDS_FORMAT + 100 {",
        [REFUSED],
    ),
    (
        "two cards with one number are both taken",
        "                if board.cards.contains_key(&id) {",
        "                if false {",
        [REFUSED],
    ),
    (
        "a column naming a card the board does not have is read",
        "        if !board.cards.contains_key(&id) {",
        "        if false {",
        [REFUSED],
    ),
    (
        "a card in two places is read",
        "        if !placed.insert(id) {",
        "        if !placed.insert(id) && false {",
        [REFUSED],
    ),
    (
        "an open board past the end is taken",
        "        Some((at, index)) if index >= boards.len() => {",
        "        Some((at, index)) if false => {",
        [REFUSED],
    ),
    (
        "a file too big to read whole is read in part",
        "        if read.truncated {",
        "        if false {",
        ["a_file_too_big_to_read_whole_is_refused"],
    ),
    (
        "a refused file is saved over",
        "            Err(why) => {\n                self.persist = false;\n",
        "            Err(why) => {\n",
        ["a_file_that_cannot_be_read_is_left_as_it_is"],
    ),
    (
        "a first run writes the starting board",
        "        app.kept_text = boards_text(&app.boards, app.active_board_idx);\n",
        "",
        [KEPT],
    ),
    (
        "an event's change is not kept",
        "        if matches!(event, Event::Key(_) | Event::Mouse(_)) {\n            self.keep();\n        }\n",
        "",
        [KEPT, FAILING],
    ),
    (
        "a failed save is not said",
        '                self.store_error = Some(format!("Not saved to {}: {err}", path.shown()));',
        "                drop(err);",
        [FAILING],
    ),
    (
        "the window closes over a failing save",
        "        if !self.unkept() {",
        "        if true {",
        [FAILING],
    ),
    (
        "keys reach the board under the question",
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "            && false",
        [FAILING],
    ),
    (
        "Save leaves while the save still fails",
        "                self.quit = !self.unkept();",
        "                self.quit = true;",
        [FAILING],
    ),
    (
        "an id read is not moved past",
        "        NEXT_ID.fetch_max(raw.saturating_add(1), Ordering::Relaxed);\n",
        "",
        ["a_card_made_after_an_import_replaces_nothing"],
    ),
    (
        "an import keeps a card in two columns",
        "                .retain(|id| board.cards.contains_key(id) && placed.insert(*id));",
        "                .retain(|id| board.cards.contains_key(id) && (placed.insert(*id) || true));",
        ["an_import_leaves_a_board_that_can_be_kept"],
    ),
    (
        "a stamp after a restart is earlier than the kept ones",
        "                self.last_stamp = self.last_stamp.max(latest);\n",
        "",
        ["stamps_are_the_clocks_and_always_later"],
    ),
    (
        "the status line is drawn nowhere",
        "    if app.view != View::CardDetail {\n        render_status(&mut tree, app, width, height);\n    }\n",
        "",
        ["the_empty_board_says_how_to_start_where_it_can_be_seen"],
    ),
    (
        "what an import did is drawn nowhere",
        "    } else if let Some(action) = &app.last_file_action {",
        "    } else if let Some(action) = None::<&String> {",
        ["what_an_import_did_is_on_the_status_line"],
    ),
    (
        "Down chooses nothing",
        "        Key::Down if app.view == View::Board => app.step_card(1),",
        "        Key::Down if app.view == View::Board => false,",
        [CHOOSE],
    ),
    (
        "the board does not follow the choice",
        "            if row < shown.end() {\n                break;\n            }",
        "            if row < shown.end() || true {\n                break;\n            }",
        [WALK],
    ),
    (
        "nothing is chosen after a delete",
        "        self.selected_card = None;\n        if let Some(row) = row {\n            self.choose_in_column(self.selected_column, row);\n        }",
        "        self.selected_card = None;\n        let _ = row;",
        [CARD_KEYS],
    ),
    (
        "the choice stays behind when M moves the card",
        "                            .move_card(card_id, from_col, to_col, 0);\n                        app.selected_column = to_col;",
        "                            .move_card(card_id, from_col, to_col, 0);",
        [CARD_KEYS],
    ),
    (
        "a new card is not chosen",
        "                    if let Some(id) = app.add_card(&text, col) {\n                        app.selected_card = Some(id);",
        "                    if let Some(_id) = app.add_card(&text, col) {\n                        app.selected_card = None;",
        [NEW_CARD],
    ),
    (
        "Right lands on the top card, not the level one",
        "        Key::Right if app.view == View::Board => {\n            let to = app.selected_column.saturating_add(1);\n            if to >= app.active_board().columns.len() {\n                return false;\n            }\n            let row = app.selected_row().unwrap_or(0);",
        "        Key::Right if app.view == View::Board => {\n            let to = app.selected_column.saturating_add(1);\n            if to >= app.active_board().columns.len() {\n                return false;\n            }\n            let row = 0;",
        [LEVEL],
    ),
    (
        "an edit of the title starts empty",
        "        Key::E if plain => (InputMode::EditCardTitle, card.title.clone()),",
        "        Key::E if plain => (InputMode::EditCardTitle, String::new()),",
        [TITLE],
    ),
    (
        "an emptied description is kept",
        "                && !matches!(mode, InputMode::CardDescription | InputMode::AssigneeFilter)",
        "                && !matches!(mode, InputMode::AssigneeFilter)",
        [DESC],
    ),
    (
        "every comment is by User",
        "                            card.add_comment(&author, &text, ts);",
        "                            card.add_comment(\"User\", &text, ts);",
        [COMMENT],
    ),
    (
        "Space ticks nothing",
        "        card.toggle_checklist_item(item_id);\n        true",
        "        let _ = item_id;\n        true",
        [TICK],
    ),
    (
        "Tab does not scroll to the item",
        "        self.checklist_focus = Some(to);\n        self.reveal_checklist_focus();",
        "        self.checklist_focus = Some(to);",
        [TAB_SCROLL],
    ),
    (
        "unarchiving clears the archive before finding the column",
        "        if column_idx >= self.columns.len() || !self.archived_card_ids.contains(&card_id) {",
        "        self.archived_card_ids.retain(|&c| c != card_id);\n        if column_idx >= self.columns.len() {",
        [NO_COLUMN],
    ),
    (
        "restore ignores where the card came from",
        "            .and_then(|id| self.columns.iter().position(|c| c.id == id))\n            .unwrap_or(0);",
        "            .and_then(|_id| None::<usize>)\n            .unwrap_or(0);",
        [RESTORE],
    ),
    (
        "the origin is not written to the boards file",
        "                out.push_str(&format!(\"origin\\t{}\\t{}\\n\", card_id.0, from.0));",
        "                let _ = (card_id, from);",
        [ORIGIN_KEPT],
    ),
    (
        "a format 1 file is refused",
        "const BOARDS_OLDEST: u32 = 1;",
        "const BOARDS_OLDEST: u32 = 2;",
        [FORMAT_ONE],
    ),
    (
        "a collapsed column's cards can still be chosen",
        "            .is_some_and(|c| c.collapsed)\n        {\n            return Vec::new();\n        }",
        "            .is_some_and(|c| c.collapsed && false)\n        {\n            return Vec::new();\n        }",
        [COLLAPSE],
    ),
    (
        "Shift+T keeps the same order",
        "                let next = order.next();",
        "                let next = order;",
        [NEXT_ORDER],
    ),
    (
        "undated cards sort first by due date",
        "                            (Some(_), None) => std::cmp::Ordering::Less,\n                            (None, Some(_)) => std::cmp::Ordering::Greater,",
        "                            (Some(_), None) => std::cmp::Ordering::Greater,\n                            (None, Some(_)) => std::cmp::Ordering::Less,",
        [DUE_LAST],
    ),
    (
        "Shift+Delete removes a column with cards",
        "            if cards > 0 {\n                app.note = Some(format!(",
        "            if cards > usize::MAX - 1 {\n                app.note = Some(format!(",
        [REMOVE_COL],
    ),
    (
        "the chosen column is not marked",
        "        if ci == app.selected_column && app.view == View::Board {",
        "        if ci == app.selected_column && app.view == View::Board && false {",
        [MARKED],
    ),
    (
        "a filter leaves a hidden card chosen",
        "        if self.selected_card.is_some() && self.selected_row().is_none() {\n            self.choose_in_column(self.selected_column, 0);",
        "        if false {\n            self.choose_in_column(self.selected_column, 0);",
        [FILTERS],
    ),
    (
        "the board list opens wherever its cursor was",
        "            app.board_cursor = app.active_board_idx;\n            true",
        "            true",
        [LIST_CURSOR],
    ),
    (
        'a chord raises the keys',
        '    if key.key == Key::F1 && plain {',
        '    if key.key == Key::F1 {',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'AltGr is taken for Ctrl',
        '    if textline::is_ctrl_chord(key.modifiers) {\n        return handle_ctrl_chord(app, key);',
        '    if key.modifiers.ctrl {\n        return handle_ctrl_chord(app, key);',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'AltGr is taken for Alt',
        '    if key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key {',
        '    if key.modifiers.alt {',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        "the Windows key's chords work the board",
        '    if !plain {\n        return false;\n    }\n',
        '',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'a chord works the archive',
        '    let plain = textline::is_plain(key.modifiers);\n    match key.key {\n        Key::Up if plain => {',
        '    let plain = true;\n    match key.key {\n        Key::Up if plain => {',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'AltGr is taken for Ctrl in the archive',
        '        Key::D if textline::is_ctrl_chord(key.modifiers) => {',
        '        Key::D if key.modifiers.ctrl => {',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'a chord works the open card',
        '    let plain = textline::is_plain(key.modifiers);\n    let card_id = app.selected_card?;',
        '    let plain = true;\n    let card_id = app.selected_card?;',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        "a command's letter is typed into the input line",
        '    if textline::types_into_field(key) {\n        app.input_buffer.extend(key.typed());',
        '    if key.types_text() {\n        app.input_buffer.extend(key.typed());',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'a chord works the input line',
        '    if !textline::is_plain(key.modifiers) {\n        return false;\n    }\n    match key.key {\n        Key::Escape => {\n            app.input_mode = InputMode::None;',
        '    match key.key {\n        Key::Escape => {\n            app.input_mode = InputMode::None;',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "kanban", timeout=900, only=only))
