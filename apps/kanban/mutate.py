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

The two-window rows (2026-10-09, design-decisions §1239): two windows of the
board each wrote their own copy of the file over the other's, so the last to
save threw away the other's cards. A save now reads the file and puts in only
what this window changed (`src/merge.rs`, its own table below), and a window
reads the file again when another saves.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

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
        "    if read.truncated {",
        "    if false {",
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
        # Not FAILING since 2026-10-09: its folder in the file's place fails
        # the read a save now does first (§1239), before any write.
        ["a_save_that_cannot_be_written_says_so"],
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
        "every new id is the same",
        "        Self(recordfile::fresh_id(|_| false))",
        "        Self(1)",
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
        "        self.last_stamp = self.last_stamp.max(latest);\n",
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
    # No row for "a command's letter is typed into the input line": since
    # 2026-10-04 the box's typing is textline::apply_key's, which tells a
    # command from AltGr itself, in its own crate and with its own tests;
    # each_kind_of_key_is_asked_for_as_itself still holds the box to it.
    (
        'a chord works the input line',
        '    if !textline::is_plain(key.modifiers) {\n        return input_key(app, key);\n    }\n',
        '',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
]

FIELD = "the_input_dialogs_box_is_the_toolkits_field"
CARET = "the_input_dialogs_caret_follows_the_typing"
EDITS = "the_input_box_edits_at_a_caret"
FROM_END = "a_title_to_edit_is_edited_from_its_end"
SHOWN = "the_input_box_edits_the_text_it_shows"

MUTATIONS += [
    # The input dialog's box is the toolkit's field (2026-10-04; lane C,
    # c-e-a-theme-can-shape-the-controls).
    (
        "the dialog's box never has the keyboard",
        "        focused: !app.show_help,\n",
        "        focused: false,\n",
        [FIELD, CARET],
    ),
    (
        "the dialog's box keeps its mark under the card",
        "        focused: !app.show_help,\n",
        "        focused: true,\n",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
    (
        "the caret is at the start of the typing",
        "            cursor: input_cursor(app),\n",
        "            cursor: text::TextCursor::from(0),\n",
        [CARET, EDITS],
    ),
]

# The input dialog's box edits at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): it
# took typing at its end and Backspace from it, and nothing else.
MUTATIONS += [
    (
        "a cut or a copy takes nothing to the clipboard",
        "        app.input_clipboard = copied;\n",
        "        let _ = copied;\n",
        [EDITS],
    ),
    (
        "a caret moved is not drawn",
        "    before\n        != (\n            app.input_editor.cursor(),\n            app.input_editor.selection_anchor(),\n        )\n}\n",
        "    false\n}\n",
        [FROM_END],
    ),
    (
        "a key that changes nothing is a redraw",
        "    before\n        != (\n            app.input_editor.cursor(),\n            app.input_editor.selection_anchor(),\n        )\n}\n",
        "    edit.handled\n}\n",
        [FROM_END],
    ),
    (
        "a key finds the editor holding another text",
        "fn input_key(app: &mut KanbanApp, key: &KeyEvent) -> bool {\n    if app.input_editor.text() != app.input_buffer {\n",
        "fn input_key(app: &mut KanbanApp, key: &KeyEvent) -> bool {\n    if false {\n",
        [SHOWN],
    ),
    (
        "a press finds the editor holding another text",
        "    let drawn = input_cursor(app);\n    if app.input_editor.text() != app.input_buffer {\n",
        "    let drawn = input_cursor(app);\n    if false {\n",
        [SHOWN],
    ),
    (
        "a press puts the caret at the start",
        "        x - rect.x - INPUT_TEXT_INSET,\n",
        "        0.0,\n",
        [EDITS],
    ),
    (
        "a press in the box does nothing",
        "                press_input(self, m.x);\n",
        "",
        [EDITS, SHOWN],
    ),
    (
        "a press reaches the box under the list of keys",
        "                    && !self.show_help\n",
        "",
        [FROM_END],
    ),
    (
        "a press with no dialog up is taken",
        "                    && self.input_mode != InputMode::None\n",
        "",
        [FROM_END],
    ),
    (
        "the selection is not drawn",
        "            selection_anchor: if app.input_editor.text() == app.input_buffer {\n",
        "            selection_anchor: if false {\n",
        [EDITS],
    ),
]

TWO = "two_windows_each_add_a_card_and_both_are_kept"
MOVE_EDIT = "a_card_moved_in_one_window_and_changed_in_the_other_is_both"
HEAR = "a_window_hears_another_windows_save"
CHOSEN_GONE = "what_another_window_took_away_is_no_longer_chosen"
BOARD_UP = "the_board_up_stays_up_when_another_window_deletes_one_before_it"
UNREADABLE = "a_save_leaves_a_file_it_cannot_read_as_it_is"
STAYS_DELETED = "a_card_deleted_in_another_window_stays_deleted"
ID_CLASH = "a_new_card_with_an_id_the_board_has_replaces_nothing"
NO_SUCH_COLUMN = "test_board_add_card_invalid_column"
FIRST_RUN = "two_first_run_windows_share_the_starting_board"
FILE_DELETED = "a_boards_file_deleted_while_open_is_written_back_whole"

# Two windows of the board (2026-10-09, design-decisions §1239).
MUTATIONS += [
    (
        "a save writes this window's boards over the file",
        "        let merged = merge::merge_boards(&self.base, &self.boards, &theirs);\n",
        "        let merged = self.boards.clone();\n        let _ = &theirs;\n",
        [TWO, MOVE_EDIT],
    ),
    (
        "a save takes the file for unchanged without asking",
        "        let read = if stamp_now.is_some() && stamp_now == self.file_stamp {",
        "        let read = if true {",
        [TWO],
    ),
    (
        "a save over a file it cannot read goes ahead",
        "            read_boards_file(&path, MAX_BOARDS_BYTES)\n        };",
        "            read_boards_file(&path, MAX_BOARDS_BYTES).or(Ok::<_, String>(None))\n        };",
        [UNREADABLE],
    ),
    (
        "a save does not move on what it compares with",
        "                self.base = merged.clone();\n",
        "",
        [STAYS_DELETED],
    ),
    (
        "a save puts up the board of the same number",
        "        let active = self.active_in(&merged);",
        "        let active = self.active_board_idx;",
        [BOARD_UP],
    ),
    # Not "its own save is taken for another's" (`if false`): reading the
    # file again then finds what the window has, and changes nothing anyone
    # can see -- the stamp saves a read, it decides nothing.
    (
        "another window's save is not heard",
        "        if now == self.file_stamp {",
        "        if true {",
        [HEAR],
    ),
    (
        "a reread throws away what is not saved",
        "            merge::merge_boards(&self.base, &self.boards, &theirs)\n        } else {",
        "            theirs.clone()\n        } else {",
        [HEAR],
    ),
    (
        "a reread with nothing unsaved leaves something to save",
        "            self.kept_text = boards_text(&self.boards, self.active_board_idx);\n        }\n        changed",
        "        }\n        changed",
        [HEAR],
    ),
    (
        "a reread does not move on what the next save compares with",
        "        self.base = theirs;\n",
        "",
        [STAYS_DELETED],
    ),
    (
        "a reread puts up the board of the same number",
        "            .and_then(|up| boards.iter().position(|b| b.id == up.id))",
        "            .map(|_| self.active_board_idx)",
        [BOARD_UP],
    ),
    (
        "a card another window deleted stays chosen",
        "            self.selected_card = None;\n        }\n        let columns",
        "        }\n        let columns",
        [CHOSEN_GONE],
    ),
    (
        "a column another window took away stays chosen",
        "        self.selected_column = self.selected_column.min(columns.saturating_sub(1));\n",
        "",
        [CHOSEN_GONE],
    ),
    (
        "a new card keeps an id the board has",
        "        card.id = self.unclaimed(card.id);\n",
        "",
        [ID_CLASH],
    ),
    (
        "a new card's id is checked against the cards only, not the columns",
        "                || self.columns.iter().any(|c| c.id == raw)\n",
        "",
        [ID_CLASH],
    ),
    (
        "a new card's id is checked against the cards only, not the labels",
        "                || self.labels.iter().any(|l| l.id == raw)\n",
        "",
        [ID_CLASH],
    ),
    (
        "the starting board takes numbers of its own in each window",
        "            last = last.saturating_add(1);\n            Id::from_stored(last)\n",
        "            let _ = last;\n            Id::new()\n",
        [FIRST_RUN],
    ),
    (
        "a first run's changes are measured from nothing",
        "            Ok(None) => self.base = self.boards.clone(),",
        "            Ok(None) => {}",
        [FIRST_RUN],
    ),
    (
        "a missing file is taken for an empty one",
        "            Ok(None) => self.base.clone(),",
        "            Ok(None) => Vec::new(),",
        [FILE_DELETED],
    ),
    (
        "a card for a column there is not is kept, in no column",
        "        self.columns.get_mut(column_idx)?.card_ids.push(card_id);\n        self.cards.insert(card_id, card);",
        "        self.cards.insert(card_id, card);\n        self.columns.get_mut(column_idx)?.card_ids.push(card_id);",
        [NO_SUCH_COLUMN],
    ),
]

DIFFERENT_CARD = "each_windows_change_to_a_different_card_stands"
OWN_FIELDS = "each_windows_change_to_a_boards_own_fields_stands"
THEIR_COLUMN = "a_card_the_other_window_added_is_in_its_column"
ARCHIVE_ORDER = "the_archive_keeps_the_other_windows_order"
LABEL_BACK = "a_label_deleted_elsewhere_comes_back_for_a_card_that_wears_it"
LABEL_OFF = "a_label_neither_window_has_is_taken_off_the_card"
ARCHIVED_MOVED = "a_card_archived_in_one_window_and_moved_in_the_other_is_only_archived"
TWO_COLUMNS = "a_card_in_two_columns_stays_in_this_windows"
COLUMN_ONCE = "a_column_names_each_card_the_board_has_once"
HOMELESS = "a_card_in_no_column_goes_where_this_window_has_it_or_to_the_first"
NO_COLUMN_LEFT = "a_card_on_a_board_with_no_column_left_is_archived"
ARCHIVE_ONCE = "the_archive_names_each_archived_card_once"

# src/merge.rs: a board both windows have is merged inside, and what a merge
# taken piece by piece leaves is mended.
MERGE = [
    (
        "a board both windows changed is taken whole from the later save",
        "            *board = merge_board(b, m, t);",
        "            let _ = (b, m, t);",
        [DIFFERENT_CARD, OWN_FIELDS],
    ),
    (
        "a field is always this window's",
        "    if mine == base {\n        theirs.clone()",
        "    if false {\n        theirs.clone()",
        [OWN_FIELDS],
    ),
    (
        "a column's name is the other window's",
        "                name: pick(&b.name, &m.name, &t.name),",
        "                name: t.name.clone(),",
        [OWN_FIELDS],
    ),
    (
        "a column's limit is this window's",
        "                wip_limit: pick(&b.wip_limit, &m.wip_limit, &t.wip_limit),",
        "                wip_limit: m.wip_limit,",
        [OWN_FIELDS],
    ),
    (
        "a column's order is this window's",
        "                sort_by: pick(&b.sort_by, &m.sort_by, &t.sort_by),",
        "                sort_by: m.sort_by,",
        [OWN_FIELDS],
    ),
    (
        "a column's folding is this window's",
        "                collapsed: pick(&b.collapsed, &m.collapsed, &t.collapsed),",
        "                collapsed: m.collapsed,",
        [OWN_FIELDS],
    ),
    (
        "a board's name is the other window's",
        "        name: pick(&base.name, &mine.name, &theirs.name),",
        "        name: theirs.name.clone(),",
        [OWN_FIELDS],
    ),
    (
        "a board's swimlanes switch is this window's",
        "        swimlanes_enabled: pick(\n            &base.swimlanes_enabled,\n            &mine.swimlanes_enabled,\n            &theirs.swimlanes_enabled,\n        ),",
        "        swimlanes_enabled: mine.swimlanes_enabled,",
        [OWN_FIELDS],
    ),
    (
        "a board's swimlanes are this window's",
        "        swimlane_names: pick(\n            &base.swimlane_names,\n            &mine.swimlane_names,\n            &theirs.swimlane_names,\n        ),",
        "        swimlane_names: mine.swimlane_names.clone(),",
        [OWN_FIELDS],
    ),
    (
        "a column's cards are this window's",
        "                card_ids: merge_ids(&b.card_ids, &m.card_ids, &t.card_ids),",
        "                card_ids: m.card_ids.clone(),",
        [THEIR_COLUMN],
    ),
    (
        "a list of ids is this window's",
        "    recordfile::merge(&entries(base), &entries(mine), &entries(theirs))",
        "    recordfile::merge(&entries(base), &entries(mine), &entries(mine))",
        [THEIR_COLUMN],
    ),
    (
        "the cards are this window's",
        "    recordfile::merge(&listed(base), &listed(mine), &listed(theirs))",
        "    recordfile::merge(&listed(base), &listed(mine), &listed(mine))",
        [DIFFERENT_CARD],
    ),
    (
        "the labels are this window's",
        "        labels: recordfile::merge(&base.labels, &mine.labels, &theirs.labels),",
        "        labels: mine.labels.clone(),",
        [OWN_FIELDS],
    ),
    (
        "the archive is this window's",
        "        archived_card_ids: merge_ids(\n            &base.archived_card_ids,\n            &mine.archived_card_ids,\n            &theirs.archived_card_ids,\n        ),",
        "        archived_card_ids: mine.archived_card_ids.clone(),",
        [ARCHIVE_ORDER],
    ),
    (
        "a label the other window deleted stays deleted under a card",
        "        .extend(mine.labels.iter().filter(|l| worn.contains(&l.id)).cloned());",
        "        .extend(mine.labels.iter().filter(|_| false).cloned());",
        [LABEL_BACK],
    ),
    (
        "a card wears a label the board does not have",
        "        card.labels.retain(|id| labels.contains(id));",
        "        card.labels.retain(|_| true);",
        [LABEL_OFF],
    ),
    (
        "an archived card stays in a column",
        "        .filter(|card| !card.archived)\n        .map(|card| card.id)\n        .collect();\n    let mine_column",
        "        .filter(|_| true)\n        .map(|card| card.id)\n        .collect();\n    let mine_column",
        [ARCHIVED_MOVED],
    ),
    (
        "a card in two columns stays in the first, not this window's",
        "                Some(_) => mine_column(*card) == Some(column.id),",
        "                Some(_) => false,",
        [TWO_COLUMNS],
    ),
    (
        "a column names a card twice",
        "            .retain(|card| placed.get(card) == Some(&here) && seen.insert(*card));",
        "            .retain(|card| placed.get(card) == Some(&here) && (seen.insert(*card) || true));",
        [COLUMN_ONCE],
    ),
    (
        "a card in no column goes to the first, not where this window has it",
        "        let target = mine_column(card)\n            .filter(|col| board.columns.iter().any(|c| c.id == *col))\n            .or_else(",
        "        let target = None\n            .or_else(",
        [HOMELESS],
    ),
    (
        "a card whose column is gone is looked for there",
        "            .filter(|col| board.columns.iter().any(|c| c.id == *col))\n",
        "",
        [HOMELESS],
    ),
    (
        "a card with no column left is kept where nothing shows it",
        "                    card.archived = true;",
        "                    let _ = card;",
        [NO_COLUMN_LEFT],
    ),
    (
        "the archive names a card that is not archived",
        "        .retain(|id| archived.contains(id) && seen.insert(*id));",
        "        .retain(|id| seen.insert(*id));",
        [ARCHIVE_ONCE],
    ),
    (
        "the archive leaves out an archived card",
        "    board.archived_card_ids.extend(missing);",
        "    drop(missing);",
        [ARCHIVE_ONCE],
    ),
]

TABLES = {
    "main.rs": MUTATIONS,
    "merge.rs": MERGE,
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
        worst = max(worst, sweep(SRC / file, rows, "kanban", timeout=900, only=mine or None))
    raise SystemExit(worst)
