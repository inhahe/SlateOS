"""Mutation test for the nonogram suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- The clues a puzzle states -------------------------------------
    (
        "a run that reaches the end of the line is never written down",
        "    if run > 0 {\n        clues.push(run);\n    }\n    if clues.is_empty() {",
        "    if clues.is_empty() {",
        ["a_run_reaching_the_end_of_the_line_is_still_counted"],
    ),
    (
        "an empty line is described by an empty list rather than a zero",
        "    if clues.is_empty() {\n        clues.push(0);\n    }\n",
        "",
        ["an_empty_line_is_stated_as_a_single_zero"],
    ),
    (
        "the run counter is never reset, so two runs read as one",
        "                clues.push(run);\n            }\n            run = 0;",
        "                clues.push(run);\n            }",
        ["separate_runs_are_listed_in_the_order_they_appear"],
    ),
    (
        "a column is read from the first cell rather than its own",
        "                .skip(c)",
        "                .skip(0)",
        ["row_clues_read_across_and_column_clues_read_down"],
    ),
    (
        "a column is read across rather than down",
        "                .step_by(cols)",
        "                .step_by(1)",
        ["row_clues_read_across_and_column_clues_read_down"],
    ),
    (
        "a picture with no width is chunked by zero",
        "    if cols == 0 {\n        return Vec::new();\n    }\n",
        "",
        ["a_clue_line_with_no_width_is_no_clue_at_all"],
    ),
    (
        "the row count is divided rather than checked",
        "    let Some(rows) = solution.len().checked_div(cols) else {\n"
        "        return Vec::new();\n"
        "    };",
        "    let rows = solution.len() / cols;",
        ["a_clue_line_with_no_width_is_no_clue_at_all"],
    ),
    (
        "a character past the right edge wraps onto the next row",
        "        for (c, ch) in line.chars().take(side).enumerate() {",
        "        for (c, ch) in line.chars().enumerate() {",
        ["a_picture_ignores_anything_past_its_own_edges"],
    ),
    (
        "the size label is written out rather than derived",
        '    format!("{side}x{side}")',
        '    format!("{side} x {side}")',
        ["a_puzzles_size_is_written_once"],
    ),
    # -- The rules of the game -----------------------------------------
    (
        "a filled cell cannot be emptied again",
        "            CellMark::Filled => CellMark::Empty,\n        };\n"
        "        self.set_cell(row, col, next);\n    }\n\n"
        "    /// Toggle a cell between Empty and MarkedEmpty",
        "            CellMark::Filled => CellMark::Filled,\n        };\n"
        "        self.set_cell(row, col, next);\n    }\n\n"
        "    /// Toggle a cell between Empty and MarkedEmpty",
        ["filling_a_cell_twice_puts_it_back"],
    ),
    (
        "filling a marked cell clears the mark instead of filling it",
        "            CellMark::Empty | CellMark::MarkedEmpty => CellMark::Filled,",
        "            CellMark::Empty => CellMark::Filled,\n"
        "            CellMark::MarkedEmpty => CellMark::Empty,",
        ["a_mark_replaces_a_fill_and_a_fill_replaces_a_mark"],
    ),
    (
        "marking a filled cell empties it instead of marking it",
        "            CellMark::Empty | CellMark::Filled => CellMark::MarkedEmpty,",
        "            CellMark::Empty => CellMark::MarkedEmpty,\n"
        "            CellMark::Filled => CellMark::Empty,",
        ["a_mark_replaces_a_fill_and_a_fill_replaces_a_mark"],
    ),
    (
        "a cell is off the grid only if both of its coordinates are",
        "        if row >= self.grid_side || col >= self.grid_side {\n            return None;\n        }",
        "        if row >= self.grid_side && col >= self.grid_side {\n            return None;\n        }",
        ["a_cell_off_the_grid_is_neither_read_nor_written"],
    ),
    (
        "one matching cell is taken for a solved picture",
        "            .all(|(&mark, &wanted)| (mark == CellMark::Filled) == wanted)",
        "            .any(|(&mark, &wanted)| (mark == CellMark::Filled) == wanted)",
        ["a_missing_cell_leaves_the_puzzle_unsolved"],
    ),
    (
        "an X counts towards the picture as much as a fill does",
        "            .all(|(&mark, &wanted)| (mark == CellMark::Filled) == wanted)",
        "            .all(|(&mark, &wanted)| (mark != CellMark::Empty) == wanted)",
        ["a_mark_is_not_a_fill_as_far_as_winning_goes"],
    ),
    (
        "check mode calls out the right answers instead of the wrong ones",
        "            Some(CellMark::Filled) => !should_fill,",
        "            Some(CellMark::Filled) => should_fill,",
        ["check_mode_calls_out_a_cell_filled_that_should_be_blank"],
    ),
    (
        "an X on a cell that should be blank is called a mistake",
        "            Some(CellMark::MarkedEmpty) => should_fill,",
        "            Some(CellMark::MarkedEmpty) => !should_fill,",
        ["check_mode_calls_out_a_cell_marked_blank_that_should_be_filled"],
    ),
    (
        "a cell you have not touched yet is called a mistake",
        "            Some(CellMark::Empty) | None => false,",
        "            Some(CellMark::Empty) | None => true,",
        ["check_mode_says_nothing_about_a_cell_you_have_not_touched"],
    ),
    (
        "the new puzzle keeps the marks of the one before it",
        "        self.cells = vec![CellMark::Empty; total];",
        "        self.cells.resize(total, CellMark::Empty);",
        ["starting_a_puzzle_clears_the_one_before_it"],
    ),
    (
        "the clock carries over into the next puzzle",
        "        self.elapsed_ms = 0;\n        self.check_mode = false;",
        "        self.check_mode = false;",
        ["starting_a_puzzle_clears_the_one_before_it"],
    ),
    (
        "check mode carries over into the next puzzle",
        "        self.elapsed_ms = 0;\n        self.check_mode = false;",
        "        self.elapsed_ms = 0;",
        ["starting_a_puzzle_clears_the_one_before_it"],
    ),
    (
        "a puzzle that is not in the catalogue starts one that is",
        "        let Some(def) = self.puzzles.get(index).cloned() else {",
        "        let Some(def) = self.puzzles.get(index % self.puzzles.len()).cloned() else {",
        ["a_puzzle_that_is_not_in_the_catalogue_starts_nothing"],
    ),
    # -- The clock ------------------------------------------------------
    (
        "the clock counts in hundredths rather than seconds",
        "        let total_secs = self.elapsed_ms / 1000;",
        "        let total_secs = self.elapsed_ms / 100;",
        ["the_clock_reads_minutes_and_seconds"],
    ),
    (
        "a minute is a hundred seconds long",
        "        let mins = total_secs / 60;",
        "        let mins = total_secs / 100;",
        ["the_clock_reads_minutes_and_seconds"],
    ),
    (
        "the seconds are not padded, so 1:05 reads as 1:5",
        '        format!("{mins}:{secs:02}")',
        '        format!("{mins}:{secs}")',
        ["the_clock_reads_minutes_and_seconds"],
    ),
    (
        "the clock runs everywhere except on the board",
        "        if self.screen == Screen::Playing {\n"
        "            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        "        if self.screen != Screen::Playing {\n"
        "            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        ["the_clock_runs_only_while_a_puzzle_is_being_played"],
    ),
    (
        "the clock is set to the last tick rather than advanced by it",
        "            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        "            self.elapsed_ms = elapsed_ms;",
        ["the_clock_runs_only_while_a_puzzle_is_being_played"],
    ),
    (
        "the window is never asked for the ticks the clock runs on",
        "        Some(TICK)",
        "        None",
        ["the_window_is_asked_for_the_ticks_the_clock_runs_on"],
    ),
    # -- The keyboard ---------------------------------------------------
    (
        "a key coming back up is played as a keystroke",
        "        Event::Key(ev) if ev.pressed => app.handle_key(ev),",
        "        Event::Key(ev) => app.handle_key(ev),",
        ["a_key_going_back_up_does_nothing"],
    ),
    (
        "a modified key is played as an unmodified one",
        "        if key_event.modifiers != Modifiers::NONE {\n"
        "            return EventResult::Ignored;\n"
        "        }\n",
        "",
        ["a_modified_key_belongs_to_whoever_is_listening_for_shortcuts"],
    ),
    (
        "the cursor runs off the top of the grid",
        "            Key::Up if self.cursor_row > 0 => {",
        "            Key::Up => {",
        ["an_arrow_at_the_edge_is_left_for_whoever_wants_it"],
    ),
    (
        "the cursor runs off the right of the grid",
        "            Key::Right if self.cursor_col.saturating_add(1) < self.grid_side => {",
        "            Key::Right => {",
        ["the_arrows_move_the_cursor_and_stop_at_the_edges"],
    ),
    (
        "the cursor runs off the left of the grid",
        "            Key::Left if self.cursor_col > 0 => {",
        "            Key::Left => {",
        ["an_arrow_at_the_edge_is_left_for_whoever_wants_it"],
    ),
    (
        "up moves the cursor down",
        "            Key::Up if self.cursor_row > 0 => {\n"
        "                self.cursor_row = self.cursor_row.saturating_sub(1);",
        "            Key::Up if self.cursor_row > 0 => {\n"
        "                self.cursor_row = self.cursor_row.saturating_add(1);",
        ["the_arrows_move_the_cursor_and_stop_at_the_edges"],
    ),
    (
        "down moves the cursor up",
        "            Key::Down if self.cursor_row.saturating_add(1) < self.grid_side => {\n"
        "                self.cursor_row = self.cursor_row.saturating_add(1);",
        "            Key::Down if self.cursor_row.saturating_add(1) < self.grid_side => {\n"
        "                self.cursor_row = self.cursor_row.saturating_sub(1);",
        ["the_arrows_move_the_cursor_and_stop_at_the_edges"],
    ),
    (
        "left moves the cursor right",
        "            Key::Left if self.cursor_col > 0 => {\n"
        "                self.cursor_col = self.cursor_col.saturating_sub(1);",
        "            Key::Left if self.cursor_col > 0 => {\n"
        "                self.cursor_col = self.cursor_col.saturating_add(1);",
        ["the_arrows_move_the_cursor_and_stop_at_the_edges"],
    ),
    (
        "right moves the cursor left",
        "            Key::Right if self.cursor_col.saturating_add(1) < self.grid_side => {\n"
        "                self.cursor_col = self.cursor_col.saturating_add(1);",
        "            Key::Right if self.cursor_col.saturating_add(1) < self.grid_side => {\n"
        "                self.cursor_col = self.cursor_col.saturating_sub(1);",
        ["the_arrows_move_the_cursor_and_stop_at_the_edges"],
    ),
    (
        "space fills the corner rather than the cell under the cursor",
        "                self.fill_at(self.cursor_row, self.cursor_col);",
        "                self.fill_at(0, 0);",
        ["space_and_enter_both_fill_the_cell_under_the_cursor"],
    ),
    (
        "X fills the cell instead of marking it",
        "            Key::X => {\n                self.toggle_mark_empty(self.cursor_row, self.cursor_col);",
        "            Key::X => {\n                self.toggle_fill(self.cursor_row, self.cursor_col);",
        ["x_marks_the_cell_under_the_cursor"],
    ),
    (
        "the check switch only ever turns on",
        "            Key::C => {\n                self.check_mode = !self.check_mode;",
        "            Key::C => {\n                self.check_mode = true;",
        ["c_turns_the_check_on_and_off_again"],
    ),
    (
        "Escape stays on the board",
        "            Key::Escape => {\n                self.screen = Screen::Select;",
        "            Key::Escape => {\n                self.screen = Screen::Playing;",
        ["escape_goes_back_to_the_list"],
    ),
    (
        "filling the last cell of the picture does not win",
        "        self.toggle_fill(row, col);\n"
        "        if self.check_win() {\n"
        "            self.screen = Screen::Won;\n"
        "        }",
        "        self.toggle_fill(row, col);",
        ["filling_the_last_cell_of_the_picture_wins_from_the_keyboard"],
    ),
    (
        "a key the game has no use for is swallowed",
        "            _ => EventResult::Ignored,\n        }\n    }\n\n    fn handle_key_won",
        "            _ => EventResult::Consumed,\n        }\n    }\n\n    fn handle_key_won",
        ["a_key_the_game_has_no_use_for_is_left_alone"],
    ),
    (
        "the list cursor runs off the top",
        "            Key::Up if self.select_cursor > 0 => {",
        "            Key::Up => {",
        ["the_list_cursor_stops_at_both_ends"],
    ),
    (
        "the list cursor runs off the bottom",
        "            Key::Down if self.select_cursor.saturating_add(1) < self.puzzles.len() => {",
        "            Key::Down => {",
        ["the_list_cursor_stops_at_both_ends"],
    ),
    (
        "the list cursor moves down when asked to move up",
        "            Key::Up if self.select_cursor > 0 => {\n"
        "                self.select_cursor = self.select_cursor.saturating_sub(1);",
        "            Key::Up if self.select_cursor > 0 => {\n"
        "                self.select_cursor = self.select_cursor.saturating_add(1);",
        ["the_list_cursor_stops_at_both_ends"],
    ),
    (
        "the list cursor moves up when asked to move down",
        "            Key::Down if self.select_cursor.saturating_add(1) < self.puzzles.len() => {\n"
        "                self.select_cursor = self.select_cursor.saturating_add(1);",
        "            Key::Down if self.select_cursor.saturating_add(1) < self.puzzles.len() => {\n"
        "                self.select_cursor = self.select_cursor.saturating_sub(1);",
        ["the_list_cursor_stops_at_both_ends"],
    ),
    (
        "Enter on the list starts the first puzzle rather than the chosen one",
        "                self.start_puzzle(self.select_cursor);",
        "                self.start_puzzle(0);",
        ["enter_on_the_list_starts_the_puzzle_under_the_cursor"],
    ),
    (
        "only Escape leaves the victory screen",
        "            Key::Enter | Key::Space | Key::Escape => {",
        "            Key::Escape => {",
        ["any_of_three_keys_leaves_the_victory_screen"],
    ),
    (
        "the victory screen goes on playing the game",
        "            Screen::Won => self.handle_key_won(key_event),",
        "            Screen::Won => self.handle_key_playing(key_event),",
        ["the_victory_screen_ignores_the_keys_that_play_the_game"],
    ),
    # -- The pointer ----------------------------------------------------
    (
        "a button coming back up is played as a click",
        "            MouseEventKind::Press(b) => b,",
        "            MouseEventKind::Press(b) | MouseEventKind::Release(b) => b,",
        ["a_click_that_is_not_a_press_is_not_a_click"],
    ),
    (
        "the right button fills like the left one",
        "                    MouseButton::Right => self.toggle_mark_empty(row, col),",
        "                    MouseButton::Right => self.fill_at(row, col),",
        ["right_clicking_marks_and_left_clicking_fills"],
    ),
    (
        "clicking a cell leaves the cursor where the keyboard left it",
        "                self.cursor_row = row;\n                self.cursor_col = col;\n",
        "",
        ["clicking_a_cell_moves_the_cursor_to_it"],
    ),
    (
        "the pointer goes on playing after the picture is solved",
        "                if self.screen == Screen::Won {\n"
        "                    return EventResult::Ignored;\n"
        "                }\n",
        "",
        ["the_pointer_cannot_change_the_picture_after_it_is_solved"],
    ),
    (
        "the check switch only ever turns on from the pointer",
        "            Target::Check => {\n                self.check_mode = !self.check_mode;",
        "            Target::Check => {\n                self.check_mode = true;",
        ["the_check_switch_can_be_reached_by_the_pointer"],
    ),
    (
        "the menu switch stays on the board",
        "            Target::Menu => {\n                self.screen = Screen::Select;",
        "            Target::Menu => {\n                self.screen = Screen::Playing;",
        ["the_menu_switch_returns_to_the_list"],
    ),
    (
        "clicking an entry starts whichever puzzle the keyboard was on",
        "                self.select_cursor = i;\n                self.start_puzzle(i);",
        "                self.start_puzzle(self.select_cursor);",
        ["clicking_an_entry_starts_that_puzzle"],
    ),
    (
        "the victory screen cannot be dismissed by clicking the board",
        "            None if self.screen == Screen::Won => {\n"
        "                self.screen = Screen::Select;\n"
        "                EventResult::Consumed\n"
        "            }",
        "            None if self.screen == Screen::Won => EventResult::Ignored,",
        ["a_click_on_the_board_returns_from_the_victory_screen"],
    ),
    (
        "a cell records the box of its transpose",
        "                f.hit(Target::Cell(row, col), g.cell_hit(row, col));",
        "                f.hit(Target::Cell(col, row), g.cell_hit(row, col));",
        ["clicking_a_cell_moves_the_cursor_to_it"],
    ),
    (
        "an entry is only clickable where its name is",
        "            f.hit(Target::Puzzle(i), entry);",
        "            f.hit(Target::Puzzle(i), list.name_rect(i));",
        ["every_entry_in_the_list_is_clickable_across_its_whole_width"],
    ),
    (
        "the footer switches record no box at all",
        "            f.hit(target, box_rect);\n",
        "",
        ["every_control_the_program_has_can_be_reached_somewhere"],
    ),
    # -- The layout -----------------------------------------------------
    (
        "the footer is placed from the top of the window",
        "        let footer = Rect::new(0.0, h - ftr, w, ftr);",
        "        let footer = Rect::new(0.0, ftr, w, ftr);",
        ["the_bands_tile_the_window_from_top_to_bottom"],
    ),
    (
        "the body starts at the top of the window rather than under the header",
        "            hdr + pad,\n            (w - pad * 2.0).max(0.0),",
        "            pad,\n            (w - pad * 2.0).max(0.0),",
        ["the_bands_tile_the_window_from_top_to_bottom"],
    ),
    (
        "the body runs to the bottom of the window rather than to the footer",
        "            (footer.y - hdr - pad * 2.0).max(0.0),",
        "            (h - hdr - pad * 2.0).max(0.0),",
        ["the_bands_tile_the_window_from_top_to_bottom"],
    ),
    (
        "the body is given no floor, so the chrome keeps its full height",
        "        let floor = h * BODY_SHARE;",
        "        let floor = 0.0;",
        ["the_footer_gives_up_its_height_before_the_body_does"],
    ),
    # -- The grid -------------------------------------------------------
    (
        "the cell size is taken from the width alone",
        "        let cell = (area.w / per_w).min(area.h / per_h).max(0.0);",
        "        let cell = (area.w / per_w).max(0.0);",
        ["the_whole_picture_fits_the_space_it_was_given"],
    ),
    (
        "the row-clue band is not counted in the width the picture needs",
        "        let per_w = usize_f32(row_slots) * CLUE_W_PER_CELL + spread;",
        "        let per_w = spread;",
        ["the_whole_picture_fits_the_space_it_was_given"],
    ),
    (
        "the clue band is assumed two numbers deep whatever the puzzle says",
        "        let per_h = usize_f32(col_slots) * CLUE_H_PER_CELL + spread;",
        "        let per_h = 2.0 * CLUE_H_PER_CELL + spread;",
        ["the_whole_picture_fits_the_space_it_was_given"],
    ),
    (
        "a deeper clue band does not make the cells smaller",
        "        let per_w = usize_f32(row_slots) * CLUE_W_PER_CELL + spread;",
        "        let per_w = 2.0 * CLUE_W_PER_CELL + spread;",
        ["a_deeper_clue_band_takes_its_room_from_the_cells"],
    ),
    (
        "the picture is put in the left corner rather than the middle",
        "        let x = area.x + (area.w - band_w - span) / 2.0;",
        "        let x = area.x;",
        ["the_picture_sits_in_the_middle_of_the_space_it_was_given"],
    ),
    (
        "the picture is put at the top rather than the middle",
        "        let y = area.y + (area.h - band_h - span) / 2.0;",
        "        let y = area.y;",
        ["the_picture_sits_in_the_middle_of_the_space_it_was_given"],
    ),
    (
        "the cells are drawn over the row-clue band",
        "            cells: Rect::new(x + band_w, y + band_h, span, span),",
        "            cells: Rect::new(x, y + band_h, span, span),",
        ["the_clue_bands_sit_against_the_cells_with_nothing_between"],
    ),
    (
        "the gap between cells is not counted in the step",
        "        self.cell + self.gap",
        "        self.cell",
        ["the_cells_are_evenly_spaced_and_never_overlap"],
    ),
    (
        "a cell is placed by its row in both directions",
        "            self.cells.x + usize_f32(col) * self.step(),\n"
        "            self.cells.y + usize_f32(row) * self.step(),\n"
        "            self.cell,\n            self.cell,",
        "            self.cells.x + usize_f32(row) * self.step(),\n"
        "            self.cells.y + usize_f32(row) * self.step(),\n"
        "            self.cell,\n            self.cell,",
        ["the_cells_are_evenly_spaced_and_never_overlap"],
    ),
    (
        "a cell is only clickable on its own ink, and the gaps go nowhere",
        "        Rect::new(r.x - half, r.y - half, r.w + self.gap, r.h + self.gap)",
        "        Rect::new(r.x, r.y, r.w, r.h)",
        ["a_click_between_two_cells_lands_in_the_nearer_one"],
    ),
    (
        "the cell boxes are grown by a whole gap each side, so they overlap",
        "        Rect::new(r.x - half, r.y - half, r.w + self.gap, r.h + self.gap)",
        "        Rect::new(\n"
        "            r.x - self.gap,\n"
        "            r.y - self.gap,\n"
        "            r.w + self.gap * 2.0,\n"
        "            r.h + self.gap * 2.0,\n"
        "        )",
        ["the_cell_boxes_abut_without_overlapping"],
    ),
    (
        "a cell is off the grid only if both of its grid coordinates are",
        "        if row >= self.side || col >= self.side {\n            return Rect::EMPTY;\n        }",
        "        if row >= self.side && col >= self.side {\n            return Rect::EMPTY;\n        }",
        ["a_cell_off_the_grid_has_no_box_at_all"],
    ),
    (
        "a grid with no room to draw in is laid out anyway",
        "        if side == 0 || area.is_empty() {",
        "        if side == 0 && area.is_empty() {",
        ["a_grid_with_no_room_is_no_grid_rather_than_a_negative_one"],
    ),
    (
        "every row clue sits beside the first row",
        "            self.row_clues.x + usize_f32(slot) * self.clue_w,\n"
        "            self.cells.y + usize_f32(row) * self.step(),",
        "            self.row_clues.x + usize_f32(slot) * self.clue_w,\n"
        "            self.cells.y,",
        ["a_row_clue_sits_beside_its_own_row"],
    ),
    (
        "every column clue sits over the first column",
        "            self.cells.x + usize_f32(col) * self.step(),\n"
        "            self.col_clues.y + usize_f32(slot) * self.clue_h,",
        "            self.cells.x,\n"
        "            self.col_clues.y + usize_f32(slot) * self.clue_h,",
        ["a_column_clue_sits_over_its_own_column"],
    ),
    (
        "a column clue slot is as wide as a row clue slot rather than a cell",
        "            self.cells.x + usize_f32(col) * self.step(),\n"
        "            self.col_clues.y + usize_f32(slot) * self.clue_h,\n"
        "            self.cell,",
        "            self.cells.x + usize_f32(col) * self.step(),\n"
        "            self.col_clues.y + usize_f32(slot) * self.clue_h,\n"
        "            self.clue_w,",
        ["a_column_clue_sits_over_its_own_column"],
    ),
    (
        "a clue slot off the end of the band is drawn anyway",
        "        if row >= self.side || slot >= self.row_slots {",
        "        if row >= self.side {",
        ["a_cell_off_the_grid_has_no_box_at_all"],
    ),
    # -- The select list ------------------------------------------------
    (
        "every entry is stacked at the top of the list",
        "            self.area.y + usize_f32(i) * self.step,",
        "            self.area.y,",
        ["the_entries_do_not_overlap_each_other"],
    ),
    (
        "a short list stretches its entries across the whole screen",
        "        let step = (area.h / usize_f32(count)).min(area.h * ENTRY_SHARE);",
        "        let step = area.h / usize_f32(count);",
        ["a_short_list_does_not_stretch_its_entries_to_fill_the_screen"],
    ),
    (
        "the size column is a guessed width rather than a measured one",
        # The `.map` line alone appears twice -- the test recomputes the widest
        # label the same way -- so the anchor carries the two lines above it,
        # where the production copy reads `self` and the test copy reads `app`.
        "        let widest = self\n"
        "            .puzzles\n"
        "            .iter()\n"
        "            .map(|p| text::measure(&size_label(p.size), l.font, FontWeightHint::Regular))",
        "        let widest = self\n"
        "            .puzzles\n"
        "            .iter()\n"
        "            .map(|_| 10.0f32)",
        ["the_size_column_is_as_wide_as_the_widest_label_in_the_catalogue"],
    ),
    (
        "the size column is dropped, so the name runs under the label",
        "            size_w: size_w.min((area.w - thumb - inset * 4.0).max(0.0)),",
        "            size_w: 0.0,",
        ["the_size_column_is_as_wide_as_the_widest_label_in_the_catalogue"],
    ),
    (
        "the size label is drawn under the thumbnail",
        "            e.right() - self.inset * 2.0 - self.thumb - self.size_w,",
        "            e.right() - self.inset * 2.0 - self.size_w,",
        ["the_three_columns_of_a_list_entry_do_not_run_into_each_other"],
    ),
    (
        "the name is given the whole entry and runs through the other two",
        "        let right = e.right() - self.inset * 3.0 - self.thumb - self.size_w;",
        "        let right = e.right();",
        ["the_three_columns_of_a_list_entry_do_not_run_into_each_other"],
    ),
    (
        "an empty list is laid out as if it had entries",
        "        if count == 0 || area.is_empty() {",
        "        if count == 0 && area.is_empty() {",
        ["a_list_with_nothing_in_it_asks_for_no_room"],
    ),
    # -- The window ------------------------------------------------------
    (
        "no string is told where to stop",
        "        max_width: Some(limit),",
        "        max_width: None,",
        ["every_string_is_told_where_to_stop"],
    ),
    (
        "the size a frame is drawn at is not the size the next click is read against",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_records_the_size_the_next_click_is_read_against"],
    ),
    (
        "a resize is read with its sides the wrong way round",
        "            app.resize(*width as f32, *height as f32);",
        "            app.resize(*height as f32, *width as f32);",
        ["a_resize_moves_the_layout_the_next_click_is_read_against"],
    ),
    (
        "a resize is ignored",
        "        Event::Resize { width, height } => {\n"
        "            app.resize(*width as f32, *height as f32);\n"
        "            EventResult::Consumed\n"
        "        }\n",
        "",
        ["a_resize_moves_the_layout_the_next_click_is_read_against"],
    ),
    (
        "a window squashed to nothing is taken at its word",
        "        self.size_drawn = (width.max(1.0), height.max(1.0));",
        "        self.size_drawn = (width, height);",
        ["a_window_squashed_to_nothing_still_lays_out"],
    ),
    (
        "the close request is not answered",
        "        if matches!(event, Event::CloseRequested) {\n"
        "            return Response::Exit;\n"
        "        }\n",
        "",
        ["closing_the_window_exits_and_nothing_else_does"],
    ),
    (
        "an event that changed something does not ask for a repaint",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        ["closing_the_window_exits_and_nothing_else_does"],
    ),
    (
        "an event that changed nothing asks for a repaint anyway",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["a_tick_off_the_board_does_not_repaint"],
    ),
    (
        "the window is named something other than what it is",
        '        "nonogram".to_string()',
        '        "picross".to_string()',
        ["the_window_names_itself_and_says_the_same_thing_twice"],
    ),
    (
        "the window title is not the name of the game",
        '        "Nonogram".to_string()',
        '        "Puzzle".to_string()',
        ["the_window_names_itself_and_says_the_same_thing_twice"],
    ),
    (
        "the window opens at a size the game was not laid out for",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (800, 600)",
        ["the_window_names_itself_and_says_the_same_thing_twice"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "nonogram", timeout=240, only=sys.argv[1:] or None))
