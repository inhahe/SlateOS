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
        "                .map_or(key.key == Key::Y, |c| c.eq_ignore_ascii_case(&'y'));",
        "                .map_or(true, |_| true);",
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
        "                let done = edit_line(input, key, 200, &clipboard);",
        "                let done = edit_line(&mut TextInput::new(), key, 200, &clipboard);\n"
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
        "        self.save_ledger();",
        "",
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
        "an unreadable ledger is saved over",
        "            Err(why) => {\n"
        "                self.persist = false;\n"
        "                self.ledger_error = Some(refused(why));",
        "            Err(why) => {\n"
        "                self.ledger_error = Some(refused(why));",
        ["a_ledger_that_cannot_be_read_is_left_as_it_is"],
    ),
    (
        "a failed save says nothing",
        '            Err(err) => Some(format!("Not saved to {}: {err}", path.display())),',
        "            Err(_) => None,",
        ["a_save_that_fails_says_so_and_the_next_one_clears_it"],
    ),
    (
        "a tab in a name splits its line",
        "            '\\t' => out.push_str(\"\\\\t\"),",
        "            '\\t' => out.push('\\t'),",
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
        "a new transaction reuses a kept one's number",
        "                    .map(|t| t.id.saturating_add(1))",
        "                    .map(|_| 1)",
        ["what_is_entered_is_there_next_time"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "finance", timeout=900, only=only))
