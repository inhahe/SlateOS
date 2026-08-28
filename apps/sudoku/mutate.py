"""Mutation test for sudoku's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Sudoku is the sixteenth application in this campaign.  Its old suite was large
and it was honest about the solver, but it could not have noticed most of what
was wrong with the program, because the program had no window: `main` built a
`SudokuApp`, dropped it and exited.  Nothing in the file ever constructed an
`Event`, so there was no path from a click to a digit to test, and no layout to
test either -- the cell size was a constant and the window's size was computed
from it.

Usage:  python -u apps/sudoku/mutate.py [substring ...]
"""

import re
import subprocess
import sys
from pathlib import Path

SRC = Path(__file__).parent / "src" / "main.rs"
BAK = Path(__file__).parent / "src" / "main.rs.bak"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── Grid arithmetic ───────────────────────────────────────────────────
    (
        "an index counts columns before rows",
        "    row.saturating_mul(GRID_SIZE).saturating_add(col)",
        "    col.saturating_mul(GRID_SIZE).saturating_add(row)",
        ["an_index_and_a_row_and_column_name_the_same_cell"],
    ),
    (
        "a flat index gives back its row and column the other way round",
        "    (index.wrapping_div(GRID_SIZE), index.wrapping_rem(GRID_SIZE))",
        "    (index.wrapping_rem(GRID_SIZE), index.wrapping_div(GRID_SIZE))",
        ["an_index_and_a_row_and_column_name_the_same_cell"],
    ),
    (
        "a box origin is not rounded down to the box",
        "        row.wrapping_div(BOX_SIZE).wrapping_mul(BOX_SIZE),",
        "        row,",
        ["a_box_origin_is_the_top_left_of_the_three_by_three_it_belongs_to"],
    ),
    (
        "a box origin uses the row for both halves",
        "        col.wrapping_div(BOX_SIZE).wrapping_mul(BOX_SIZE),",
        "        row.wrapping_div(BOX_SIZE).wrapping_mul(BOX_SIZE),",
        ["a_box_origin_is_the_top_left_of_the_three_by_three_it_belongs_to"],
    ),
    (
        "reading a square past the last column wraps to the next row",
        "pub fn at(grid: &[u8; TOTAL_CELLS], row: usize, col: usize) -> u8 {\n    if row >= GRID_SIZE || col >= GRID_SIZE {\n        return 0;\n    }",
        "pub fn at(grid: &[u8; TOTAL_CELLS], row: usize, col: usize) -> u8 {",
        ["a_square_off_the_board_reads_as_empty_rather_than_wrapping_round"],
    ),
    (
        "writing a square past the last column wraps to the next row",
        "fn put(grid: &mut [u8; TOTAL_CELLS], row: usize, col: usize, digit: u8) {\n    if row >= GRID_SIZE || col >= GRID_SIZE {\n        return;\n    }",
        "fn put(grid: &mut [u8; TOTAL_CELLS], row: usize, col: usize, digit: u8) {",
        ["a_write_off_the_board_lands_nowhere_rather_than_on_the_next_row"],
    ),
    (
        "reading a cell past the last column wraps to the next row",
        "    pub fn cell(&self, row: usize, col: usize) -> Cell {\n        if row >= GRID_SIZE || col >= GRID_SIZE {\n            return Cell::empty();\n        }",
        "    pub fn cell(&self, row: usize, col: usize) -> Cell {",
        ["a_square_off_the_board_cannot_be_read_or_written_through_the_model"],
    ),
    (
        "writing a cell past the last column wraps to the next row",
        "    fn write(&mut self, row: usize, col: usize, edit: impl FnOnce(&mut Cell)) {\n        if row >= GRID_SIZE || col >= GRID_SIZE {\n            return;\n        }",
        "    fn write(&mut self, row: usize, col: usize, edit: impl FnOnce(&mut Cell)) {",
        ["a_square_off_the_board_cannot_be_read_or_written_through_the_model"],
    ),
    # ── Conflicts ─────────────────────────────────────────────────────────
    (
        "an empty square conflicts with everything",
        "    if digit == 0 {\n        return false;\n    }",
        "    if digit == 0 {\n        return true;\n    }",
        ["an_empty_square_is_never_a_conflict"],
    ),
    (
        "a row is not scanned for a repeat",
        "    if (0..GRID_SIZE).any(|c| c != col && at(grid, row, c) == digit) {\n        return true;\n    }",
        "    if false {\n        return true;\n    }",
        ["a_digit_repeated_in_a_row_a_column_or_a_box_is_a_conflict"],
    ),
    (
        "a column is not scanned for a repeat",
        "    if (0..GRID_SIZE).any(|r| r != row && at(grid, r, col) == digit) {\n        return true;\n    }",
        "    if false {\n        return true;\n    }",
        ["a_digit_repeated_in_a_row_a_column_or_a_box_is_a_conflict"],
    ),
    (
        "a box is not scanned for a repeat",
        "            (r, c) != (row, col) && at(grid, r, c) == digit",
        "            false",
        ["a_digit_repeated_in_a_row_a_column_or_a_box_is_a_conflict"],
    ),
    (
        "a cell counts as a repeat of itself in its row",
        "    if (0..GRID_SIZE).any(|c| c != col && at(grid, row, c) == digit) {",
        "    if (0..GRID_SIZE).any(|c| at(grid, row, c) == digit) {",
        ["a_cell_is_never_its_own_conflict"],
    ),
    (
        "a cell counts as a repeat of itself in its box",
        "            (r, c) != (row, col) && at(grid, r, c) == digit\n        })",
        "            at(grid, r, c) == digit\n        })",
        ["a_cell_is_never_its_own_conflict"],
    ),
    (
        "a board with holes in it counts as finished",
        "    !grid.contains(&0) && is_grid_valid(grid)",
        "    is_grid_valid(grid)",
        ["a_grid_with_a_hole_is_valid_but_not_complete"],
    ),
    (
        "a board that repeats a digit counts as finished",
        "    !grid.contains(&0) && is_grid_valid(grid)",
        "    !grid.contains(&0)",
        [
            "a_full_grid_that_repeats_a_digit_is_not_complete",
            "a_full_board_with_a_digit_in_the_wrong_place_is_not_a_win",
        ],
    ),
    (
        "a board's digits are read off its marks",
        "        *slot = cell.value;",
        "        *slot = u8::from(cell.has_any_note());",
        ["the_digits_of_a_board_are_its_digits_and_not_its_marks"],
    ),
    # ── Cells and marks ───────────────────────────────────────────────────
    (
        "a hint is not locked",
        "        matches!(self.origin, Origin::Given | Origin::Hint)",
        "        matches!(self.origin, Origin::Given)",
        ["a_clue_and_a_hint_are_both_locked_but_only_one_is_a_clue"],
    ),
    (
        "a clue is not locked",
        "        matches!(self.origin, Origin::Given | Origin::Hint)",
        "        matches!(self.origin, Origin::Hint)",
        ["a_clue_and_a_hint_are_both_locked_but_only_one_is_a_clue"],
    ),
    (
        "a mark's slot is the digit itself",
        "        usize::from(digit).checked_sub(1)",
        "        usize::from(digit).checked_add(0)",
        ["a_marks_slot_is_the_digit_less_one_and_nothing_else_has_a_slot"],
    ),
    (
        "a ten is a digit as far as the marks are concerned",
        "fn note_slot(digit: u8) -> Option<usize> {\n    if (1..=9).contains(&digit) {",
        "fn note_slot(digit: u8) -> Option<usize> {\n    if (1..=10).contains(&digit) {",
        ["a_marks_slot_is_the_digit_less_one_and_nothing_else_has_a_slot"],
    ),
    (
        "a mark goes on and stays on",
        "        if let Some(slot) = note_slot(digit).and_then(|i| self.notes.get_mut(i)) {\n            *slot = !*slot;",
        "        if let Some(slot) = note_slot(digit).and_then(|i| self.notes.get_mut(i)) {\n            *slot = true;",
        ["a_mark_goes_on_and_comes_off_again"],
    ),
    (
        "a mark is read out of the slot after its own",
        "        note_slot(digit)\n            .and_then(|i| self.notes.get(i))",
        "        note_slot(digit)\n            .and_then(|i| self.notes.get(i.wrapping_add(1)))",
        ["every_digit_has_its_own_mark"],
    ),
    (
        "clearing the marks clears all but the first",
        "    pub const fn clear_notes(&mut self) {\n        self.notes = [false; GRID_SIZE];",
        "    pub const fn clear_notes(&mut self) {\n        self.notes = [false, true, true, true, true, true, true, true, true];",
        ["clearing_the_marks_clears_all_nine"],
    ),
    (
        "a mark for something that is not a digit is taken as a mark for one",
        "        if let Some(slot) = note_slot(digit).and_then(|i| self.notes.get_mut(i)) {",
        "        if let Some(slot) = note_slot(digit).or(Some(0)).and_then(|i| self.notes.get_mut(i)) {",
        ["a_mark_for_something_that_is_not_a_digit_is_ignored"],
    ),
    # ── The scoreboard ────────────────────────────────────────────────────
    (
        "every level is counted in the same slot",
        "fn stat_slot(difficulty: Difficulty) -> usize {\n    match difficulty {\n        Difficulty::Easy => 0,\n        Difficulty::Medium => 1,\n        Difficulty::Hard => 2,\n    }\n}",
        "fn stat_slot(difficulty: Difficulty) -> usize {\n    let _ = difficulty;\n    0\n}",
        [
            "the_three_levels_keep_three_separate_slots",
            "a_finish_is_counted_against_its_own_level_and_nobody_elses",
        ],
    ),
    (
        "a slower finish replaces a faster one",
        "                Some(prev) => prev.min(elapsed_secs),",
        "                Some(prev) => prev.max(elapsed_secs),",
        ["a_slower_finish_never_replaces_a_faster_one"],
    ),
    (
        "a finish is not counted",
        "            *count = count.saturating_add(1);",
        "            *count = *count;",
        # Not `the_three_levels_keep_three_separate_slots`: that one only ever
        # calls `stat_slot`, so a broken counter is invisible to it.
        ["a_finish_is_counted_against_its_own_level_and_nobody_elses"],
    ),
    (
        "a difficulty's clue range is the same for every level",
        "            Self::Easy => (35, 40),",
        "            Self::Easy => (22, 27),",
        ["a_harder_level_asks_for_fewer_clues_than_an_easier_one"],
    ),
    (
        "the difficulty chip goes round the wrong way",
        "            Self::Easy => Self::Medium,\n            Self::Medium => Self::Hard,\n            Self::Hard => Self::Easy,",
        "            Self::Easy => Self::Hard,\n            Self::Medium => Self::Easy,\n            Self::Hard => Self::Medium,",
        ["cycling_difficulty_visits_all_three_and_comes_home"],
    ),
    (
        "two levels share a name",
        "            Self::Medium => \"Medium\",",
        "            Self::Medium => \"Easy\",",
        ["every_difficulty_has_its_own_name_and_its_own_colour"],
    ),
    (
        "two levels share a colour",
        "            Self::Medium => YELLOW,",
        "            Self::Medium => GREEN,",
        ["every_difficulty_has_its_own_name_and_its_own_colour"],
    ),
    (
        "the total counts only the easy games",
        "    pub fn total_completed(&self) -> u32 {",
        "    pub fn total_completed(&self) -> u32 {\n        return self.games_completed(Difficulty::Easy);\n        #[allow(unreachable_code)]",
        ["the_total_is_every_level_added_up"],
    ),
    # ── The solver ────────────────────────────────────────────────────────
    #
    # `count_solutions_inner` used to open with `if found >= limit { return
    # found }` as well as close its loop with `if total >= limit { break }`.
    # The sweep caught that: breaking the loop bound survived, because the entry
    # guard alone still stopped the search (known-issues.md lesson 51 -- a guard
    # behind a duplicate of itself).  The entry guard is gone now and the one
    # case it really did decide, a limit of none, is answered at the door.
    (
        "the search takes the cells in reading order",
        "        if best.is_none_or(|(_, _, _, seen)| count < seen) {",
        "        if best.is_none() {",
        ["the_next_cell_to_try_is_the_one_with_the_fewest_answers_not_the_first_one"],
    ),
    (
        "the search takes the cell with the most answers",
        "        if best.is_none_or(|(_, _, _, seen)| count < seen) {",
        "        if best.is_none_or(|(_, _, _, seen)| count > seen) {",
        ["the_next_cell_to_try_is_the_one_with_the_fewest_answers_not_the_first_one"],
    ),
    (
        "a filled cell is offered to the search",
        "        if at(grid, r, c) != 0 {\n            continue;\n        }",
        "        if false {\n            continue;\n        }",
        [
            "the_next_cell_to_try_is_the_one_with_the_fewest_answers_not_the_first_one",
            "a_finished_grid_has_no_next_cell_and_no_empty_one",
        ],
    ),
    (
        "the first empty square is looked for from the wrong end",
        "        .find(|&(r, c)| at(grid, r, c) == 0)",
        "        .filter(|&(r, c)| at(grid, r, c) == 0)\n        .next_back()",
        ["the_first_empty_square_is_found_in_reading_order"],
    ),
    (
        "the ninth digit is never a candidate",
        "    let mut mask: u16 = 0x1FF;",
        "    let mut mask: u16 = 0x0FF;",
        ["the_candidates_are_the_digits_that_would_not_break_a_rule"],
    ),
    (
        "the row is not struck off the candidates",
        "        strike(at(grid, row, i));",
        "        strike(0);",
        ["the_candidates_are_the_digits_that_would_not_break_a_rule"],
    ),
    (
        "the column is not struck off the candidates",
        "        strike(at(grid, i, col));",
        "        strike(0);",
        ["the_candidates_are_the_digits_that_would_not_break_a_rule"],
    ),
    (
        "the box is not struck off the candidates",
        "            strike(at(grid, br.saturating_add(dr), bc.saturating_add(dc)));",
        "            strike(0);",
        ["the_candidates_are_the_digits_that_would_not_break_a_rule"],
    ),
    (
        "a candidate is a digit whose bit is clear",
        "    note_slot(digit).is_some_and(|bit| mask & (1u16 << bit) != 0)",
        "    note_slot(digit).is_some_and(|bit| mask & (1u16 << bit) == 0)",
        [
            "the_candidates_are_the_digits_that_would_not_break_a_rule",
            "a_cell_with_eight_of_its_nine_digits_around_it_has_one_candidate",
        ],
    ),
    (
        "the solver never tries a nine",
        "    for digit in 1..=9u8 {\n        if is_candidate(cands, digit) {",
        "    for digit in 1..=8u8 {\n        if is_candidate(cands, digit) {",
        ["the_solver_fills_an_empty_grid"],
    ),
    (
        "the solver gives up on a grid it has finished",
        "    let Some((row, col, cands)) = next_cell(grid) else {\n        return true;\n    };\n    for digit in 1..=9u8 {",
        "    let Some((row, col, cands)) = next_cell(grid) else {\n        return false;\n    };\n    for digit in 1..=9u8 {",
        ["the_solver_fills_an_empty_grid", "the_solver_finishes_a_grid_it_can_finish"],
    ),
    (
        "the solver leaves its wrong guesses on the board",
        "            if solve(grid) {\n                return true;\n            }\n            put(grid, row, col, 0);",
        "            if solve(grid) {\n                return true;\n            }",
        ["the_solver_takes_back_a_guess_that_led_nowhere"],
    ),
    (
        "a finished grid counts as no answer at all",
        "        return found.saturating_add(1);",
        "        return found;",
        [
            "a_grid_with_one_hole_has_exactly_one_answer",
            "a_grid_with_room_to_guess_has_more_than_one_answer",
        ],
    ),
    (
        "counting answers does not stop at the limit",
        "        if total >= limit {\n            break;\n        }",
        "        if false {\n            break;\n        }",
        ["counting_answers_stops_at_the_limit_it_was_given"],
    ),
    (
        "asking for no answers at all is answered with one",
        "    if limit == 0 {\n        return 0;\n    }\n    count_solutions_inner(grid, limit, 0)",
        "    count_solutions_inner(grid, limit, 0)",
        ["counting_answers_stops_at_the_limit_it_was_given"],
    ),
    (
        "counting answers forgets the ones already found",
        "            total = count_solutions_inner(grid, limit, total);",
        "            total = count_solutions_inner(grid, limit, 0);",
        [
            "a_grid_with_room_to_guess_has_more_than_one_answer",
            "counting_answers_stops_at_the_limit_it_was_given",
        ],
    ),
    (
        "a shuffled solve is not shuffled",
        "    rng.shuffle(&mut digits);",
        "    let _ = &mut digits;",
        ["one_seed_is_one_grid_and_two_seeds_are_two"],
    ),
    (
        "a shuffled solve tries every digit, not only the candidates",
        "    let mut digits: Vec<u8> = (1..=9u8).filter(|&d| is_candidate(cands, d)).collect();",
        "    let mut digits: Vec<u8> = (1..=9u8).collect();",
        ["a_shuffled_solve_still_produces_a_finished_grid"],
    ),
    # ── The generator ─────────────────────────────────────────────────────
    (
        "a clue is taken out even when it leaves two answers",
        "        if count_solutions(&mut test, 2) == 1 {",
        "        if count_solutions(&mut test, 2) >= 1 {",
        ["a_generated_puzzle_has_exactly_one_answer"],
    ),
    (
        "clues are taken out until there are none left",
        "        if removed >= target_removals {\n            break;\n        }",
        "        if false {\n            break;\n        }",
        ["a_generated_puzzle_keeps_at_least_the_clues_its_level_asks_for"],
    ),
    (
        "a clue that had to stay is not put back",
        "            *slot = saved;",
        "            *slot = 0;",
        ["a_generated_puzzle_has_exactly_one_answer"],
    ),
    (
        "the puzzle is the solution with nothing taken out",
        "        if let Some(slot) = puzzle.get_mut(cell_idx) {\n            *slot = 0;\n        }",
        "        if let Some(slot) = puzzle.get_mut(cell_idx) {\n            *slot = saved;\n        }",
        ["a_generated_puzzle_keeps_at_least_the_clues_its_level_asks_for"],
    ),
    (
        "a hole in the puzzle is still a clue",
        "        Some(v) if v != 0 => Cell::as_given(v),",
        "        Some(v) => Cell::as_given(v),",
        ["a_generated_puzzle_is_its_solution_with_clues_taken_out"],
    ),
    # ── Intents ───────────────────────────────────────────────────────────
    (
        "a paused game will not answer the Pause chip",
        "            Intent::Pause => return self.toggle_pause(),",
        "            Intent::Pause if self.status == GameStatus::Playing => {\n                return self.toggle_pause();\n            }",
        ["a_paused_game_can_be_restarted_and_resumed"],
    ),
    (
        "a won game will not answer the New chip",
        "            Intent::NewGame => {\n                self.new_game(self.difficulty);\n                return EventResult::Consumed;\n            }",
        "            Intent::NewGame if self.status == GameStatus::Playing => {\n                self.new_game(self.difficulty);\n                return EventResult::Consumed;\n            }",
        ["a_won_game_still_answers_the_new_and_difficulty_chips"],
    ),
    (
        "a won game will not answer the difficulty chip",
        "            Intent::CycleDifficulty => {\n                self.new_game(self.difficulty.next());\n                return EventResult::Consumed;\n            }",
        "            Intent::CycleDifficulty if self.status == GameStatus::Playing => {\n                self.new_game(self.difficulty.next());\n                return EventResult::Consumed;\n            }",
        ["a_won_game_still_answers_the_new_and_difficulty_chips"],
    ),
    (
        "ctrl and a number deal the level the game is already on",
        "            Intent::SetDifficulty(d) => {\n                self.new_game(d);",
        "            Intent::SetDifficulty(_) => {\n                self.new_game(self.difficulty);",
        ["ctrl_and_a_number_deals_that_level"],
    ),
    (
        "a paused game plays on",
        "        if self.status != GameStatus::Playing {\n            return EventResult::Ignored;\n        }\n        match intent {\n            Intent::Select(row, col) => self.select(row, col),",
        "        match intent {\n            Intent::Select(row, col) => self.select(row, col),",
        ["a_paused_game_can_be_restarted_and_resumed"],
    ),
    (
        "pausing a finished game unpauses it",
        "            GameStatus::Won => EventResult::Ignored,",
        "            GameStatus::Won => {\n                self.status = GameStatus::Playing;\n                EventResult::Consumed\n            }",
        ["pausing_a_finished_game_is_ignored"],
    ),
    (
        "pausing a paused game leaves it paused",
        "            GameStatus::Paused => {\n                self.status = GameStatus::Playing;",
        "            GameStatus::Paused => {\n                self.status = GameStatus::Paused;",
        ["a_paused_game_can_be_restarted_and_resumed"],
    ),
    # ── Selection ─────────────────────────────────────────────────────────
    (
        "a square off the board can be selected",
        "        if row >= GRID_SIZE || col >= GRID_SIZE || self.selected == (row, col) {",
        "        if self.selected == (row, col) {",
        ["a_square_off_the_board_cannot_be_selected"],
    ),
    (
        "selecting the square already selected asks for a repaint",
        "        if row >= GRID_SIZE || col >= GRID_SIZE || self.selected == (row, col) {",
        "        if row >= GRID_SIZE || col >= GRID_SIZE {",
        ["selecting_the_square_already_selected_asks_for_no_repaint"],
    ),
    (
        "up and down are the wrong way round",
        "            Dir::Up => (row.saturating_sub(1), col),\n            Dir::Down => (row.saturating_add(1), col),",
        "            Dir::Up => (row.saturating_add(1), col),\n            Dir::Down => (row.saturating_sub(1), col),",
        ["an_arrow_moves_the_selection_one_square_that_way"],
    ),
    (
        "left and right move by row",
        "            Dir::Left => (row, col.saturating_sub(1)),\n            Dir::Right => (row, col.saturating_add(1)),",
        "            Dir::Left => (row.saturating_sub(1), col),\n            Dir::Right => (row.saturating_add(1), col),",
        ["an_arrow_moves_the_selection_one_square_that_way"],
    ),
    (
        "the selection can be moved off the bottom of the board",
        "        if row >= GRID_SIZE || col >= GRID_SIZE || self.selected == (row, col) {",
        "        if col >= GRID_SIZE || self.selected == (row, col) {",
        ["the_selection_stops_at_the_edge_rather_than_wrapping_round"],
    ),
    (
        "the selection can be moved off the side of the board",
        "        if row >= GRID_SIZE || col >= GRID_SIZE || self.selected == (row, col) {",
        "        if row >= GRID_SIZE || self.selected == (row, col) {",
        ["the_selection_stops_at_the_edge_rather_than_wrapping_round"],
    ),
    # ── Writing, erasing, hinting ─────────────────────────────────────────
    (
        "a number that is not a sudoku digit is written anyway",
        "        if note_slot(digit).is_none() {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_number_that_is_not_a_sudoku_digit_is_refused"],
    ),
    (
        "a clue can be written over",
        "        let cell = self.cell(row, col);\n        if cell.fixed() {\n            return EventResult::Ignored;\n        }\n        if self.note_mode {",
        "        let cell = self.cell(row, col);\n        if self.note_mode {",
        ["a_clue_cannot_be_written_over", "a_clue_takes_no_marks"],
    ),
    (
        "note mode writes a digit like any other",
        "        if self.note_mode {\n            self.write(row, col, |c| c.toggle_note(digit));",
        "        if false {\n            self.write(row, col, |c| c.toggle_note(digit));",
        ["in_note_mode_a_digit_becomes_a_mark_and_not_an_answer"],
    ),
    (
        "writing the digit already there asks for a repaint",
        "        if cell.value == digit {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["writing_the_digit_that_is_already_there_asks_for_no_repaint"],
    ),
    (
        "writing a digit leaves the marks behind it",
        "        self.write(row, col, |c| {\n            c.value = digit;\n            c.clear_notes();\n        });",
        "        self.write(row, col, |c| {\n            c.value = digit;\n        });",
        ["writing_a_digit_sweeps_the_marks_away"],
    ),
    (
        "a digit is written but not recorded",
        "        self.record(Change::SetValue {\n            row,\n            col,\n            old_value: cell.value,\n            new_value: digit,\n            old_notes: cell.notes,\n        });",
        "        let _ = Change::SetValue {\n            row,\n            col,\n            old_value: cell.value,\n            new_value: digit,\n            old_notes: cell.notes,\n        };",
        ["undo_takes_a_digit_back_and_redo_puts_it_again"],
    ),
    (
        "the last digit does not win the game",
        "        self.check_completion();\n        EventResult::Consumed\n    }\n\n    fn erase(&mut self) -> EventResult {",
        "        EventResult::Consumed\n    }\n\n    fn erase(&mut self) -> EventResult {",
        ["the_last_digit_wins_and_the_time_goes_on_the_board"],
    ),
    (
        "a clue can be erased",
        "        if cell.fixed() || (cell.is_empty() && !cell.has_any_note()) {",
        "        if cell.is_empty() && !cell.has_any_note() {",
        ["a_clue_cannot_be_erased"],
    ),
    (
        "erasing a bare square asks for a repaint",
        "        if cell.fixed() || (cell.is_empty() && !cell.has_any_note()) {\n            return EventResult::Ignored;\n        }",
        "        if cell.fixed() {\n            return EventResult::Ignored;\n        }",
        ["erasing_a_square_that_is_already_bare_asks_for_no_repaint"],
    ),
    (
        "erasing a square keeps its marks",
        "        self.write(row, col, |c| {\n            c.value = 0;\n            c.clear_notes();\n        });",
        "        self.write(row, col, |c| {\n            c.value = 0;\n        });",
        ["erasing_a_square_that_holds_only_marks_clears_the_marks"],
    ),
    (
        "the hints never run out",
        "        if self.hints_remaining() == 0 {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        # Not `the_hint_key_greys_out_when_the_hints_are_gone`: the grey is
        # drawn from the counter, which still reads zero with the guard gone.
        ["the_hints_run_out"],
    ),
    # `use_hint` used to read `if cell.fixed() || cell.value == solution`.  The
    # sweep caught that too: dropping `cell.fixed()` survived, because a clue's
    # value *is* the answer, so the second half refuses clues on its own -- once
    # more the shape of lesson 51.  One test, not two, decides the guard now.
    (
        "a hint is spent on a square that is already right",
        "        if cell.value == self.solution_at(row, col) {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        [
            "a_hint_for_a_square_that_is_already_right_is_refused",
            "a_hint_for_a_clue_is_refused",
        ],
    ),
    (
        "a hint makes the square a clue",
        "            c.origin = Origin::Hint;\n            c.clear_notes();\n        });\n        self.check_completion();",
        "            c.origin = Origin::Given;\n            c.clear_notes();\n        });\n        self.check_completion();",
        [
            "spending_a_hint_does_not_add_a_clue_to_the_puzzle",
            "a_hint_fills_the_square_from_the_solution_and_locks_it",
        ],
    ),
    (
        "a hint leaves the square editable",
        "            c.value = answer;\n            c.origin = Origin::Hint;",
        "            c.value = answer;\n            c.origin = Origin::Player;",
        ["a_hint_fills_the_square_from_the_solution_and_locks_it"],
    ),
    (
        "a hint cannot win the game",
        "            c.clear_notes();\n        });\n        self.check_completion();\n        EventResult::Consumed\n    }\n\n    fn undo(&mut self) -> EventResult {",
        "            c.clear_notes();\n        });\n        EventResult::Consumed\n    }\n\n    fn undo(&mut self) -> EventResult {",
        ["a_hint_can_win_the_game"],
    ),
    # ── Undo and redo ─────────────────────────────────────────────────────
    (
        "an empty history is still an undo",
        "        let Some(change) = self.undo_stack.pop() else {\n            return EventResult::Ignored;\n        };",
        "        let Some(change) = self.undo_stack.pop() else {\n            return EventResult::Consumed;\n        };",
        ["there_is_nothing_to_take_back_from_a_fresh_game"],
    ),
    (
        "an empty redo is still a redo",
        "        let Some(change) = self.redo_stack.pop() else {\n            return EventResult::Ignored;\n        };",
        "        let Some(change) = self.redo_stack.pop() else {\n            return EventResult::Consumed;\n        };",
        ["there_is_nothing_to_take_back_from_a_fresh_game"],
    ),
    (
        "an undo puts back the new value instead of the old one",
        "            } => self.write(row, col, |c| {\n                c.value = old_value;\n                c.notes = old_notes;\n            }),",
        "            } => self.write(row, col, |c| {\n                c.notes = old_notes;\n            }),",
        ["undo_takes_a_digit_back_and_redo_puts_it_again"],
    ),
    (
        "an undo does not give back the marks it swept away",
        "                c.value = old_value;\n                c.notes = old_notes;\n            }),\n            Change::ToggleNote { row, col, digit } => {",
        "                c.value = old_value;\n            }),\n            Change::ToggleNote { row, col, digit } => {",
        ["undoing_a_digit_gives_back_the_marks_it_swept_away"],
    ),
    (
        "an undone hint stays a hint",
        "            } => self.write(row, col, |c| {\n                c.value = old_value;\n                c.notes = old_notes;\n                c.origin = Origin::Player;\n            }),",
        "            } => self.write(row, col, |c| {\n                c.value = old_value;\n                c.notes = old_notes;\n            }),",
        ["undoing_a_hint_gives_the_hint_back"],
    ),
    (
        "an undone move cannot be redone",
        "        self.redo_stack.push(change);",
        "        let _ = change;",
        ["undo_takes_a_digit_back_and_redo_puts_it_again"],
    ),
    (
        "a redone move cannot be undone again",
        "        self.undo_stack.push(change);\n        // A redo can fill the last empty cell",
        "        let _ = change;\n        // A redo can fill the last empty cell",
        ["undo_takes_a_digit_back_and_redo_puts_it_again"],
    ),
    (
        "a redo cannot win the game",
        "        self.undo_stack.push(change);\n        // A redo can fill the last empty cell, and used not to be able to win:\n        // `check_completion` was called from the two places that wrote a digit\n        // forwards and from neither of the two that wrote one back.\n        self.check_completion();",
        "        self.undo_stack.push(change);",
        ["a_redo_can_win_the_game"],
    ),
    (
        "a fresh move keeps the moves that were undone",
        "        self.undo_stack.push(change);\n        self.redo_stack.clear();",
        "        self.undo_stack.push(change);",
        ["a_fresh_move_throws_away_the_moves_that_were_undone"],
    ),
    (
        "the history grows for ever",
        "        if self.undo_stack.len() > MAX_UNDO {\n            self.undo_stack.remove(0);\n        }",
        "        if false {\n            self.undo_stack.remove(0);\n        }",
        ["the_history_forgets_its_oldest_move_rather_than_growing_for_ever"],
    ),
    (
        "the history forgets its newest move rather than its oldest",
        "            self.undo_stack.remove(0);",
        "            self.undo_stack.pop();",
        ["the_history_forgets_its_oldest_move_rather_than_growing_for_ever"],
    ),
    (
        "a redone hint is written from the record and not from the solution",
        "            Change::Hint { row, col, .. } => {\n                let answer = self.solution_at(row, col);",
        "            Change::Hint { row, col, old_value, .. } => {\n                let answer = old_value;",
        ["undoing_a_hint_gives_the_hint_back"],
    ),
    # ── Winning ───────────────────────────────────────────────────────────
    (
        "a board that is merely full is a win",
        "        if self.status == GameStatus::Playing && is_grid_complete(&values_array(&self.cells)) {",
        "        if self.status == GameStatus::Playing && !values_array(&self.cells).contains(&0) {",
        ["a_full_board_with_a_digit_in_the_wrong_place_is_not_a_win"],
    ),
    (
        "a win is not put on the scoreboard",
        "            self.stats\n                .record_completion(self.difficulty, self.elapsed_secs());",
        "            let _ = self.difficulty;",
        ["the_last_digit_wins_and_the_time_goes_on_the_board"],
    ),
    # ── The clock ─────────────────────────────────────────────────────────
    (
        "the clock runs while the game is paused or over",
        "        if self.status != GameStatus::Playing {\n            return EventResult::Ignored;\n        }\n        let before = self.elapsed_secs();",
        "        let before = self.elapsed_secs();",
        ["the_clock_stops_while_paused_and_after_a_win"],
    ),
    (
        "the clock counts the times it was woken",
        "        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        "        self.elapsed_ms = self.elapsed_ms.saturating_add(CLOCK_MS);",
        ["the_clock_counts_the_time_it_is_given_not_the_times_it_is_asked"],
    ),
    (
        "a tick that moves nothing still asks for a repaint",
        "        if self.elapsed_secs() == before {\n            // The displayed clock has not moved, so there is nothing to repaint.\n            return EventResult::Ignored;\n        }",
        "        if false {\n            // The displayed clock has not moved, so there is nothing to repaint.\n            return EventResult::Ignored;\n        }",
        ["a_tick_that_does_not_move_the_displayed_clock_asks_for_no_repaint"],
    ),
    (
        "the clock is woken even when there is nothing to count",
        "        if self.status == GameStatus::Playing {\n            Some(Duration::from_millis(CLOCK_MS))\n        } else {\n            None\n        }",
        "        Some(Duration::from_millis(CLOCK_MS))",
        ["the_clock_is_only_woken_while_there_is_time_to_count"],
    ),
    (
        "the clock wraps at an hour",
        "    let mins = secs.wrapping_div(60);",
        "    let mins = secs.wrapping_div(60).wrapping_rem(60);",
        ["the_clock_counts_past_an_hour_rather_than_wrapping"],
    ),
    (
        "the clock shows seconds before minutes",
        "    let mins = secs.wrapping_div(60);\n    let rest = secs.wrapping_rem(60);",
        "    let mins = secs.wrapping_rem(60);\n    let rest = secs.wrapping_div(60);",
        ["the_clock_counts_past_an_hour_rather_than_wrapping"],
    ),
    (
        "the clock is read in milliseconds",
        "    pub fn elapsed_secs(&self) -> u64 {",
        "    pub fn elapsed_secs(&self) -> u64 {\n        return self.elapsed_ms;\n        #[allow(unreachable_code)]",
        ["the_clock_counts_the_time_it_is_given_not_the_times_it_is_asked"],
    ),
    (
        "two states share a word",
        "        GameStatus::Won => \"Completed\",",
        "        GameStatus::Won => \"Playing\",",
        ["the_status_word_and_its_colour_agree_with_the_state"],
    ),
    (
        "two states share a colour",
        "        GameStatus::Won => GREEN,",
        "        GameStatus::Won => BLUE,",
        ["the_status_word_and_its_colour_agree_with_the_state"],
    ),
    # ── The keyboard ──────────────────────────────────────────────────────
    (
        "letting a key go counts as pressing it",
        "        if !key.pressed {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["letting_a_key_go_is_not_pressing_it"],
    ),
    (
        "a key held with alt or the windows key is taken by the game",
        "        if key.modifiers.alt || key.modifiers.super_key {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_key_held_with_alt_or_the_windows_key_belongs_to_the_desktop"],
    ),
    (
        "ctrl is ignored, so a plain z is an undo",
        "        if key.modifiers.ctrl {\n            let intent = match key.key {",
        "        if true {\n            let intent = match key.key {",
        ["a_plain_z_is_not_an_undo"],
    ),
    (
        "ctrl-z and ctrl-y are the wrong way round",
        "                Key::Z => Intent::Undo,\n                Key::Y => Intent::Redo,",
        "                Key::Z => Intent::Redo,\n                Key::Y => Intent::Undo,",
        ["ctrl_z_and_ctrl_y_take_back_and_put_back"],
    ),
    (
        "ctrl and a number all deal the same level",
        "                Key::Num2 => Intent::SetDifficulty(Difficulty::Medium),",
        "                Key::Num2 => Intent::SetDifficulty(Difficulty::Easy),",
        ["ctrl_and_a_number_deals_that_level"],
    ),
    (
        "a number key writes the digit after its own",
        "            Key::Num4 => Intent::Digit(4),",
        "            Key::Num4 => Intent::Digit(5),",
        ["a_number_key_writes_its_own_digit"],
    ),
    (
        "backspace does not erase",
        "            Key::Delete | Key::Backspace => Intent::Erase,",
        "            Key::Delete => Intent::Erase,",
        ["delete_and_backspace_both_erase"],
    ),
    (
        "the arrow keys are transposed",
        "            Key::Up => Intent::Move(Dir::Up),\n            Key::Down => Intent::Move(Dir::Down),\n            Key::Left => Intent::Move(Dir::Left),\n            Key::Right => Intent::Move(Dir::Right),",
        "            Key::Up => Intent::Move(Dir::Left),\n            Key::Down => Intent::Move(Dir::Right),\n            Key::Left => Intent::Move(Dir::Up),\n            Key::Right => Intent::Move(Dir::Down),",
        ["the_arrow_keys_walk_the_selection_about"],
    ),
    (
        "n does not toggle the marks",
        "            Key::N => Intent::ToggleNotes,",
        "            Key::N => Intent::Hint,",
        ["the_letter_keys_do_what_the_keypad_does"],
    ),
    (
        "h does not ask for a hint",
        "            Key::H => Intent::Hint,",
        "            Key::H => Intent::ToggleNotes,",
        ["the_letter_keys_do_what_the_keypad_does"],
    ),
    (
        "p does not pause",
        "            Key::P => Intent::Pause,",
        "            Key::P => Intent::NewGame,",
        ["the_letter_keys_do_what_the_keypad_does"],
    ),
    (
        "d does not cycle the difficulty",
        "            Key::D => Intent::CycleDifficulty,",
        "            Key::D => Intent::NewGame,",
        ["the_letter_keys_do_what_the_keypad_does"],
    ),
    (
        "f2 does not deal a new game",
        "            Key::F2 => Intent::NewGame,",
        "            Key::F2 => Intent::Pause,",
        ["the_letter_keys_do_what_the_keypad_does"],
    ),
    (
        "a key this game has no use for is answered anyway",
        "            _ => return EventResult::Ignored,\n        };\n        self.apply(intent)\n    }\n\n    fn handle_mouse",
        "            _ => Intent::NewGame,\n        };\n        self.apply(intent)\n    }\n\n    fn handle_mouse",
        ["a_key_this_game_has_no_use_for_is_left_alone"],
    ),
    # ── The mouse ─────────────────────────────────────────────────────────
    (
        "any button plays the game",
        "        let MouseEventKind::Press(MouseButton::Left) = event.kind else {\n            return EventResult::Ignored;\n        };",
        "        let MouseEventKind::Press(_) = event.kind else {\n            return EventResult::Ignored;\n        };",
        ["only_the_left_button_plays_the_game"],
    ),
    (
        "moving the mouse over a square selects it",
        "        let MouseEventKind::Press(MouseButton::Left) = event.kind else {\n            return EventResult::Ignored;\n        };",
        "        if matches!(event.kind, MouseEventKind::Leave) {\n            return EventResult::Ignored;\n        }",
        ["moving_the_mouse_or_letting_the_button_go_is_not_a_click"],
    ),
    (
        "a click is read against a fixed size rather than the drawn one",
        "            .frame(self.size.0, self.size.1)",
        "            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)",
        ["a_click_is_read_against_the_size_the_frame_was_drawn_at"],
    ),
    (
        "a click on a square selects its transpose",
        "            Some(Target::Cell(r, c)) => Intent::Select(r, c),",
        "            Some(Target::Cell(r, c)) => Intent::Select(c, r),",
        ["clicking_a_square_selects_that_square_and_no_other"],
    ),
    (
        "a click on empty space is a click on something",
        "            None => return EventResult::Ignored,",
        "            None => Intent::NewGame,",
        [
            "clicking_outside_everything_does_nothing",
            "clicking_the_gap_between_two_squares_selects_neither",
        ],
    ),
    (
        "the erase key asks for a hint",
        "            Some(Target::Erase) => Intent::Erase,",
        "            Some(Target::Erase) => Intent::Hint,",
        ["the_keypad_keys_do_what_they_say"],
    ),
    (
        "the notes key asks for a hint",
        "            Some(Target::Notes) => Intent::ToggleNotes,",
        "            Some(Target::Notes) => Intent::Hint,",
        ["the_keypad_keys_do_what_they_say"],
    ),
    (
        "the undo and redo keys are the wrong way round",
        "            Some(Target::Undo) => Intent::Undo,\n            Some(Target::Redo) => Intent::Redo,",
        "            Some(Target::Undo) => Intent::Redo,\n            Some(Target::Redo) => Intent::Undo,",
        ["the_keypad_keys_do_what_they_say"],
    ),
    (
        "the pause chip deals a new game",
        "            Some(Target::Pause) => Intent::Pause,",
        "            Some(Target::Pause) => Intent::NewGame,",
        ["the_header_chips_do_what_they_say"],
    ),
    (
        "the difficulty chip deals the same level again",
        "            Some(Target::Difficulty) => Intent::CycleDifficulty,",
        "            Some(Target::Difficulty) => Intent::NewGame,",
        ["the_header_chips_do_what_they_say"],
    ),
    (
        "a keypad digit writes the digit after it",
        "            Some(Target::Digit(d)) => Intent::Digit(d),",
        "            Some(Target::Digit(d)) => Intent::Digit(d.wrapping_add(1)),",
        ["the_keypad_keys_do_what_they_say"],
    ),
    # ── Layout ────────────────────────────────────────────────────────────
    (
        "the header is dropped before the footer",
        "pub const BAND_DROP_ORDER: [usize; 3] = [2, 1, 0];",
        "pub const BAND_DROP_ORDER: [usize; 3] = [0, 1, 2];",
        ["the_bands_are_dropped_from_the_bottom_up_as_the_window_shrinks"],
    ),
    (
        "the board keeps no share of the window",
        "pub const BOARD_SHARE: f32 = 0.5;",
        "pub const BOARD_SHARE: f32 = 0.0;",
        ["the_board_keeps_its_share_of_every_window"],
    ),
    (
        "a margin may be wider than the window it is taken from",
        "        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0).min(w.min(h) / 4.0);",
        "        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0);",
        ["a_margin_is_never_more_than_a_quarter_of_the_side_it_is_taken_from"],
    ),
    (
        "a band that did not fit is a flat strip rather than gone",
        "        let header = if head_h > 0.0 {\n            Rect::new(0.0, 0.0, w, head_h)\n        } else {\n            Rect::EMPTY\n        };",
        "        let header = Rect::new(0.0, 0.0, w, head_h);",
        ["a_band_that_did_not_fit_is_gone_rather_than_flat"],
    ),
    (
        "no band is ever given up",
        "        for &i in &BAND_DROP_ORDER {\n            if wants.iter().sum::<f32>() <= budget {\n                break;\n            }",
        "        for &i in &BAND_DROP_ORDER {\n            if true {\n                break;\n            }",
        ["the_board_keeps_its_share_of_every_window"],
    ),
    (
        "the keypad sits under the footer",
        "        let keypad = if pad_h > 0.0 {\n            Rect::new(0.0, (h - foot_h - pad_h).max(0.0), w, pad_h)",
        "        let keypad = if pad_h > 0.0 {\n            Rect::new(0.0, (h - pad_h).max(0.0), w, pad_h)",
        ["the_bands_stack_down_the_window_in_the_order_they_are_named"],
    ),
    (
        "the footer is drawn at the top",
        "            Rect::new(0.0, (h - foot_h).max(0.0), w, foot_h)",
        "            Rect::new(0.0, 0.0, w, foot_h)",
        ["the_bands_stack_down_the_window_in_the_order_they_are_named"],
    ),
    (
        "the board starts at the top of the window rather than under the header",
        "        let board = Rect::new(\n            pad,\n            head_h + pad,",
        "        let board = Rect::new(\n            pad,\n            pad,",
        ["the_bands_stack_down_the_window_in_the_order_they_are_named"],
    ),
    (
        "the board's band ignores the bands below it",
        "            (h - head_h - pad_h - foot_h - pad * 2.0).max(0.0),",
        "            (h - head_h - pad * 2.0).max(0.0),",
        [
            "the_bands_stack_down_the_window_in_the_order_they_are_named",
            "no_cell_is_drawn_over_a_band_or_outside_the_window",
        ],
    ),
    (
        "a cell is sized from the width alone",
        "        let natural = (board.w / side).min(board.h / side);",
        "        let natural = board.w / side;",
        ["no_cell_is_drawn_over_a_band_or_outside_the_window"],
    ),
    (
        "a cell smaller than a pixel is rounded up to one",
        "        let (step, cell) = if natural < 1.0 {\n            (0.0, 0.0)",
        "        let (step, cell) = if natural < 1.0 {\n            (1.0, 1.0)",
        ["a_window_with_no_room_for_a_board_draws_none_rather_than_a_wrong_one"],
    ),
    (
        "the gap between cells is added to the step rather than taken out of the cell",
        "                (natural - (natural * 0.06).clamp(0.0, 3.0)).max(1.0),",
        "                natural,",
        ["two_cells_never_cover_the_same_pixel_and_there_is_a_gap_between_them"],
    ),
    (
        "the board is pinned to the corner of its band rather than centred",
        "            board.x + (board.w - grid_w).max(0.0) / 2.0,\n            board.y + (board.h - grid_w).max(0.0) / 2.0,",
        "            board.x,\n            board.y,",
        ["the_board_is_a_square_sitting_in_the_middle_of_its_band"],
    ),
    (
        "the board is a rectangle rather than a square",
        "        let grid_w = step * side;",
        "        let grid_w = step * side * 1.5;",
        ["the_board_is_a_square_sitting_in_the_middle_of_its_band"],
    ),
    (
        "a square off the board still has a rectangle",
        "        if self.cell <= 0.0 || row >= GRID_SIZE || col >= GRID_SIZE {",
        "        if self.cell <= 0.0 {",
        ["a_square_off_the_board_has_no_rectangle"],
    ),
    (
        "a cell's rectangle counts columns down and rows across",
        "            self.grid.x + col as f32 * self.step,\n            self.grid.y + row as f32 * self.step,",
        "            self.grid.x + row as f32 * self.step,\n            self.grid.y + col as f32 * self.step,",
        ["a_cell_moves_right_with_its_column_and_down_with_its_row"],
    ),
    (
        "a box encloses eight cells rather than nine",
        "                .saturating_add(BOX_SIZE.saturating_sub(1)),\n            box_col\n                .saturating_mul(BOX_SIZE)\n                .saturating_add(BOX_SIZE.saturating_sub(1)),",
        "                .saturating_add(BOX_SIZE.saturating_sub(2)),\n            box_col\n                .saturating_mul(BOX_SIZE)\n                .saturating_add(BOX_SIZE.saturating_sub(2)),",
        ["a_box_encloses_exactly_its_own_nine_cells"],
    ),
    (
        "a keypad key is as wide as the whole strip",
        "        let step = inner / KEYPAD.len() as f32;",
        "        let step = inner;",
        ["the_keypad_keys_sit_in_a_row_and_do_not_overlap"],
    ),
    (
        "the keypad keys all sit on top of each other",
        "            self.keypad.x + self.pad + i as f32 * step,",
        "            self.keypad.x + self.pad,",
        ["the_keypad_keys_sit_in_a_row_and_do_not_overlap"],
    ),
    (
        "a key is wider than its own share of the strip",
        "        let kw = (step - gap).max(1.0).min(step);",
        "        let kw = (step + gap).max(1.0);",
        ["the_keypad_keys_sit_in_a_row_and_do_not_overlap"],
    ),
    (
        "a key is taller than the strip it sits in",
        "        let kh = (self.keypad.h - self.pad).max(1.0).min(self.keypad.h);",
        "        let kh = (self.keypad.h + self.pad).max(1.0);",
        ["the_keypad_keys_sit_in_a_row_and_do_not_overlap"],
    ),
    (
        "a keypad key past the last one still has a rectangle",
        "        if !self.shows(self.keypad) || i >= KEYPAD.len() {",
        "        if !self.shows(self.keypad) {",
        ["the_keypad_keys_sit_in_a_row_and_do_not_overlap"],
    ),
    (
        "the header chips all sit on top of each other",
        "        let x = right - (w + gap) * (i as f32 + 1.0) + gap;",
        "        let x = right - (w + gap) + gap;",
        ["the_header_chips_sit_side_by_side_inside_the_header"],
    ),
    (
        "a header chip runs off the left of the header",
        "            x.max(self.header.x),",
        "            x,",
        ["the_header_chips_sit_side_by_side_inside_the_header"],
    ),
    (
        "a band nought pixels tall is worth drawing into",
        "        !r.is_empty() && r.w > 0.0 && r.h > 0.0",
        "        r.w >= 0.0 && r.h >= 0.0",
        ["a_band_that_did_not_fit_is_gone_rather_than_flat"],
    ),
    # ── Drawing ───────────────────────────────────────────────────────────
    (
        "a label may start left of the box it is centred in",
        "    let x = (r.x + (r.w - w) / 2.0).max(r.x);",
        "    let x = r.x + (r.w - w) / 2.0;",
        ["no_text_is_drawn_outside_the_window_it_belongs_to"],
    ),
    (
        "a label may start above the box it is centred in",
        "        r.y + ((r.h - line_h) / 2.0).max(0.0),\n        s,\n        size,\n        color,\n        weight,\n        Some((r.right() - x).max(0.0)),",
        "        r.y + (r.h - line_h) / 2.0,\n        s,\n        size,\n        color,\n        weight,\n        Some((r.right() - x).max(0.0)),",
        ["a_centred_label_starts_inside_its_box_and_may_use_only_what_is_left"],
    ),
    (
        "a left-aligned label may start above its box",
        "        // See `centred_in`: a line taller than its box must not start above it.\n        r.y + ((r.h - line_h) / 2.0).max(0.0),",
        "        r.y + (r.h - line_h) / 2.0,",
        ["no_text_is_drawn_outside_the_window_it_belongs_to"],
    ),
    (
        "the width a label may use is measured from the box and not from where it starts",
        "        Some((r.right() - x).max(0.0)),",
        "        Some(r.w),",
        ["a_centred_label_starts_inside_its_box_and_may_use_only_what_is_left"],
    ),
    (
        "a chip records no hit box",
        "    fill(f, r, SURFACE0, 5.0);\n    f.hit(target, r);",
        "    fill(f, r, SURFACE0, 5.0);\n    let _ = target;",
        ["every_control_the_program_has_can_be_reached_with_a_mouse"],
    ),
    (
        "the background is the board's rather than the window's",
        "        fill(&mut f, l.window, BASE, 0.0);",
        "        fill(&mut f, l.grid, BASE, 0.0);",
        ["a_window_too_small_for_anything_still_draws_something"],
    ),
    (
        "a paused game does not say how to come back",
        "            if self.status == GameStatus::Paused {\n                \"Resume\"\n            } else {\n                \"Pause\"\n            },",
        "            \"Pause\",",
        [
            "pausing_hides_the_board",
            "the_pause_chip_offers_to_come_back_while_the_game_is_paused",
        ],
    ),
    (
        "the header does not say how far along the player is",
        "                \"{}   {}/{TOTAL_CELLS}   Hints {}   Notes {}\",",
        "                \"{}   {}   Hints {}   Notes {}\",",
        ["the_header_shows_the_clock_the_state_and_how_far_along_the_player_is"],
    ),
    (
        "the header's state line is always the same colour",
        "            status_color(self.status),\n            FontWeightHint::Regular,",
        "            BLUE,\n            FontWeightHint::Regular,",
        ["the_state_line_is_drawn_in_the_colour_of_the_state"],
    ),
    (
        "the clock is not in the header",
        "            &format!(\"Sudoku  {}\", format_time(self.elapsed_secs())),",
        "            \"Sudoku\",",
        ["the_header_shows_the_clock_the_state_and_how_far_along_the_player_is"],
    ),
    (
        "a square records no hit box",
        "        f.hit(Target::Cell(row, col), r);",
        "        let _ = (row, col);",
        ["every_square_records_a_hit_box_where_it_was_painted"],
    ),
    (
        "a square's hit box is its neighbour's",
        "        f.hit(Target::Cell(row, col), r);",
        "        f.hit(Target::Cell(row, col), l.cell_rect(row, col.wrapping_add(1)));",
        ["every_square_records_a_hit_box_where_it_was_painted"],
    ),
    (
        "every square is outlined, not only the selected one",
        "        if selected {\n            stroke(f, r, BLUE, 2.0, l.cell * 0.06);\n        }",
        "        stroke(f, r, BLUE, 2.0, l.cell * 0.06);",
        ["only_the_selected_square_is_outlined"],
    ),
    (
        "the selected square is shaded like its neighbours",
        "        let bg = if hidden {\n            SURFACE0\n        } else if selected {\n            SURFACE2",
        "        let bg = if hidden {\n            SURFACE0\n        } else if selected {\n            SURFACE1",
        ["the_selected_square_is_shaded_differently_from_its_neighbours"],
    ),
    (
        "a paused game keeps its digits on screen",
        "        if hidden {\n            // A pause that leaves the board on screen has paused the clock and\n            // the keyboard but not the thing a player pauses to stop looking at.\n            return;\n        }",
        "        if false {\n            return;\n        }",
        ["pausing_hides_the_board"],
    ),
    (
        "a digit that breaks a rule is drawn like any other",
        "        let color = if conflicting {\n            RED\n        } else {",
        "        let color = if false {\n            RED\n        } else {",
        ["a_digit_that_breaks_a_rule_is_drawn_in_red"],
    ),
    (
        "a hint is drawn in the player's colour",
        "                Origin::Hint => PEACH,",
        "                Origin::Hint => BLUE,",
        ["a_clue_a_hint_and_the_players_own_digit_are_three_different_colours"],
    ),
    (
        "a clue is drawn in the player's colour",
        "                Origin::Given => TEXT_COLOR,",
        "                Origin::Given => BLUE,",
        ["a_clue_a_hint_and_the_players_own_digit_are_three_different_colours"],
    ),
    (
        "a clue is not drawn any bolder than the player's own digit",
        "        let weight = if cell.origin == Origin::Given {\n            FontWeightHint::Bold",
        "        let weight = if cell.origin == Origin::Given {\n            FontWeightHint::Regular",
        ["a_clue_a_hint_and_the_players_own_digit_are_three_different_colours"],
    ),
    (
        "a filled square draws its marks instead of its digit",
        "        if cell.is_empty() {\n            self.draw_notes(f, l, r, cell);\n            return;\n        }",
        "        self.draw_notes(f, l, r, cell);\n        if cell.is_empty() {\n            return;\n        }",
        ["a_square_with_a_digit_in_it_draws_no_marks"],
    ),
    (
        "the marks are stacked in one place",
        "            let nrow = slot.wrapping_div(BOX_SIZE) as f32;\n            let ncol = slot.wrapping_rem(BOX_SIZE) as f32;",
        "            let nrow = 0.0f32;\n            let ncol = 0.0f32;",
        ["nine_marks_go_in_nine_places"],
    ),
    (
        "the marks are laid out down the columns rather than across the rows",
        "            let nrow = slot.wrapping_div(BOX_SIZE) as f32;\n            let ncol = slot.wrapping_rem(BOX_SIZE) as f32;",
        "            let nrow = slot.wrapping_rem(BOX_SIZE) as f32;\n            let ncol = slot.wrapping_div(BOX_SIZE) as f32;",
        ["only_the_marks_that_are_set_are_drawn_and_they_read_across"],
    ),
    (
        "a mark that is not set is drawn anyway",
        "            if !cell.has_note(digit) {\n                continue;\n            }",
        "            if false {\n                continue;\n            }",
        ["only_the_marks_that_are_set_are_drawn_and_they_read_across"],
    ),
    (
        "the notes key is not lit while note mode is on",
        "                Target::Notes if self.note_mode => TEAL,",
        "                Target::Notes if self.note_mode => SUBTEXT0,",
        ["a_key_that_would_do_nothing_is_drawn_greyed_out"],
    ),
    (
        "the hint key stays lit when the hints are gone",
        "                Target::Hint if self.hints_remaining() == 0 => OVERLAY0,",
        "                Target::Hint if self.hints_remaining() == 0 => PEACH,",
        ["the_hint_key_greys_out_when_the_hints_are_gone"],
    ),
    (
        "the undo key stays lit with nothing to take back",
        "                Target::Undo if self.undo_stack.is_empty() => OVERLAY0,",
        "                Target::Undo if self.undo_stack.is_empty() => MAUVE,",
        ["a_key_that_would_do_nothing_is_drawn_greyed_out"],
    ),
    (
        "the redo key stays lit with nothing to put back",
        "                Target::Redo if self.redo_stack.is_empty() => OVERLAY0,",
        "                Target::Redo if self.redo_stack.is_empty() => MAUVE,",
        ["a_key_that_would_do_nothing_is_drawn_greyed_out"],
    ),
    (
        "the footer does not say whether the board holds together",
        "                if self.is_valid() {\n                    \"no conflicts\"\n                } else {\n                    \"conflicts\"\n                }",
        "                \"no conflicts\"",
        ["the_footer_shows_the_record_and_whether_the_board_holds_together"],
    ),
    (
        "the footer shows no record",
        "            .map_or_else(|| \"--:--\".to_string(), format_time);",
        "            .map_or_else(|| \"--:--\".to_string(), |_| \"--:--\".to_string());",
        ["the_footer_shows_the_record_and_whether_the_board_holds_together"],
    ),
    (
        "the footer counts only the level being played",
        "                self.stats.total_completed(),",
        "                self.stats.games_completed(self.difficulty),",
        ["the_footer_counts_every_level_and_not_just_the_one_being_played"],
    ),
    # ── The window ────────────────────────────────────────────────────────
    (
        "a resize is ignored",
        "            app.resize(*width as f32, *height as f32);\n            EventResult::Consumed",
        "            EventResult::Consumed",
        ["resizing_the_window_moves_the_board"],
    ),
    (
        "the ticks never reach the clock",
        "        Event::Tick { elapsed_ms } => app.tick(*elapsed_ms),",
        "        Event::Tick { .. } => EventResult::Ignored,",
        ["the_clock_counts_the_time_it_is_given_not_the_times_it_is_asked"],
    ),
    (
        "the keyboard is not wired to the window",
        "        Event::Key(key) => app.handle_key(key),",
        "        Event::Key(_) => EventResult::Ignored,",
        ["a_number_key_writes_its_own_digit"],
    ),
    (
        "the mouse is not wired to the window",
        "        Event::Mouse(mouse) => app.handle_mouse(mouse),",
        "        Event::Mouse(_) => EventResult::Ignored,",
        ["clicking_a_square_selects_that_square_and_no_other"],
    ),
    (
        "the events this game has no use for are answered anyway",
        "        _ => EventResult::Ignored,\n    }\n}\n\nimpl App for SudokuApp {",
        "        _ => EventResult::Consumed,\n    }\n}\n\nimpl App for SudokuApp {",
        ["the_events_this_game_has_no_use_for_are_left_alone"],
    ),
    (
        "the window does not close when it is asked to",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        if false {\n            return Response::Exit;\n        }",
        ["asking_the_window_to_close_closes_it"],
    ),
    (
        "the window is repainted whether or not anything changed",
        "            EventResult::Consumed => Response::Redraw,\n            EventResult::Ignored => Response::Idle,",
        "            EventResult::Consumed | EventResult::Ignored => Response::Redraw,",
        ["the_window_is_only_repainted_when_something_changed"],
    ),
    (
        "render does not remember the size it drew at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["a_click_is_read_against_the_size_the_frame_was_drawn_at"],
    ),
    (
        "render draws something other than the frame",
        "        self.frame(width, height).into_tree()",
        "        Frame::new(width, height).into_tree()",
        ["what_the_window_draws_is_what_the_frame_drew"],
    ),
    (
        "the program calls itself two different things",
        "        \"sudoku\".to_string()",
        "        \"Sudoku game\".to_string()",
        ["the_program_names_itself_the_same_way_everywhere"],
    ),
]


def run_tests():
    out = subprocess.run(
        [
            "python",
            "scripts/run-timeout.py",
            "240",
            "cargo",
            "test",
            "-p",
            "sudoku",
            "--target",
            "x86_64-pc-windows-gnu",
        ],
        capture_output=True,
        text=True,
        cwd=Path(__file__).parent.parent.parent,
    )
    failed = set(re.findall(r"^    tests::(\S+)$", out.stdout, re.M))
    compiled = "could not compile" not in out.stdout + out.stderr
    timed_out = out.returncode == 124
    # An unbounded loop does not fail a test; it runs until the runner kills it.
    # A harness that only counted named failures would score that as a mutant
    # nobody noticed, which is the opposite of the truth: a hang IS the symptom.
    # Same for a mutant that aborts the process before any test can report.
    crashed = compiled and not timed_out and not failed and out.returncode != 0
    return compiled, failed, timed_out, crashed, out


def main():
    # Written fresh every run and removed at the end.  It exists only so a
    # Ctrl-C mid-mutation leaves the real program on disk.  It must never be
    # *reused* across runs: a stale backup restored over a fixed source silently
    # throws away every fix made since, and then reports the same survivors --
    # output that looks like evidence and is not.
    original = SRC.read_text(encoding="utf-8", newline="")
    BAK.write_text(original, encoding="utf-8", newline="")
    verdicts = []
    only = sys.argv[1:]
    try:
        for name, old, new, expect in MUTATIONS:
            if only and not any(o in name for o in only):
                continue
            if original.count(old) != 1:
                verdicts.append((name, f"SKIP anchor appears {original.count(old)}x"))
                print(f"[skip] {name}: anchor appears {original.count(old)} times")
                continue
            SRC.write_text(original.replace(old, new), encoding="utf-8", newline="")
            compiled, failed, timed_out, crashed, out = run_tests()
            if timed_out:
                verdicts.append((name, "caught by a hang"))
                print(f"[ok]   {name}: caught \u2014 the suite hung")
            elif crashed:
                verdicts.append((name, "caught by a crash"))
                print(
                    f"[ok]   {name}: caught \u2014 the harness died (exit {out.returncode})"
                )
            elif not compiled:
                verdicts.append((name, "SKIP did not compile"))
                print(f"[skip] {name}: mutant did not compile")
                print(out.stdout[-1500:])
            elif set(expect) <= failed:
                verdicts.append((name, f"caught by {len(failed)} test(s)"))
                print(f"[ok]   {name}: caught ({', '.join(sorted(failed))})")
            elif failed:
                verdicts.append((name, f"WRONG TESTS: {sorted(failed)}"))
                print(f"[??]   {name}: expected {expect}, got {sorted(failed)}")
            else:
                verdicts.append((name, "SURVIVED"))
                print(f"[BAD]  {name}: SURVIVED \u2014 no test failed")
            SRC.write_text(original, encoding="utf-8", newline="")
    finally:
        # Whatever happens -- a Ctrl-C, an exception, a full disk -- the tree is
        # left with the real program in it and not a mutant, and with no backup
        # for the next run to mistake for the truth.
        SRC.write_text(original, encoding="utf-8", newline="")
        BAK.unlink(missing_ok=True)
    print("\n=== summary ===")
    for name, v in verdicts:
        print(f"{v:<34} {name}")


if __name__ == "__main__":
    main()
