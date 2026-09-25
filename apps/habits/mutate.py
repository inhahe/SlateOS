"""Mutation test for the habit tracker's clock, pointer layer, lists and keeping.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The tracker drew four tabs, a button, a row and seven day cells per habit, an
archive and a form, and handled no pointer event (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED).  "Today" was
18 May 2026 in every run, moved by two keys; nothing was kept between runs;
Ctrl+D deleted a habit without asking; the archive and the statistics table
stopped at the window's bottom edge; and a space could not be typed in a name.

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
    # -- the clock --------------------------------------------------------------------------------
    (
        "today is a fixed day again",
        "        let today =\n"
        "            today_from_clock().unwrap_or_else(|| Date::from_civil(date::Date::from_unix_utc(0)));",
        "        let today = Date {\n            year: 2026,\n            month: 5,\n            day: 18,\n        };",
        ["a_new_tracker_takes_today_from_the_clock"],
    ),
    (
        "a tick never moves today",
        "                self.today = today;\n                true",
        "                let _ = today;\n                true",
        ["a_tick_after_midnight_moves_today_on"],
    ),
    (
        "the window is never woken",
        "        Some(Duration::from_secs(left.min(3600)))",
        "        let _ = left;\n        None",
        ["the_window_asks_to_be_woken_by_midnight"],
    ),
    (
        "+ moves today again",
        '            "a" | "A" if !ctrl && self.screen == Screen::Dashboard => {',
        '            "+" => self.advance_day(),\n'
        '            "a" | "A" if !ctrl && self.screen == Screen::Dashboard => {',
        ["the_keys_that_moved_today_are_gone"],
    ),
    # -- what the window says ----------------------------------------------------------------------
    (
        "the notice is drawn under the header again",
        "                    y: start_y + 60.0 + i as f32 * 24.0,",
        "                    y: 2.0 + i as f32 * 12.0,",
        ["an_empty_tracker_says_so_where_the_habits_would_be"],
    ),
    (
        "the header names the day twice",
        '                "{} {}",\n'
        "                self.today.day_of_week_short(),\n"
        "                self.today.format_full()\n",
        '                "{} {} -- {}",\n'
        "                self.today.day_of_week_short(),\n"
        "                self.today.format_full(),\n"
        "                self.today.day_of_week_short()\n",
        ["the_header_names_the_day_once"],
    ),
    # -- deleting asks -------------------------------------------------------------------------------
    (
        "a delete does not ask",
        "        self.pending_delete = Some(idx);",
        "        self.delete_habit(idx);",
        [
            "ctrl_d_asks_and_only_y_deletes",
            "an_archived_row_restores_and_deletes",
            "the_delete_question_answers_to_the_pointer",
        ],
    ),
    (
        "any key answers yes",
        '            if key.eq_ignore_ascii_case("y") {',
        "            if !key.is_empty() {",
        ["ctrl_d_asks_and_only_y_deletes"],
    ),
    (
        "Enter answers yes",
        '            if key.eq_ignore_ascii_case("y") {',
        '            if key.eq_ignore_ascii_case("y") || key == "Return" {',
        ["ctrl_d_asks_and_only_y_deletes"],
    ),
    (
        "Ctrl+D in the archive looks in the dashboard's list",
        "                let list = if self.screen == Screen::Archive {\n"
        "                    self.archived_habits()",
        "                let list = if false {\n                    self.archived_habits()",
        ["ctrl_d_in_the_archive_deletes_the_archived_habit"],
    ),
    (
        "the question's Delete button records no hit box",
        "        cmds.hit(Target::ConfirmDelete, delete);",
        "",
        ["an_archived_row_restores_and_deletes"],
    ),
    (
        "the question's Keep button is not drawn",
        '        self.button(cmds, keep, "Keep", Target::KeepHabit);',
        "",
        ["the_delete_question_answers_to_the_pointer"],
    ),
    (
        "the question's card answers for the backdrop",
        "        cmds.hit(Target::ConfirmCard, card);",
        "",
        ["the_delete_question_answers_to_the_pointer"],
    ),
    (
        "the question has no backdrop",
        "        cmds.hit(\n            Target::ConfirmBackdrop,\n"
        "            Rect::new(0.0, 0.0, self.width, self.height),\n        );",
        "",
        [
            "nothing_behind_a_question_or_the_form_can_be_pressed",
            "the_delete_question_answers_to_the_pointer",
        ],
    ),
    (
        "an archived row has no Delete button",
        '            self.button(cmds, delete, "Delete", Target::DeleteButton(idx));',
        "",
        ["an_archived_row_restores_and_deletes"],
    ),
    # -- the form --------------------------------------------------------------------------------------
    (
        "the form has no backdrop",
        "        cmds.hit(\n            Target::FormBackdrop,\n"
        "            Rect::new(0.0, 0.0, self.width, self.height),\n        );",
        "",
        ["nothing_behind_a_question_or_the_form_can_be_pressed", "the_form_answers_to_the_pointer"],
    ),
    (
        "F2 no longer switches the frequency",
        '            "F2" => {\n                self.create_frequency_daily = !self.create_frequency_daily;',
        '            "F9" => {\n                self.create_frequency_daily = !self.create_frequency_daily;',
        ["test_create_form_f2_toggles_frequency"],
    ),
    (
        "the weekly count runs past seven",
        "                self.create_weekly_count = self.create_weekly_count.saturating_add(1).min(7);",
        "                self.create_weekly_count = self.create_weekly_count.saturating_add(1);",
        ["test_create_form_up_and_down_set_the_weekly_count"],
    ),
    (
        "the space bar types nothing",
        '            "Space" => self.type_into_name(" "),',
        "",
        ["a_name_can_hold_a_space"],
    ),
    (
        "a blank name is accepted",
        "        if self.create_name.trim().is_empty() {",
        "        if self.create_name.is_empty() {",
        ["a_blank_name_is_refused"],
    ),
    (
        "the count's buttons show for a daily habit",
        "        if !self.create_frequency_daily {",
        "        if true {",
        ["the_weekly_count_is_offered_only_for_a_weekly_habit"],
    ),
    (
        "the category chip does not step",
        '            Target::FormCategory => self.handle_create_form_key("Tab", false),',
        "            Target::FormCategory => {}",
        ["the_form_answers_to_the_pointer"],
    ),
    # -- the pointer on the dashboard -------------------------------------------------------------
    (
        "a tab records no hit box",
        "            cmds.hit(Target::Tab(*tab), Rect::new(tx, y + 4.0, w, 28.0));",
        "",
        ["every_tab_is_a_button"],
    ),
    (
        "the filter chip records no hit box",
        "        cmds.hit(Target::FilterChip, chip);",
        "",
        ["the_filter_chip_is_there_unset_and_steps_through_categories"],
    ),
    (
        "a day cell records no hit box",
        "                    Target::DayCell(habit_idx, col),",
        "                    Target::HabitRow(habit_idx),",
        ["a_day_cell_checks_its_habit_in_on_its_day"],
    ),
    (
        "a day cell checks in today whatever its column",
        "                self.selected_day_col = col.min(6);",
        "                let _ = col;\n                self.selected_day_col = 0;",
        ["a_day_cell_checks_its_habit_in_on_its_day"],
    ),
    (
        "a row press chooses nothing",
        "                return self.select_habit(index);",
        "                let _ = index;\n                return false;",
        ["a_row_press_chooses_its_habit"],
    ),
    (
        "the Archive button does nothing",
        "            Target::ArchiveButton(index) => self.archive_habit(index),",
        "            Target::ArchiveButton(_) => {}",
        ["the_archive_button_archives_its_own_row"],
    ),
    (
        "the Restore button does nothing",
        "            Target::RestoreButton(index) => self.unarchive_habit(index),",
        "            Target::RestoreButton(_) => {}",
        ["an_archived_row_restores_and_deletes", "the_archive_scrolls_to_its_last_habit"],
    ),
    (
        "a statistics row opens the first habit's graph",
        "                self.heatmap_habit_idx = position;",
        "                let _ = position;\n                self.heatmap_habit_idx = 0;",
        ["a_statistics_row_opens_its_graph", "the_statistics_table_scrolls_to_its_last_habit"],
    ),
    (
        "a statistics row records no hit box",
        "            cmds.hit(Target::StatsRow(idx), row);",
        "",
        ["a_statistics_row_opens_its_graph"],
    ),
    (
        "the graph's Next button is not drawn",
        '        self.button(cmds, next, "Next >", Target::GraphNext);',
        "",
        ["the_graph_buttons_step_through_the_habits"],
    ),
    (
        "the pointer lights nothing",
        "                self.hover = over;\n                true",
        "                let _ = over;\n                true",
        ["hovering_a_control_lights_it"],
    ),
    # -- the lists -------------------------------------------------------------------------------------------
    (
        "the dashboard's rows are not clipped under the day heads",
        "        cmds.hit(Target::HabitList, pane);\n        cmds.clip(pane);",
        "        cmds.hit(Target::HabitList, pane);\n"
        "        cmds.clip(Rect::new(0.0, 0.0, self.width, self.height));",
        ["a_row_scrolled_under_the_day_heads_cannot_be_pressed"],
    ),
    (
        "the wheel turns the wrong way",
        "                let rows = self.wheel.rows(dy);",
        "                let rows = self.wheel.rows(-dy);",
        ["the_wheel_scrolls_the_list_it_is_over"],
    ),
    (
        "the wheel scrolls wherever the pointer is",
        "                let over = self.target_at(event.x, event.y);\n                if !matches!(",
        "                let _ = self.target_at(event.x, event.y);\n"
        "                let over = Some(Target::StatsList);\n                if !matches!(",
        ["the_wheel_scrolls_the_list_it_is_over", "the_statistics_table_scrolls_to_its_last_habit"],
    ),
    (
        "the archive stops at the bottom edge again",
        "            let ry = pane.y + 2.0 + vi as f32 * Self::ROW_H - self.scroll_offset;",
        "            let ry = pane.y + 2.0 + vi as f32 * Self::ROW_H;",
        ["the_archive_scrolls_to_its_last_habit"],
    ),
    (
        "the statistics table stops at the bottom edge again",
        "                pane.y + vi as f32 * STATS_ROW_H - self.scroll_offset,",
        "                pane.y + vi as f32 * STATS_ROW_H,",
        ["the_statistics_table_scrolls_to_its_last_habit"],
    ),
    (
        "the scroll limit ignores the list's length",
        "            (rows as f32 * row_h + LIST_END_PAD - pane.h).max(0.0)",
        "            let _ = (pane, row_h, rows);\n            2000.0",
        [
            "page_down_does_not_scroll_a_list_that_fits",
            "page_down_scrolls_a_long_list_to_its_end_and_no_further",
        ],
    ),
    (
        "the arrows leave the view behind",
        "            } else if bottom > self.scroll_offset + pane.h {\n"
        "                self.scroll_offset = bottom - pane.h;\n            }",
        "            }",
        ["the_arrows_keep_the_chosen_habit_on_screen"],
    ),
    (
        "the selection is left past the end of a shorter list",
        "            self.selected_habit = self.selected_habit.min(rows.saturating_sub(1));",
        "",
        ["archiving_the_last_row_leaves_a_habit_chosen"],
    ),
    (
        "the number keys keep the selection",
        "        self.screen = screen;\n        self.selected_habit = 0;\n"
        "        self.scroll_offset = 0.0;\n        true",
        "        self.screen = screen;\n        self.scroll_offset = 0.0;\n        true",
        ["the_number_keys_start_each_screen_at_its_top"],
    ),
    (
        "Page Down leaves the selection behind",
        "        if matches!(self.screen, Screen::Dashboard | Screen::Archive) {",
        "        if false {",
        [
            "page_down_scrolls_a_long_list_to_its_end_and_no_further",
            "every_advertised_key_does_something",
        ],
    ),
    # -- the card ------------------------------------------------------------------------------------------
    (
        "the key card is not drawn",
        "        if self.show_help {\n            guitk::shortcut::render_card(",
        "        if false {\n            guitk::shortcut::render_card(",
        ["f1_shows_the_keys_and_the_card_is_modal"],
    ),
    (
        "the key card lets keys through",
        "        if self.show_help {\n            // Modal: a key behind the card",
        "        if false {\n            // Modal: a key behind the card",
        ["f1_shows_the_keys_and_the_card_is_modal"],
    ),
    # -- what is kept --------------------------------------------------------------------------------------
    (
        "the window keeps nothing",
        "        app.persist = true;",
        "        app.persist = false;",
        [
            "habits_and_their_check_ins_survive_a_restart",
            "a_new_habit_after_a_restart_gets_a_new_id",
            "a_failed_write_is_reported",
        ],
    ),
    (
        "a tracker made with new writes the user's file",
        "            persist: false,",
        "            persist: true,",
        ["a_tracker_made_with_new_writes_nothing"],
    ),
    (
        "check-ins are not written",
        '        doc.set_seq(&[HABITS_KEY, &key, "check_ins"], &refs);',
        "        let _ = refs;",
        ["habits_and_their_check_ins_survive_a_restart"],
    ),
    (
        "archiving is not written",
        '        doc.set_bool(&[HABITS_KEY, &key, "archived"], habit.archived);',
        "",
        ["habits_and_their_check_ins_survive_a_restart"],
    ),
    (
        "a check-in is not written when it is made",
        "                self.store_habit(habit_idx);",
        "",
        ["habits_and_their_check_ins_survive_a_restart"],
    ),
    (
        "a deleted habit is not taken out of the file",
        "        doc.remove(&[HABITS_KEY, &id.to_string()]);",
        "        let _ = id;",
        ["habits_and_their_check_ins_survive_a_restart"],
    ),
    (
        "ids start again at 1 after a restart",
        "            self.next_id = self.next_id.max(id.saturating_add(1));",
        "",
        ["a_new_habit_after_a_restart_gets_a_new_id", "an_unreadable_entry_is_skipped_and_left_alone"],
    ),
    (
        "an entry with no name is shown",
        '            let Some(name) = get("name").filter(|n| !n.trim().is_empty()) else {',
        '            let Some(name) = get("name").or(Some(String::from("?"))) else {',
        ["an_unreadable_entry_is_skipped_and_left_alone"],
    ),
    (
        "a kept weekly count is not clamped",
        "                        .clamp(1, 7),",
        ",",
        ["an_unreadable_entry_is_skipped_and_left_alone"],
    ),
    (
        "a failed write is covered by the change's own message",
        '        self.status_msg = String::from("Habit created!");\n'
        "        self.store_habit(self.habits.len().saturating_sub(1));",
        "        self.store_habit(self.habits.len().saturating_sub(1));\n"
        '        self.status_msg = String::from("Habit created!");',
        ["a_failed_write_is_reported"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "habits", timeout=900, only=only))
