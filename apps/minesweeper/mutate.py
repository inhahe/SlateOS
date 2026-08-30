"""Mutation test for minesweeper's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Minesweeper is the fourteenth application in this campaign, and the one where
the gap between "tested" and "reachable" was widest in a particular direction:
the old file had roughly eleven hundred lines of tests and not one `Event::`
anywhere in it, so every one of them called `reveal`/`flag`/`chord` directly,
with coordinates it had made up.  The program had no window, so there was no
path from a click to those functions at all -- which means the suite could not
have noticed if `handle_mouse` had been deleted, because there was nothing to
delete.  It also could not notice that `revealed_count` and `flags_placed`
disagreed with the cells they counted, because it asked the counters.

Usage:  python -u apps/minesweeper/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── Steps, directions and neighbours ──────────────────────────────────
    (
        "a step off the top edge clamps instead of falling off",
        "            Self::Back => at.checked_sub(1),",
        "            Self::Back => Some(at.saturating_sub(1)),",
        ["a_step_back_from_the_edge_falls_off_rather_than_wrapping"],
    ),
    (
        "a step forward does not move",
        "            Self::Fwd => at.checked_add(1),",
        "            Self::Fwd => Some(at),",
        ["a_step_back_from_the_edge_falls_off_rather_than_wrapping"],
    ),
    (
        "up moves down",
        "            Self::Up => (Step::Back, Step::Stay),",
        "            Self::Up => (Step::Fwd, Step::Stay),",
        ["the_four_arrow_directions_are_the_four_unit_steps"],
    ),
    (
        "left moves up",
        "            Self::Left => (Step::Stay, Step::Back),",
        "            Self::Left => (Step::Back, Step::Stay),",
        ["the_four_arrow_directions_are_the_four_unit_steps"],
    ),
    (
        # Not a deletion: NEIGHBOURS is `[(Step, Step); 8]`, so removing an
        # element does not compile and the sweep learns nothing. Replacing the
        # bottom-right corner with "stay where you are" is the same fault --
        # a neighbour that is not there -- expressed in a way the compiler
        # accepts.
        "the bottom-right neighbour is the cell itself",
        "    (Step::Fwd, Step::Fwd),\n];",
        "    (Step::Stay, Step::Stay),\n];",
        ["the_eight_neighbour_offsets_are_eight_distinct_moves_and_none_is_staying_put"],
    ),
    (
        "a cell counts itself among its neighbours",
        "    (Step::Stay, Step::Back),",
        "    (Step::Stay, Step::Stay),",
        ["the_eight_neighbour_offsets_are_eight_distinct_moves_and_none_is_staying_put"],
    ),
    (
        "a neighbour one past the right edge is on the board",
        "            (r < rows && c < cols).then_some((r, c))",
        "            (r < rows && c <= cols).then_some((r, c))",
        ["every_neighbour_is_on_the_board_and_none_is_the_cell_itself"],
    ),
    # ── The three boards ──────────────────────────────────────────────────
    (
        "beginner is eight columns wide",
        "    pub fn cols(self) -> usize {\n        match self {\n            Self::Beginner => 9,",
        "    pub fn cols(self) -> usize {\n        match self {\n            Self::Beginner => 8,",
        ["the_three_difficulties_are_the_real_minesweeper_boards"],
    ),
    (
        "expert is square",
        "            Self::Expert => 30,\n        }\n    }\n\n    /// The board's height, in cells.",
        "            Self::Expert => 16,\n        }\n    }\n\n    /// The board's height, in cells.",
        ["the_three_difficulties_are_the_real_minesweeper_boards"],
    ),
    (
        "beginner buries nine mines",
        "            Self::Beginner => 10,\n            Self::Intermediate => 40,",
        "            Self::Beginner => 9,\n            Self::Intermediate => 40,",
        ["the_three_difficulties_are_the_real_minesweeper_boards"],
    ),
    (
        "expert buries fewer mines than intermediate",
        "            Self::Expert => 99,",
        "            Self::Expert => 39,",
        ["the_three_difficulties_are_the_real_minesweeper_boards"],
    ),
    (
        "a board's cell count is rows plus columns",
        "        self.rows().saturating_mul(self.cols())",
        "        self.rows().saturating_add(self.cols())",
        ["the_three_difficulties_are_the_real_minesweeper_boards"],
    ),
    (
        "the difficulty cycle stops at expert",
        "            Self::Expert => Self::Beginner,",
        "            Self::Expert => Self::Expert,",
        ["cycling_difficulty_visits_all_three_and_comes_home"],
    ),
    (
        "two difficulties share a colour",
        "            Self::Intermediate => YELLOW,",
        "            Self::Intermediate => GREEN,",
        ["every_difficulty_has_its_own_name_and_its_own_colour"],
    ),
    (
        "two difficulties share a name",
        '            Self::Intermediate => "Intermediate",',
        '            Self::Intermediate => "Beginner",',
        ["every_difficulty_has_its_own_name_and_its_own_colour"],
    ),
    # ── Addressing the board ──────────────────────────────────────────────
    (
        "a row one past the bottom is on the board",
        "        if row >= self.rows() || col >= self.cols() {",
        "        if row > self.rows() || col >= self.cols() {",
        ["an_index_and_a_row_and_column_name_the_same_cell"],
    ),
    (
        "an index is built from the row count instead of the column count",
        "        row.checked_mul(self.cols())?.checked_add(col)",
        "        row.checked_mul(self.rows())?.checked_add(col)",
        ["an_index_and_a_row_and_column_name_the_same_cell"],
    ),
    (
        "an index splits into column and row rather than row and column",
        "    Some((index.checked_div(cols)?, index.checked_rem(cols)?))",
        "    Some((index.checked_rem(cols)?, index.checked_div(cols)?))",
        ["an_index_and_a_row_and_column_name_the_same_cell"],
    ),
    # ── Burying the mines ─────────────────────────────────────────────────
    (
        "only the clicked cell is safe, not the ring around it",
        "        let safe: Vec<(usize, usize)> = std::iter::once((safe_row, safe_col))\n            .chain(self.neighbours(safe_row, safe_col))\n            .collect();",
        "        let safe: Vec<(usize, usize)> = std::iter::once((safe_row, safe_col)).collect();",
        ["the_first_click_and_everything_around_it_is_safe"],
    ),
    (
        "the clicked cell itself may hold a mine",
        "        let safe: Vec<(usize, usize)> = std::iter::once((safe_row, safe_col))\n            .chain(self.neighbours(safe_row, safe_col))\n            .collect();",
        "        let safe: Vec<(usize, usize)> = self.neighbours(safe_row, safe_col).collect();",
        ["the_first_click_and_everything_around_it_is_safe"],
    ),
    (
        "every cell is a candidate, safe zone or not",
        "                Some(rc) => !safe.contains(&rc),",
        "                Some(rc) => rc == rc,",
        ["the_first_click_and_everything_around_it_is_safe"],
    ),
    (
        "one mine short",
        "        for &i in candidates.iter().take(self.total_mines()) {",
        "        for &i in candidates.iter().take(self.total_mines().saturating_sub(1)) {",
        ["the_first_click_buries_exactly_the_advertised_number_of_mines"],
    ),
    (
        "the candidates are not shuffled, so every board is the same shape",
        "        self.rng.shuffle(&mut candidates);",
        "        let _ = &mut candidates;",
        ["one_named_seed_names_one_named_board"],
    ),
    (
        "the counts are never computed",
        "        self.compute_counts();\n    }",
        "    }",
        ["every_count_is_the_number_of_mines_around_that_cell"],
    ),
    (
        "a count is of the neighbours that are not mines",
        "                    .filter(|&(r, c)| self.is_mine(r, c))\n                    .count();",
        "                    .filter(|&(r, c)| !self.is_mine(r, c))\n                    .count();",
        ["every_count_is_the_number_of_mines_around_that_cell"],
    ),
    (
        "the last row's counts are never computed",
        "        for row in 0..self.rows() {\n            for col in 0..self.cols() {\n                let n = self",
        "        for row in 0..self.rows().saturating_sub(1) {\n            for col in 0..self.cols() {\n                let n = self",
        ["every_count_is_the_number_of_mines_around_that_cell"],
    ),
    # ── Dealing the next board ────────────────────────────────────────────
    (
        "the next game's seed is this one plus one",
        "        let seed = self.rng.next_u64();",
        "        let seed = self.seed.saturating_add(1);",
        ["a_second_game_is_not_the_first_one_shifted_by_one"],
    ),
    (
        "the next game is dealt at the old difficulty",
        "        *self = Self::with_seed(difficulty, seed);",
        "        *self = Self::with_seed(self.difficulty, seed);",
        ["changing_difficulty_deals_a_board_of_the_new_size"],
    ),
    (
        "a new game keeps the clock running",
        "            elapsed_ms: 0,",
        "            elapsed_ms: 1,",
        ["a_new_game_clears_the_clock_the_flags_and_the_wreckage"],
    ),
    (
        "cycling the difficulty stays where it is",
        "            Action::CycleDifficulty => {\n                self.deal(self.difficulty.next());",
        "            Action::CycleDifficulty => {\n                self.deal(self.difficulty);",
        ["changing_difficulty_deals_a_board_of_the_new_size"],
    ),
    (
        "setting a difficulty by name cycles instead",
        "            Action::SetDifficulty(difficulty) => {\n                self.deal(difficulty);",
        "            Action::SetDifficulty(difficulty) => {\n                self.deal(difficulty.next());",
        ["changing_difficulty_deals_a_board_of_the_new_size"],
    ),
    # ── The cursor ────────────────────────────────────────────────────────
    (
        "the cursor may walk one row past the bottom",
        "        if r >= self.rows() || c >= self.cols() {\n            return EventResult::Ignored;\n        }\n        self.cursor = (r, c);",
        "        if r > self.rows() || c >= self.cols() {\n            return EventResult::Ignored;\n        }\n        self.cursor = (r, c);",
        ["the_cursor_stops_at_the_edges_rather_than_wrapping"],
    ),
    (
        "an arrow key says it moved without moving",
        "        self.cursor = (r, c);\n        EventResult::Consumed\n    }",
        "        EventResult::Consumed\n    }",
        ["the_arrow_keys_walk_the_cursor_one_cell_at_a_time"],
    ),
    # ── Uncovering ────────────────────────────────────────────────────────
    (
        "a finished game still answers a click",
        "    fn reveal(&mut self, row: usize, col: usize) -> EventResult {\n        if self.is_over() {",
        "    fn reveal(&mut self, row: usize, col: usize) -> EventResult {\n        if false {",
        ["a_lost_game_answers_nothing_more"],
    ),
    (
        "a flagged cell can be uncovered by a click",
        "        if cell.state != CellState::Hidden {\n            return EventResult::Ignored;\n        }\n        self.cursor = (row, col);",
        "        if cell.state == CellState::Revealed {\n            return EventResult::Ignored;\n        }\n        self.cursor = (row, col);",
        ["a_flag_stops_the_click_that_would_have_uncovered_the_cell"],
    ),
    (
        "uncovering a cell does not move the cursor to it",
        "        self.cursor = (row, col);\n\n        if self.status == GameStatus::Ready {",
        "        if self.status == GameStatus::Ready {",
        ["uncovering_a_hidden_cell_uncovers_it_and_moves_the_cursor_there"],
    ),
    (
        "the mines are never buried",
        "        if self.status == GameStatus::Ready {\n            self.place_mines(row, col);",
        "        if self.status == GameStatus::Won {\n            self.place_mines(row, col);",
        ["the_first_click_buries_exactly_the_advertised_number_of_mines"],
    ),
    (
        "the first click leaves the game in its pre-dealt state",
        "            self.status = GameStatus::Playing;\n        }",
        "        }",
        ["uncovering_a_hidden_cell_uncovers_it_and_moves_the_cursor_there"],
    ),
    (
        "uncovering a safe cell is what loses the game",
        "        if self.is_mine(row, col) {\n            self.set_state(row, col, CellState::Revealed);",
        "        if !self.is_mine(row, col) {\n            self.set_state(row, col, CellState::Revealed);",
        ["uncovering_a_mine_loses_marks_where_and_shows_every_other_mine"],
    ),
    (
        "uncovering a mine does not end the game",
        "            self.status = GameStatus::Lost;\n            self.losing_cell = Some((row, col));",
        "            self.losing_cell = Some((row, col));",
        ["uncovering_a_mine_loses_marks_where_and_shows_every_other_mine"],
    ),
    (
        "the losing cell is not remembered",
        "            self.losing_cell = Some((row, col));\n            self.reveal_all_mines();",
        "            self.reveal_all_mines();",
        ["the_mine_that_ended_the_game_is_the_one_painted_red"],
    ),
    (
        "a loss leaves the other mines hidden",
        "            self.reveal_all_mines();\n            return EventResult::Consumed;",
        "            return EventResult::Consumed;",
        ["uncovering_a_mine_loses_marks_where_and_shows_every_other_mine"],
    ),
    (
        "a loss erases the flags the player got right",
        "            if cell.is_mine && cell.state == CellState::Hidden {\n                cell.state = CellState::Revealed;",
        "            if cell.is_mine {\n                cell.state = CellState::Revealed;",
        ["a_flagged_mine_stays_flagged_when_the_game_is_lost"],
    ),
    (
        "clearing the board is not a win",
        "        self.flood_reveal(row, col);\n        if self.is_cleared() {",
        "        self.flood_reveal(row, col);\n        if false {",
        ["uncovering_the_last_safe_cell_wins_and_plants_the_flags_you_did_not_need"],
    ),
    (
        "a win leaves the mines unflagged",
        "            self.flag_all_mines();\n        }\n        EventResult::Consumed",
        "        }\n        EventResult::Consumed",
        ["uncovering_the_last_safe_cell_wins_and_plants_the_flags_you_did_not_need"],
    ),
    (
        "a win uncovers the mines rather than flagging them",
        "            if cell.is_mine && cell.state == CellState::Hidden {\n                cell.state = CellState::Flagged;",
        "            if cell.is_mine && cell.state == CellState::Hidden {\n                cell.state = CellState::Revealed;",
        ["uncovering_the_last_safe_cell_wins_and_plants_the_flags_you_did_not_need"],
    ),
    (
        "a board is cleared as soon as one safe cell is open",
        "            .all(|c| c.is_mine || c.state == CellState::Revealed)",
        "            .any(|c| c.is_mine || c.state == CellState::Revealed)",
        ["a_board_is_only_cleared_when_every_safe_cell_is_open"],
    ),
    (
        "a board is only cleared once the mines are uncovered too",
        "            .all(|c| c.is_mine || c.state == CellState::Revealed)",
        "            .all(|c| !c.is_mine || c.state == CellState::Revealed)",
        ["uncovering_the_last_safe_cell_wins_and_plants_the_flags_you_did_not_need"],
    ),
    (
        "a flagged safe cell counts as cleared",
        "            .all(|c| c.is_mine || c.state == CellState::Revealed)",
        "            .all(|c| c.is_mine || c.state != CellState::Hidden)",
        ["a_flag_on_a_safe_cell_is_not_the_same_as_opening_it"],
    ),
    # ── The flood ─────────────────────────────────────────────────────────
    (
        # This was "the flood runs onto the mines", deleting `|| cell.is_mine`
        # from the loop guard, and it survived -- correctly: no mine can reach
        # that stack, so the clause was unreachable and is now gone from the
        # production code. What the flood must still be stopped by is a cell
        # that is not hidden, which is how it terminates at all.
        # (`if false` here would simply never terminate, which the runner would
        # score as a hang and so as "caught" without telling anyone anything.
        # Letting the flood through flags is the interesting half of the guard.)
        "the flood washes over the flags",
        "            if cell.state != CellState::Hidden {",
        "            if cell.state == CellState::Revealed {",
        ["a_flag_dams_the_flood"],
    ),
    (
        "the flood crosses the ones as well as the blanks",
        "            if cell.adjacent == 0 {",
        "            if cell.adjacent <= 1 {",
        ["uncovering_a_cell_that_touches_nothing_opens_the_whole_clearing"],
    ),
    (
        "the flood does not spread at all",
        "                stack.extend(self.neighbours(r, c).collect::<Vec<_>>());",
        "                stack.clear();",
        ["uncovering_a_cell_that_touches_nothing_opens_the_whole_clearing"],
    ),
    (
        "the clicked cell is never opened",
        "        let mut stack = vec![(row, col)];",
        "        let mut stack: Vec<(usize, usize)> = Vec::new();",
        ["uncovering_a_hidden_cell_uncovers_it_and_moves_the_cursor_there"],
    ),
    # ── Flagging ──────────────────────────────────────────────────────────
    (
        "a flag will not go on",
        "            CellState::Hidden => CellState::Flagged,",
        "            CellState::Hidden => CellState::Hidden,",
        ["a_flag_goes_on_and_comes_off_again"],
    ),
    (
        "a flag will not come off",
        "            CellState::Flagged => CellState::Hidden,\n            CellState::Revealed => return EventResult::Ignored,",
        "            CellState::Flagged => CellState::Flagged,\n            CellState::Revealed => return EventResult::Ignored,",
        ["a_flag_goes_on_and_comes_off_again"],
    ),
    (
        "an open cell can be flagged",
        "            CellState::Revealed => return EventResult::Ignored,\n        };",
        "            CellState::Revealed => CellState::Flagged,\n        };",
        ["a_flag_on_an_open_cell_is_refused"],
    ),
    (
        "a flag is refused before the first click, as it used to be",
        "    fn flag(&mut self, row: usize, col: usize) -> EventResult {\n        if self.is_over() {",
        "    fn flag(&mut self, row: usize, col: usize) -> EventResult {\n        if self.is_over() || self.status == GameStatus::Ready {",
        ["a_flag_may_be_planted_before_the_first_click"],
    ),
    (
        "a finished game still takes flags",
        "    fn flag(&mut self, row: usize, col: usize) -> EventResult {\n        if self.is_over() {",
        "    fn flag(&mut self, row: usize, col: usize) -> EventResult {\n        if false {",
        ["a_lost_game_answers_nothing_more"],
    ),
    (
        "flagging does not move the cursor",
        "        self.cursor = (row, col);\n        self.set_state(row, col, next);",
        "        self.set_state(row, col, next);",
        ["flagging_moves_the_cursor_to_what_was_flagged"],
    ),
    (
        "the flags are counted as the cells that are not flagged",
        "            .filter(|c| c.state == CellState::Flagged)",
        "            .filter(|c| c.state != CellState::Flagged)",
        ["a_flag_goes_on_and_comes_off_again"],
    ),
    (
        "the open cells are counted as the covered ones",
        "            .filter(|c| c.state == CellState::Revealed)\n            .count()",
        "            .filter(|c| c.state != CellState::Revealed)\n            .count()",
        ["a_fresh_board_is_ready_empty_and_unmined"],
    ),
    (
        "the mines are counted as the cells without one",
        "        self.cells.iter().filter(|c| c.is_mine).count()",
        "        self.cells.iter().filter(|c| !c.is_mine).count()",
        ["a_fresh_board_is_ready_empty_and_unmined"],
    ),
    (
        "the counter reads flags less mines",
        "        mines.saturating_sub(flags)",
        "        flags.saturating_sub(mines)",
        ["the_counter_is_mines_less_flags_and_goes_below_nought"],
    ),
    (
        "the counter refuses to go below nought",
        "        let mines = i64::try_from(self.total_mines()).unwrap_or(i64::MAX);",
        "        let mines = i64::try_from(self.total_mines().max(self.flag_count())).unwrap_or(i64::MAX);",
        ["the_counter_is_mines_less_flags_and_goes_below_nought"],
    ),
    # ── Chording ──────────────────────────────────────────────────────────
    (
        # This was "a chord fires before the first click", turning
        # `status != Playing` into `is_over()`, and it survived -- correctly.
        # `Ready` implies no revealed cell, so the guard below refuses that
        # case anyway and the two spellings are the same program. The
        # production code now says `is_over()`, and what is left to mutate is
        # the part of the guard that is actually load-bearing: a lost board is
        # covered in revealed numbers, and a chord must not run on one.
        "a chord fires after the game is over",
        "    fn chord(&mut self, row: usize, col: usize) -> EventResult {\n        if self.is_over() {",
        "    fn chord(&mut self, row: usize, col: usize) -> EventResult {\n        if false {",
        ["chording_after_the_game_is_over_does_nothing"],
    ),
    (
        "a chord fires on a covered cell",
        "        if cell.state != CellState::Revealed || cell.is_mine {",
        "        if cell.is_mine {",
        ["chording_a_covered_cell_does_nothing"],
    ),
    (
        "a chord fires whatever the flags say",
        "        if usize::from(cell.adjacent) != flags {",
        "        if usize::from(cell.adjacent) > 99 {",
        ["chording_a_number_whose_flags_are_missing_does_nothing"],
    ),
    (
        # Not "flags are missing": with no flags anywhere the mutant counts
        # eight *unflagged* neighbours, which matches no small count, so that
        # test sees the chord refused exactly as it expects and passes. The
        # test that actually pins which cells count as flags is the one that
        # plants the right ones and requires the chord to fire.
        "a chord counts the unflagged neighbours as its flags",
        "            .filter(|&(r, c)| self.is_flagged(r, c))\n            .count();",
        "            .filter(|&(r, c)| !self.is_flagged(r, c))\n            .count();",
        ["chording_a_satisfied_number_opens_everything_else_around_it"],
    ),
    (
        "a chord with too many flags fires anyway",
        "        if usize::from(cell.adjacent) != flags {",
        "        if usize::from(cell.adjacent) > flags {",
        ["chording_a_number_with_too_many_flags_does_nothing"],
    ),
    (
        "a chord claims to have acted with nothing left to open",
        "        if hidden.is_empty() {\n            return EventResult::Ignored;\n        }",
        "        if hidden.is_empty() {\n            return EventResult::Consumed;\n        }",
        ["chording_a_number_with_nothing_left_around_it_does_nothing"],
    ),
    (
        # Named for what it does rather than what it looks like. A flagged
        # neighbour is not opened by the loop either way -- `reveal` turns away
        # anything that is not `Hidden` -- so the one observable effect is that
        # a number with nothing but flags left around it answers `Consumed`
        # instead of `Ignored`, which is the test named here.
        "a chord counts the flags among the cells it has left to open",
        "                    .is_some_and(|n| n.state == CellState::Hidden)",
        "                    .is_some_and(|n| n.state != CellState::Revealed)",
        ["chording_a_number_with_nothing_left_around_it_does_nothing"],
    ),
    (
        # This was "a chord that hits a mine carries on regardless", deleting a
        # `break` after the losing reveal, and it survived -- correctly. Every
        # `reveal` after the game ends returns `Ignored` without touching a
        # cell, so the loop was already inert and the `break` bought nothing
        # but an early exit; it is gone from the production code. `reveal`'s
        # own `is_over` guard, which is what actually stops the chord, is
        # mutated separately above. What is left here is the loop itself.
        "a chord opens only the first cell it was going to",
        "        for (r, c) in hidden {\n            self.reveal(r, c);\n        }",
        "        for (r, c) in hidden.into_iter().take(1) {\n            self.reveal(r, c);\n        }",
        ["chording_a_satisfied_number_opens_everything_else_around_it"],
    ),
    (
        "a chord does not move the cursor",
        "        self.cursor = (row, col);\n        EventResult::Consumed\n    }\n\n    /// Whether every cell that is not a mine has been uncovered.",
        "        EventResult::Consumed\n    }\n\n    /// Whether every cell that is not a mine has been uncovered.",
        ["chording_a_satisfied_number_opens_everything_else_around_it"],
    ),
    # ── The clock ─────────────────────────────────────────────────────────
    (
        "the clock runs before the first click",
        "        if self.status != GameStatus::Playing {\n            return EventResult::Ignored;\n        }\n        let before = self.elapsed_secs();",
        "        if self.is_over() {\n            return EventResult::Ignored;\n        }\n        let before = self.elapsed_secs();",
        ["the_clock_does_not_run_before_the_first_click"],
    ),
    (
        "the clock counts the times it was woken",
        "        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        "        self.elapsed_ms = self.elapsed_ms.saturating_add(1_000);",
        ["the_clock_counts_the_time_that_passed_not_the_times_it_was_woken"],
    ),
    (
        "every tick asks for a repaint, moved or not",
        "        if self.elapsed_secs() == before {",
        "        if self.elapsed_secs() != before {",
        ["a_tick_that_does_not_move_the_displayed_second_asks_for_no_repaint"],
    ),
    (
        "the displayed clock runs ten times too fast",
        "        self.elapsed_ms.checked_div(1_000).unwrap_or(0)",
        "        self.elapsed_ms.checked_div(100).unwrap_or(0)",
        ["the_clock_counts_the_time_that_passed_not_the_times_it_was_woken"],
    ),
    (
        "the window is woken while there is no clock to move",
        "        if self.status == GameStatus::Playing {\n            Some(Duration::from_millis(CLOCK_MS))",
        "        if !self.is_over() {\n            Some(Duration::from_millis(CLOCK_MS))",
        ["the_window_is_only_woken_while_there_is_a_clock_to_move"],
    ),
    (
        "the clock wraps at the hour",
        "    let mins = secs.checked_div(60).unwrap_or(0);",
        "    let mins = secs.checked_rem(3_600).unwrap_or(0).checked_div(60).unwrap_or(0);",
        ["the_clock_reads_minutes_and_seconds_and_keeps_counting_past_the_hour"],
    ),
    (
        "the clock drops the leading zero on the seconds",
        '    format!("{mins:02}:{rest:02}")',
        '    format!("{mins:02}:{rest}")',
        ["the_clock_reads_minutes_and_seconds_and_keeps_counting_past_the_hour"],
    ),
    # ── Keys ──────────────────────────────────────────────────────────────
    (
        "letting a key go counts as pressing it",
        "        if !key.pressed {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["letting_a_key_go_is_not_pressing_it"],
    ),
    (
        "ctrl-1 opens expert",
        "                Key::Num1 => Difficulty::Beginner,",
        "                Key::Num1 => Difficulty::Expert,",
        ["ctrl_and_a_digit_jumps_straight_to_a_difficulty"],
    ),
    (
        "ctrl is not looked at, so ctrl-right moves the cursor",
        "        if key.modifiers.ctrl {",
        "        if key.modifiers.shift {",
        ["ctrl_with_anything_else_is_left_for_the_window_to_deal_with"],
    ),
    (
        "the super key is not looked at",
        "        if key.modifiers.alt || key.modifiers.super_key {",
        "        if key.modifiers.alt {",
        ["a_key_held_with_alt_or_the_super_key_belongs_to_the_desktop"],
    ),
    (
        "enter no longer uncovers",
        "            Key::Space | Key::Enter => Action::Reveal(row, col),",
        "            Key::Space => Action::Reveal(row, col),",
        ["enter_uncovers_the_same_cell_space_would"],
    ),
    (
        "F uncovers rather than flagging",
        "            Key::F => Action::Flag(row, col),",
        "            Key::F => Action::Reveal(row, col),",
        ["the_keyboard_plays_the_whole_game_without_a_mouse"],
    ),
    (
        "D deals rather than changing level",
        "            Key::D => Action::CycleDifficulty,",
        "            Key::D => Action::NewGame,",
        ["the_letter_keys_do_what_the_footer_says_they_do"],
    ),
    (
        "F2 no longer deals a new board",
        "            Key::N | Key::F2 => Action::NewGame,",
        "            Key::N => Action::NewGame,",
        ["the_letter_keys_do_what_the_footer_says_they_do"],
    ),
    (
        "C flags rather than chording",
        "            Key::C => Action::Chord(row, col),",
        "            Key::C => Action::Flag(row, col),",
        ["c_chords_where_the_cursor_is"],
    ),
    (
        "an arrow key uncovers as well as moving",
        "            Key::Up => Action::Move(Dir::Up),",
        "            Key::Up => Action::Reveal(row, col),",
        ["the_arrow_keys_walk_the_cursor_one_cell_at_a_time"],
    ),
    # ── The pointer ───────────────────────────────────────────────────────
    (
        "a click is read against the default window rather than this one",
        "            .frame(self.size.0, self.size.1)\n            .hit_test(event.x, event.y);",
        "            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)\n            .hit_test(event.x, event.y);",
        ["a_click_is_read_against_the_window_the_player_is_looking_at"],
    ),
    (
        "a click's coordinates are read the wrong way round",
        "            .hit_test(event.x, event.y);",
        "            .hit_test(event.y, event.x);",
        ["clicking_a_cell_uncovers_that_cell_and_no_other"],
    ),
    (
        "the right button uncovers rather than flagging",
        "            (MouseEventKind::Press(MouseButton::Right), Some(Target::Cell(r, c))) => {\n                Action::Flag(r, c)\n            }",
        "            (MouseEventKind::Press(MouseButton::Right), Some(Target::Cell(r, c))) => {\n                Action::Reveal(r, c)\n            }",
        ["the_right_button_flags_and_the_left_uncovers"],
    ),
    (
        "a double-click no longer chords",
        "                MouseEventKind::Press(MouseButton::Middle)\n                | MouseEventKind::DoubleClick(MouseButton::Left),",
        "                MouseEventKind::Press(MouseButton::Middle),",
        ["the_middle_button_and_a_double_click_both_chord"],
    ),
    (
        "the new-game chip changes the level instead",
        "            (MouseEventKind::Press(MouseButton::Left), Some(Target::NewGame)) => Action::NewGame,",
        "            (MouseEventKind::Press(MouseButton::Left), Some(Target::NewGame)) => {\n                Action::CycleDifficulty\n            }",
        ["clicking_the_new_chip_deals_a_board_at_the_same_level"],
    ),
    (
        "the level chip answers any button",
        "            (MouseEventKind::Press(MouseButton::Left), Some(Target::Difficulty)) => {\n                Action::CycleDifficulty\n            }",
        "            (_, Some(Target::Difficulty)) => Action::CycleDifficulty,",
        ["the_chips_answer_only_the_left_button"],
    ),
    (
        "moving the pointer over a cell uncovers it",
        "            (MouseEventKind::Press(MouseButton::Left), Some(Target::Cell(r, c))) => {\n                Action::Reveal(r, c)\n            }",
        "            (_, Some(Target::Cell(r, c))) => Action::Reveal(r, c),",
        ["moving_the_pointer_and_letting_go_of_a_button_are_not_clicks"],
    ),
    # ── Layout ────────────────────────────────────────────────────────────
    (
        "the margin may grow wider than the window",
        "        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0).min(w.min(h) / 4.0);",
        "        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0);",
        ["a_margin_never_grows_wider_than_the_thing_it_indents"],
    ),
    (
        "the header is given up before the footer",
        "pub const BAND_DROP_ORDER: [usize; 2] = [1, 0];",
        "pub const BAND_DROP_ORDER: [usize; 2] = [0, 1];",
        ["a_window_that_shrinks_gives_up_the_footer_before_the_header"],
    ),
    (
        "the board is given only a sixth of the window",
        "pub const BOARD_SHARE: f32 = 0.55;",
        "pub const BOARD_SHARE: f32 = 0.15;",
        ["the_board_keeps_its_share_of_a_window_that_is_getting_shorter"],
    ),
    (
        "a band is dropped only once it is already too big",
        "            if wants.iter().sum::<f32>() <= budget {",
        "            if wants.iter().sum::<f32>() >= budget {",
        ["a_window_that_shrinks_gives_up_the_footer_before_the_header"],
    ),
    (
        "a dropped header is a strip of no height rather than nothing",
        "        let header = if head_h > 0.0 {",
        "        let header = if head_h >= 0.0 {",
        ["a_dropped_band_is_gone_rather_than_a_strip_of_no_height"],
    ),
    (
        "a dropped footer is a strip of no height rather than nothing",
        "        let footer = if foot_h > 0.0 {",
        "        let footer = if foot_h >= 0.0 {",
        ["a_dropped_band_is_gone_rather_than_a_strip_of_no_height"],
    ),
    (
        "a cell under a pixel is rounded up, so the board leaves the window",
        "        let (step, cell) = if natural < 1.0 {",
        "        let (step, cell) = if natural < 0.0 {",
        ["no_cell_is_ever_drawn_outside_the_window"],
    ),
    (
        "there is no gap between one cell and the next",
        "            (\n                natural,\n                (natural - (natural * 0.08).clamp(0.0, 3.0)).max(1.0),\n            )",
        "            (natural, natural)",
        ["there_is_a_gap_between_one_cell_and_the_next"],
    ),
    (
        "the board is not centred across the window",
        "            board.x + (board.w - grid_w).max(0.0) / 2.0,",
        "            board.x,",
        ["the_board_is_centred_in_the_space_it_is_given"],
    ),
    (
        "the board is drawn over the header",
        "        let board = Rect::new(\n            pad,\n            head_h + pad,",
        "        let board = Rect::new(\n            pad,\n            pad,",
        # Retargeted. Staying inside the window and staying inside the board's
        # own band are two different claims, and only the first was ever
        # asserted: a board slid up over the header is still wholly within the
        # window. The sweep caught this only through two chip tests, which
        # failed because the board had buried the chips' hit boxes -- a symptom
        # of the fault rather than the fault. The rule now has its own test.
        ["no_cell_is_drawn_on_top_of_the_header_or_the_footer"],
    ),
    (
        "a cell's column is used as its row",
        "            self.grid.x + col as f32 * self.step,\n            self.grid.y + row as f32 * self.step,",
        "            self.grid.x + row as f32 * self.step,\n            self.grid.y + row as f32 * self.step,",
        # Retargeted. `the_box_a_cell_records_is_the_box_it_was_painted_in`
        # asserts `probe::rect_of` against `cell_rect`, and `rect_of` returns
        # the box the drawing pass itself got from `cell_rect` -- both sides of
        # that comparison move together when `cell_rect` is mutated, so the
        # test is structurally unable to fail here (lesson 52). Agreement
        # between hit box and drawn box is still a real claim and still lives
        # there; *where the grid puts a cell* is a different claim and now has
        # its own test, which is the one named here.
        ["a_cells_box_moves_across_with_its_column_and_down_with_its_row"],
    ),
    (
        "a board with no room still records its cells",
        "        if self.cell <= 0.0 {\n            return Rect::EMPTY;\n        }",
        "        if self.cell < 0.0 {\n            return Rect::EMPTY;\n        }",
        ["a_window_with_no_room_for_a_board_draws_no_board_at_all"],
    ),
    (
        "the two chips are drawn on top of one another",
        "        let x = right - (w + gap) * (i as f32 + 1.0) + gap;",
        "        let x = right - (w + gap) * 1.0 + gap;",
        ["the_two_chips_sit_side_by_side_inside_the_header"],
    ),
    (
        "a chip is drawn where there is no header to hold it",
        "        if !self.shows(self.header) {\n            return Rect::EMPTY;\n        }",
        "        if false {\n            return Rect::EMPTY;\n        }",
        ["there_are_no_chips_when_there_is_no_header"],
    ),
    (
        "a chip may grow wider than the header",
        "        let w = (self.header.w * 0.16).clamp(40.0, 130.0);",
        "        let w = (self.header.w * 0.16).clamp(40.0, 1300.0);",
        ["a_chip_stops_growing_once_it_is_wide_enough_to_read"],
    ),
    # ── Drawing ───────────────────────────────────────────────────────────
    (
        "a covered cell and a flagged one look the same",
        "            CellState::Flagged => SURFACE2,",
        "            CellState::Flagged => SURFACE1,",
        ["a_covered_cell_a_flagged_one_and_an_open_one_are_three_different_faces"],
    ),
    (
        "the mine that ended the game is not marked",
        "            CellState::Revealed if lost_here => RED,",
        "            CellState::Revealed if lost_here => SURFACE0,",
        ["the_mine_that_ended_the_game_is_the_one_painted_red"],
    ),
    (
        "a cell records no box, so no click can reach it",
        "        f.hit(Target::Cell(row, col), r);",
        "        let _ = &r;",
        ["every_cell_of_every_board_records_a_box_a_click_can_find"],
    ),
    (
        "a flagged cell is drawn blank",
        '                centred_in(f, r, "F", size, PEACH, FontWeightHint::Bold);',
        '                centred_in(f, r, "", size, PEACH, FontWeightHint::Bold);',
        ["a_flagged_cell_carries_a_flag_and_an_uncovered_mine_carries_a_star"],
    ),
    (
        "an uncovered mine is drawn blank",
        '                centred_in(f, r, "*", size, ink, FontWeightHint::Bold);',
        '                centred_in(f, r, "", size, ink, FontWeightHint::Bold);',
        ["a_flagged_cell_carries_a_flag_and_an_uncovered_mine_carries_a_star"],
    ),
    (
        "a count is drawn in the colour of the count above it",
        "                    .get(usize::from(cell.adjacent).saturating_sub(1))",
        "                    .get(usize::from(cell.adjacent))",
        ["an_open_number_is_drawn_in_the_colour_that_count_is_given"],
    ),
    (
        "two counts share a colour",
        "    GREEN,      // 2",
        "    BLUE,       // 2",
        ["each_neighbour_count_is_written_in_its_own_colour"],
    ),
    (
        "a cell that touches nothing has a nought written on it",
        "            CellState::Revealed if cell.adjacent > 0 => {",
        "            CellState::Revealed if cell.adjacent < 9 => {",
        ["an_open_cell_that_touches_nothing_is_left_blank"],
    ),
    (
        "every cell but the cursor is outlined",
        "        if self.cursor == (row, col) {",
        "        if self.cursor != (row, col) {",
        ["the_cursor_is_outlined_and_only_the_cursor_is"],
    ),
    (
        "the header counter ignores the flags",
        '                &format!("Mines {}", self.mines_remaining()),',
        '                &format!("Mines {}", self.total_mines()),',
        ["the_counter_falls_as_flags_are_planted"],
    ),
    (
        "the header clock shows milliseconds",
        "                format_time(self.elapsed_secs()),",
        "                format_time(self.elapsed_ms()),",
        ["the_header_shows_the_counter_the_clock_the_state_and_the_level"],
    ),
    (
        "the new-game chip is drawn on top of the level chip",
        '        chip(f, l.chip(0), Target::NewGame, "New", l.font, LAVENDER);',
        '        chip(f, l.chip(1), Target::NewGame, "New", l.font, LAVENDER);',
        ["clicking_the_level_chip_moves_to_the_next_level"],
    ),
    (
        "a loss and a win read the same",
        '        GameStatus::Lost => "Boom",',
        '        GameStatus::Lost => "Cleared",',
        ["every_state_of_the_game_says_its_own_word_in_its_own_colour"],
    ),
    (
        "a win and a game in progress are the same colour",
        "        GameStatus::Won => GREEN,",
        "        GameStatus::Won => BLUE,",
        ["every_state_of_the_game_says_its_own_word_in_its_own_colour"],
    ),
    (
        "the footer runs its hints off the right edge",
        "            if x + w > l.footer.right() - l.pad {",
        "            if x + w > l.footer.right() + 10_000.0 {",
        [
            "the_footer_drops_the_hints_that_do_not_fit_rather_than_running_off_the_edge"
        ],
    ),
    (
        "the footer draws every hint on top of the first",
        "            x += w + l.pad * 2.0;",
        "            x += 0.0;",
        ["the_footer_lays_its_hints_out_in_a_row_rather_than_in_a_heap"],
    ),
    # ── The window's own surface ──────────────────────────────────────────
    (
        "the program's id no longer matches the one main launches",
        '        "minesweeper".to_string()',
        '        "mines".to_string()',
        ["the_program_names_itself_the_same_way_everywhere"],
    ),
    (
        "the window opens at the wrong size",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (WINDOW_HEIGHT as u32, WINDOW_WIDTH as u32)",
        ["the_program_names_itself_the_same_way_everywhere"],
    ),
    (
        "the close button does not close the window",
        "        if matches!(event, Event::CloseRequested) {",
        "        if matches!(event, Event::FocusOut) {",
        ["a_close_request_closes_the_window"],
    ),
    (
        "nothing ever asks for a repaint",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        ["an_event_that_changed_something_asks_for_a_repaint_and_one_that_did_not_does_not"],
    ),
    (
        "everything asks for a repaint",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["an_event_that_changed_something_asks_for_a_repaint_and_one_that_did_not_does_not"],
    ),
    (
        "drawing does not record the size the next click is read against",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["the_window_remembers_the_size_it_was_last_drawn_at"],
    ),
    (
        "a resize is read the wrong way round",
        "        self.size = (width.max(1.0), height.max(1.0));",
        "        self.size = (height.max(1.0), width.max(1.0));",
        ["the_window_remembers_the_size_it_was_last_drawn_at"],
    ),
    (
        "a resize is ignored",
        "            app.resize(*width as f32, *height as f32);\n            EventResult::Consumed",
        "            EventResult::Consumed",
        ["the_window_remembers_the_size_it_was_last_drawn_at"],
    ),
    (
        "the ticks never reach the clock",
        "        Event::Tick { elapsed_ms } => app.tick(*elapsed_ms),",
        "        Event::Tick { .. } => EventResult::Ignored,",
        ["the_clock_counts_the_time_that_passed_not_the_times_it_was_woken"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "minesweeper", timeout=240))
