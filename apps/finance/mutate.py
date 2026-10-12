"""Mutation test for finance's forms, questions, lists, pointer layer and ledger.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Nothing could be entered -- `add_account`, `add_transaction` and `set_budget`
had no caller outside the tests -- nothing was kept, "today" was 18 May 2026 in
every run, and nothing answered the pointer (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED and
TD-C-FINANCE-IS-A-VIEWER-OVER-SAMPLE-DATA).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- the keys ------------------------------------------------------------------------------------------
    (
        "N opens no form",
        '            "n" | "N" if !ctrl => {',
        '            "n" | "N" if false => {',
        [
            "a_transaction_is_entered_from_the_keyboard",
            "an_account_is_entered_with_what_it_held",
            "every_advertised_key_does_something",
        ],
    ),
    (
        "N on the accounts screen makes a transaction",
        "                if self.screen == Screen::Accounts {\n"
        "                    self.open_new_account();",
        "                if false {\n"
        "                    self.open_new_account();",
        ["n_on_the_accounts_screen_adds_an_account"],
    ),
    (
        "Enter changes nothing",
        '            "Return" => self.edit_chosen(),',
        '            "Return" => {}',
        ["a_transaction_is_changed_in_place", "every_category_can_be_given_a_budget"],
    ),
    (
        "Delete asks nothing",
        '            "Delete" => self.ask_to_delete(),',
        '            "Delete" => {}',
        ["ctrl_d_asks_before_deleting_and_only_y_deletes", "deleting_is_kept_too"],
    ),
    (
        "Ctrl+D asks nothing",
        '            "d" if ctrl => self.ask_to_delete(),',
        '            "d" if ctrl => {}',
        ["ctrl_d_asks_before_deleting_and_only_y_deletes"],
    ),
    (
        "any key answers the question yes",
        "                key.single_char().map_or(key.key == Key::Y, typed_y)",
        "                key.single_char().map_or(true, |_| true)",
        ["ctrl_d_asks_before_deleting_and_only_y_deletes"],
    ),
    (
        "a form lets keys through to the screen behind",
        "        if self.form.is_some() {\n"
        "            return self.handle_form_key(key);",
        "        if false {\n"
        "            return self.handle_form_key(key);",
        [
            "a_digit_typed_into_a_form_is_not_a_screen_switch",
            "a_transaction_is_entered_from_the_keyboard",
        ],
    ),
    (
        "Shift+Tab walks forward",
        "                    at.checked_sub(1).unwrap_or(fields.len().saturating_sub(1))",
        "                    at.saturating_add(1).checked_rem(fields.len()).unwrap_or(0)",
        ["tab_walks_the_fields_and_comes_round"],
    ),
    (
        "a field takes no typing",
        "                let done = textline::apply_key(input, key, 200, &clipboard, 13.0);",
        "                let done = textline::apply_key(&mut TextInput::new(), key, 200, &clipboard, 13.0);\n"
        "                let _ = input;",
        [
            "a_transaction_is_entered_from_the_keyboard",
            "a_form_says_what_is_wrong_and_keeps_what_was_typed",
        ],
    ),
    # -- what a form accepts ---------------------------------------------------------------------------------------
    (
        "money in stays under Food",
        "                if *income && Category::EXPENSE_CATS.contains(category) {",
        "                if false {",
        ["money_in_is_not_filed_under_food", "income_is_money_in"],
    ),
    (
        "an expense is kept as money in",
        "            magnitude.saturating_neg()\n"
        "        };",
        "            magnitude\n"
        "        };",
        ["a_transaction_is_entered_from_the_keyboard"],
    ),
    (
        "a transaction with no description is kept",
        "        if description.is_empty() {",
        "        if false {",
        ["a_form_says_what_is_wrong_and_keeps_what_was_typed"],
    ),
    (
        "a transaction of nothing is kept",
        "        if magnitude == 0 {",
        "        if false {",
        ["a_zero_amount_is_refused"],
    ),
    (
        "three decimals are taken",
        "    if frac.len() > 2 {",
        "    if false {",
        [
            "amounts_are_read_as_people_write_them",
            "a_form_says_what_is_wrong_and_keeps_what_was_typed",
        ],
    ),
    (
        "a sign is taken where Income or Expense says the direction",
        "        if !negative {\n"
        "            return Err(String::from(",
        "        if false {\n"
        "            return Err(String::from(",
        ["amounts_are_read_as_people_write_them"],
    ),
    (
        "an impossible date is taken",
        "        if u32::from(day) > guitk::date::days_in_month(i32::from(year), u32::from(month)) {",
        "        if false {",
        [
            "dates_are_read_back_and_impossible_ones_refused",
            "a_form_says_what_is_wrong_and_keeps_what_was_typed",
        ],
    ),
    (
        "an account form drops the opening balance",
        "                let id = self.add_account(&name, kind, opening);",
        "                let id = self.add_account(&name, kind, 0);\n"
        "                let _ = opening;",
        ["an_account_is_entered_with_what_it_held"],
    ),
    (
        "the budget form sets nothing",
        "                    self.set_budget(category, cents);",
        "                    let _ = (category, cents);",
        ["every_category_can_be_given_a_budget"],
    ),
    (
        "an emptied budget stays",
        "        if monthly_limit <= 0 {\n"
        "            self.budgets.retain(|b| b.category != category);",
        "        if monthly_limit < 0 {\n"
        "            self.budgets.retain(|b| b.category != category);",
        ["every_category_can_be_given_a_budget"],
    ),
    # -- deleting ------------------------------------------------------------------------------------------------------
    (
        "a delete on the dashboard asks about a row it does not show",
        "            Screen::Dashboard | Screen::Budgets | Screen::Reports => None,\n"
        "        };",
        "            Screen::Dashboard | Screen::Budgets | Screen::Reports => {\n"
        "                self.selected_id.map(Doomed::Transaction)\n"
        "            }\n"
        "        };",
        ["nothing_is_deleted_from_a_screen_that_does_not_show_it"],
    ),
    (
        "deleting an account leaves its transactions",
        "                self.transactions.retain(|t| t.account_id != id);",
        "                self.transactions.retain(|_| true);",
        ["deleting_an_account_asks_and_takes_its_transactions"],
    ),
    # -- the lists ------------------------------------------------------------------------------------------------------
    (
        "the list is every month",
        "                    return tx.date.same_month(&self.view_month);",
        "                    return true;",
        [
            "the_list_is_the_month_on_screen_newest_first",
            "the_search_box_takes_the_keys_and_reaches_every_month",
        ],
    ),
    (
        "the list is in the order things were typed",
        "        rows.sort_by(|(_, a), (_, b)| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));",
        "",
        ["the_list_is_the_month_on_screen_newest_first"],
    ),
    (
        "the chosen row scrolls off the bottom",
        "                } else if at >= scroll.saturating_add(visible) {",
        "                } else if false {",
        ["the_chosen_row_stays_on_screen", "the_budgets_scroll_in_a_short_window"],
    ),
    (
        "the wheel runs past the end",
        "            now.saturating_add(rows.unsigned_abs())\n"
        "        }\n"
        "        .min(last);",
        "            now.saturating_add(rows.unsigned_abs())\n"
        "        };\n"
        "        let _ = last;",
        ["the_wheel_scrolls_a_long_list"],
    ),
    (
        "the wheel scrolls nothing",
        "        let rows = self.wheel.rows(dy);",
        "        let rows = self.wheel.rows(dy) * 0;",
        ["the_wheel_scrolls_a_long_list"],
    ),
    (
        "a screen is not held to its area",
        "        f.clip(self.content_rect());",
        "        f.clip(Rect::new(0.0, 0.0, w, h));",
        ["the_wheel_scrolls_a_long_list"],
    ),
    (
        "the total runs under the status bar",
        "        let total_y = self.height - Self::STATUS_H - 52.0;",
        "        let total_y = self.height - 60.0;",
        ["the_total_is_above_the_status_bar"],
    ),
    # -- the pointer ----------------------------------------------------------------------------------------------------
    (
        "a press behind a form reaches what is behind it",
        "        f.hit(Target::FormBackdrop, Rect::new(0.0, 0.0, w, h));",
        "",
        ["a_press_behind_a_form_reaches_nothing"],
    ),
    (
        "the question has no backdrop",
        "        f.hit(Target::QuestionBackdrop, Rect::new(0.0, 0.0, w, h));",
        "",
        ["a_press_outside_the_question_keeps_and_reaches_nothing"],
    ),
    (
        "a press goes through the list of keys",
        "            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, w, h));",
        "",
        ["f1_shows_the_keys_and_a_press_puts_them_away"],
    ),
    (
        "a press elsewhere leaves the keys in the search box",
        "        if target != Target::Search {\n"
        "            self.search_active = false;",
        "        if false {\n"
        "            self.search_active = false;",
        ["the_search_box_takes_the_keys_and_reaches_every_month"],
    ),
    (
        "one press on a transaction changes it",
        "                if self.selected_id == Some(id) {",
        "                if true {",
        ["a_row_press_chooses_it_and_a_second_changes_it"],
    ),
    (
        "no press on an account changes it",
        "                if self.selected_account == Some(id) {",
        "                if false {",
        ["an_account_is_changed_in_place"],
    ),
    (
        "no press on a budget sets it",
        "                if self.selected_budget == i {",
        "                if false {",
        ["every_category_can_be_given_a_budget"],
    ),
    (
        "a recent transaction does nothing",
        "            Target::RecentRow(id) => self.show_transaction(id),",
        "            Target::RecentRow(_) => {}",
        ["a_recent_transaction_opens_in_the_list"],
    ),
    (
        "the back arrow steps forward",
        "                self.step_choice(field, false);",
        "                self.step_choice(field, true);",
        ["a_form_answers_the_pointer"],
    ),
    (
        "This month is offered on this month",
        "            !self.view_month.same_month(&self.current_date),",
        "            true,",
        ["the_month_arrows_are_buttons"],
    ),
    # -- the clock ------------------------------------------------------------------------------------------------------
    (
        "midnight never comes",
        "                if today == self.current_date {",
        "                if true {",
        ["midnight_moves_today_and_the_month_on_screen_with_it"],
    ),
    (
        "the view stays on the old month at midnight",
        "                if self.view_month.same_month(&self.current_date) {",
        "                if false {",
        ["midnight_moves_today_and_the_month_on_screen_with_it"],
    ),
    (
        "the window never wakes",
        "        let left = clock_now().map_or(3600, |(_, left)| left.saturating_add(1));\n"
        "        Some(Duration::from_secs(left.min(3600)))",
        "        None",
        ["a_tick_on_the_same_day_draws_nothing"],
    ),
    # -- the ledger -----------------------------------------------------------------------------------------------------
    (
        "nothing is kept",
        "        self.keep_lists_in_view();\n        self.save_ledger();",
        "        self.keep_lists_in_view();",
        [
            "what_is_entered_is_there_next_time",
            "deleting_is_kept_too",
            "a_save_that_fails_says_so_and_the_next_one_clears_it",
        ],
    ),
    (
        "a window made by new keeps everything",
        "        if !self.persist {\n"
        "            return;\n"
        "        }\n"
        "        let Some(path) = ledger_path() else {",
        "        if false {\n"
        "            return;\n"
        "        }\n"
        "        let Some(path) = ledger_path() else {",
        ["a_window_made_by_new_keeps_nothing"],
    ),
    (
        # Each save reads the file again first and refuses to write over one
        # it cannot read, so the file is safe either way; what this line
        # does is stop the window trying -- and asking at the close about
        # changes it said from the start would not be kept.
        "a window that could not read its ledger goes on trying to keep it",
        "            Err(why) => {\n"
        "                self.persist = false;\n"
        "                self.ledger_error = Some(format!(\n"
        '                    "{} was not read ({why}), so nothing is saved over it",',
        "            Err(why) => {\n"
        "                self.ledger_error = Some(format!(\n"
        '                    "{} was not read ({why}), so nothing is saved over it",',
        ["a_ledger_that_cannot_be_read_is_left_as_it_is"],
    ),
    (
        "a failed save says nothing",
        '                self.ledger_error = Some(format!("Not saved to {}: {err}", path.shown()));',
        "                drop(err);",
        # Not the folder-where-the-file-goes test since 2026-10-09: that fails
        # the read a save now does first (§1239), before any write.
        ["a_save_that_cannot_be_written_says_so"],
    ),
    (
        "a tab in a description splits its line",
        "            tsv::escape(&t.description),",
        "            t.description.clone(),",
        ["the_ledger_reads_back_what_it_wrote_whatever_the_text"],
    ),
    (
        "a transaction in a missing account is read",
        "        if !ledger.accounts.iter().any(|a| a.id == tx.account_id) {",
        "        if false {",
        ["a_ledger_is_refused_whole_and_says_where"],
    ),
    (
        "two accounts with one number are read",
        '                    return Err(bad("two accounts have one number"));',
        "",
        ["a_ledger_is_refused_whole_and_says_where"],
    ),
    (
        "a new transaction's number counts up from the largest",
        "        let id = recordfile::fresh_id(|id| self.transactions.iter().any(|t| t.id == id));",
        "        let id = self.transactions.iter().map(|t| t.id).max().map_or(1, |m| m.saturating_add(1));",
        ["two_windows_each_enter_a_transaction_and_both_are_kept"],
    ),
    (
        "a chord works a form's own keys",
        '        let plain = textline::is_plain(key.modifiers);\n        match key.key {\n            Key::Tab if plain => {',
        '        let plain = true;\n        match key.key {\n            Key::Tab if plain => {',
        ['a_chord_is_neither_a_finance_key_nor_typing'],
    ),
    (
        'a chord raises the keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['a_chord_is_neither_a_finance_key_nor_typing'],
    ),
    (
        'a chorded Y answers the delete question',
        '                textline::types_into_field(key) && key.single_char().is_some_and(typed_y)',
        '                key.single_char().is_some_and(typed_y)',
        ['a_chord_is_neither_a_finance_key_nor_typing'],
    ),
    (
        "a command's letter is typed into the search",
        '        if self.search_active && textline::types_into_field(key) {',
        '        if self.search_active && !key.text.is_empty() {',
        ['a_chord_is_neither_a_finance_key_nor_typing'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        let ctrl = textline::is_ctrl_chord(key.modifiers);',
        '        let ctrl = key.modifiers.ctrl;',
        ['a_chord_is_neither_a_finance_key_nor_typing'],
    ),
    (
        'a Ctrl chord works a plain shortcut',
        '        if ctrl && key.key != Key::D {',
        '        if false {',
        ['a_chord_is_neither_a_finance_key_nor_typing'],
    ),
    (
        "Alt's and the Windows key's chords work the shortcuts",
        '        if !ctrl && !plain && !textline::types_into_field(key) {',
        '        if false {',
        ['a_chord_is_neither_a_finance_key_nor_typing'],
    ),
    # -- the text boxes, the toolkit's fields (c-e-a-theme-can-shape-the-controls)
    (
        'the search box is drawn the same wherever the pointer is',
        '                hovered: self.hover == Some(Target::Search),',
        '                hovered: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        'the search box is not marked while it has the keyboard',
        '                focused: self.search_active,',
        '                focused: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        'a form field is not marked while it has the keyboard',
        '                hovered: self.hover == Some(Target::Field(field)),\n                focused,',
        '                hovered: self.hover == Some(Target::Field(field)),\n                focused: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        "the text boxes take the toolkit's focus width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();',
        '        let _ = settings;',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
]

# Two windows of Finance save into one ledger (2026-10-09, design-decisions
# §1239): each wrote its own copy over the other's, so the last to save threw
# away the other's entries.
TWO = "two_windows_each_enter_a_transaction_and_both_are_kept"
HEAR = "a_window_hears_another_windows_save"
UNSAVED = "a_change_not_yet_saved_survives_hearing_another_save"
STAYS_DELETED = "a_transaction_deleted_in_another_window_stays_deleted"
BUDGETS = "budgets_set_in_two_windows_are_kept_by_category"
ACCOUNT_BACK = "an_account_deleted_while_another_window_used_it_comes_back"
FORM_DELETED = "a_transaction_deleted_elsewhere_while_changed_here_is_saved_back"
CHOSEN_GONE = "what_another_window_deleted_is_no_longer_chosen"
UNREADABLE = "a_save_leaves_a_file_it_cannot_read_as_it_is"
FILE_DELETED = "a_ledger_file_deleted_while_open_is_written_back_whole"

MUTATIONS += [
    (
        "a save writes this window's ledger over the file",
        "        let merged = merge_ledgers(&self.base, &self.ledger(), &theirs);\n",
        "        let merged = self.ledger();\n        let _ = &theirs;\n",
        [TWO],
    ),
    (
        "a save takes the file for unchanged without asking",
        "        let read = if stamp_now.is_some() && stamp_now == self.file_stamp {",
        "        let read = if true {",
        [TWO],
    ),
    (
        "a missing file is taken for an empty one",
        "            Ok(None) => self.base.clone(),",
        "            Ok(None) => Ledger::default(),",
        [FILE_DELETED],
    ),
    (
        "a save over a file it cannot read goes ahead",
        "            read_ledger_file(&path)\n        };",
        "            read_ledger_file(&path).or(Ok::<_, String>(None))\n        };",
        [UNREADABLE],
    ),
    (
        "a save does not move on what it compares with",
        "                self.base.clone_from(&merged);\n",
        "",
        [STAYS_DELETED],
    ),
    (
        "an account's number counts up from the largest",
        "        let id = recordfile::fresh_id(|id| self.accounts.iter().any(|a| a.id == id));",
        "        let id = self.accounts.iter().map(|a| a.id).max().map_or(1, |m| m.saturating_add(1));",
        [TWO],
    ),
    (
        "another window's save is not heard",
        "        if now == self.file_stamp {",
        "        if true {",
        [HEAR],
    ),
    (
        "a reread throws away what is not saved",
        "        let shown = if mine == self.base {",
        "        let shown = if true {",
        [UNSAVED],
    ),
    (
        "a reread does not move on what the next save compares with",
        "        self.base = theirs;\n",
        "",
        [STAYS_DELETED],
    ),
    (
        # Not ACCOUNT_BACK: the account a kept transaction is in is put back
        # by the mend after the merge, from either side, so taking this
        # window's accounts whole loses only the other window's new ones.
        "the accounts are this window's",
        "    let mut accounts = recordfile::merge(&base.accounts, &mine.accounts, &theirs.accounts);",
        "    let mut accounts = mine.accounts.clone();",
        [TWO],
    ),
    (
        "the transactions are this window's",
        "        recordfile::merge(&base.transactions, &mine.transactions, &theirs.transactions);",
        "        mine.transactions.clone();",
        [TWO],
    ),
    (
        "the budgets are this window's",
        "        budgets: recordfile::merge_by(&base.budgets, &mine.budgets, &theirs.budgets, |b| {\n            b.category\n        }),",
        "        budgets: mine.budgets.clone(),",
        [BUDGETS],
    ),
    (
        "an account a kept transaction is in stays deleted",
        "            .filter(|a| needed.contains(&a.id))\n            .chain(",
        "            .filter(|_| false)\n            .chain(",
        [ACCOUNT_BACK],
    ),
    (
        "an account deleted here is looked for in this window's copy alone",
        "                theirs.accounts.iter().filter(|a| {\n                    needed.contains(&a.id) && !mine.accounts.iter().any(|m| m.id == a.id)\n                }),",
        "                theirs.accounts.iter().filter(|_| false),",
        [ACCOUNT_BACK],
    ),
    (
        "a change to a transaction deleted elsewhere is dropped",
        "                    None => self.transactions.push(changed),",
        "                    None => drop(changed),",
        [FORM_DELETED],
    ),
    (
        "an account another window deleted stays chosen",
        "            .is_some_and(|id| !self.accounts.iter().any(|a| a.id == id))\n        {\n            self.selected_account",
        "            .is_some_and(|_| false)\n        {\n            self.selected_account",
        [CHOSEN_GONE],
    ),
    (
        "a delete waits on a transaction another window deleted",
        "            Some(Doomed::Transaction(id)) => !self.transactions.iter().any(|t| t.id == id),",
        "            Some(Doomed::Transaction(_)) => false,",
        [CHOSEN_GONE],
    ),
    (
        "a delete waits on an account another window deleted",
        "            Some(Doomed::Account(id)) => !self.accounts.iter().any(|a| a.id == id),",
        "            Some(Doomed::Account(_)) => false,",
        [CHOSEN_GONE],
    ),
]

# Closing while a save is failing asks first (2026-10-10): the window went at
# once, and the change the failing save held went with it -- the only one of
# the programs that keep records to do so.
CLOSING = "closing_while_a_save_fails_asks_first"
DISCARD = "closing_without_saving_goes_without_the_change"
BROKEN_LEFT = "a_ledger_that_cannot_be_read_is_left_as_it_is"

MUTATIONS += [
    (
        "a close never asks",
        "            return if self.request_close() {\n"
        "                Response::Exit\n"
        "            } else {\n"
        "                Response::KeepOpen\n"
        "            };",
        "            return Response::Exit;",
        [CLOSING, DISCARD],
    ),
    (
        "a close with everything kept asks",
        "        if !self.unkept() {\n            return true;\n        }\n        let detail",
        "        let detail",
        [CLOSING],
    ),
    (
        "a close does not try the save again first",
        "        if self.unkept() {\n            self.save_ledger();\n        }\n",
        "",
        [CLOSING],
    ),
    (
        "a window that keeps nothing asks at the close",
        "        self.persist && self.ledger() != self.base",
        "        self.ledger() != self.base",
        [BROKEN_LEFT],
    ),
    (
        "the question does not say why",
        '            &format!("{detail} -- try saving again before closing?"),',
        '            &format!("{} -- try saving again before closing?", detail.len()),',
        [CLOSING],
    ),
    (
        "Save at the question goes even when the save fails",
        "                self.save_ledger();\n                !self.unkept()",
        "                self.save_ledger();\n                true",
        [CLOSING],
    ),
    (
        "Don't save stays",
        "            unsaved::Choice::Discard => true,",
        "            unsaved::Choice::Discard => false,",
        [DISCARD],
    ),
    (
        "Cancel goes",
        "            unsaved::Choice::Cancel => false,",
        "            unsaved::Choice::Cancel => true,",
        [CLOSING],
    ),
    (
        "a key under the question reaches the ledger",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))\n"
        "        {",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Mouse(_))\n"
        "        {",
        [CLOSING, DISCARD],
    ),
    (
        "the question is not drawn",
        "            question.render(&palette, width, height, &mut tree);",
        "            let _ = (question, palette);",
        [CLOSING],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "finance", timeout=900, only=only))
