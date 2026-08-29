"""Mutation test for the word search's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The word search is the tenth application in this campaign, and the one whose
old suite made the clearest case for the exercise.  `main` built a
`WordSearchApp`, bound it to `_app` and returned, so no board ever reached a
screen -- and the four faults that followed from that all had tests standing
next to them:

* `format_time` had four tests and no callers.  The header formatted the clock
  inline with its own `format!`, so the tested copy and the drawn copy were two
  implementations of one rule and only the untested one shipped.
* `HintHighlight::ticks` counted down from ten and nothing decremented it, so
  the renderer's `ticks > 0` could never be false.  No test noticed, because
  no test could advance time: there was no `Event::Tick` arm.
* There was no `Event::Mouse` arm either, in a game whose natural gesture is a
  drag.
* The clock read `00:00` for the length of every game.

Writing the suite through the public event path found a fifth, this time in
code that was not merely untested but wrong: `walk` decided between "count up"
and "count down" with `if end > start { .. } else { .. }`, which sends the
*still* axis of a horizontal or vertical line downwards.  The rows of the line
from `(2, 3)` to `(2, 7)` came out `2, 1, 0, 0, 0`, so no word lying along any
row but the first could be marked at all.  Three cases written as two.
(`known-issues.md` lesson 53.)

The first sweep put 105 mutations through the suite and caught 100.  The five
survivors were worth more than the hundred, because they sorted themselves into
two piles and neither pile was "the sweep was wrong":

* **Two were unreachable guards, and both are now deleted** rather than
  excused (`known-issues.md` lesson 51).  `generate_puzzle` skipped a word
  longer than the board, which `try_place_word` already declines identically --
  down to the random stream, since `rng.below(0)` returns without drawing.
  `hint_for` refused on `status == Won`, which the `placed.found` test two
  lines below already covers, because `Won` means every word is found.  A check
  standing behind a duplicate of itself is not defence in depth; it is one
  check plus a place a fault cannot be observed.
* **Three were genuinely blind tests**, and two of the three are the shapes
  lesson 52 names.  `assert_ne!(a.words(), b.words())` treats inequality as
  distinctness, so a colour list with fifteen animals pasted onto the front of
  it still passed; the fix asserts a fact from outside -- all five lists are
  thirty words long.  The greying test looked only at the *after* of finding a
  word, so `is_found_cell` losing its `w.found &&` -- which solves the whole
  puzzle on the first frame -- changed nothing it watched; it asserts the
  before now.  And the seeding test was built entirely out of `with_seed`, so
  `seed ^ 1` kept equal seeds equal and unequal seeds unequal and sailed
  through; it is pinned by one literal board now.

Usage:  python -u apps/wordsearch/mutate.py [substring ...]

(`-u`: this script's stdout is a pipe, and a buffered run looks like a hang
for several minutes.)
"""

import re
import subprocess
import sys
from pathlib import Path

SRC = Path(__file__).parent / "src" / "main.rs"
BAK = Path(__file__).parent / "src" / "main.rs.bak"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── Steps and directions ──────────────────────────────────────────────
    (
        "a step back walks forward",
        "            Self::Back => at.checked_sub(i),",
        "            Self::Back => at.checked_add(i),",
        ["a_step_walks_the_way_it_names_and_stops_at_the_edge"],
    ),
    (
        "the still axis moves after all",
        "            Self::Stay => Some(at),",
        "            Self::Stay => Some(at.saturating_add(i)),",
        ["a_step_walks_the_way_it_names_and_stops_at_the_edge"],
    ),
    (
        "a step off the low edge wraps to the top of the range",
        "            Self::Back => at.checked_sub(i),",
        "            Self::Back => Some(at.wrapping_sub(i)),",
        ["a_step_walks_the_way_it_names_and_stops_at_the_edge"],
    ),
    (
        "reversing a step leaves it alone",
        "            Self::Back => Self::Fwd,\n            Self::Stay => Self::Stay,\n            Self::Fwd => Self::Back,",
        "            Self::Back => Self::Back,\n            Self::Stay => Self::Stay,\n            Self::Fwd => Self::Fwd,",
        ["reversing_a_step_undoes_it"],
    ),
    (
        "two of the eight directions are the same direction",
        "    (Step::Back, Step::Back), // up-left",
        "    (Step::Stay, Step::Fwd), // up-left",
        ["the_eight_directions_are_eight_different_directions"],
    ),
    (
        "a direction is offered without its opposite",
        "    (Step::Back, Step::Fwd),  // up-right",
        "    (Step::Stay, Step::Stay), // up-right",
        ["the_eight_directions_are_eight_different_directions"],
    ),
    (
        "a word's cells are one short",
        "        (0..self.word.len())",
        "        (0..self.word.len().saturating_sub(1))",
        ["a_words_cells_run_from_its_start_along_its_direction"],
    ),
    (
        "a word's row and column steps are swapped",
        "            .map_while(|i| Some((dr.from(self.start.0, i)?, dc.from(self.start.1, i)?)))",
        "            .map_while(|i| Some((dc.from(self.start.0, i)?, dr.from(self.start.1, i)?)))",
        ["a_words_cells_run_from_its_start_along_its_direction"],
    ),
    # ── Word lists ────────────────────────────────────────────────────────
    (
        "a word list holds a word that is not capital ASCII",
        '"TIGER", "EAGLE",',
        '"Tiger", "EAGLE",',
        ["every_category_word_is_capital_ascii_so_a_byte_is_a_letter"],
    ),
    (
        "a word list holds a word too long for the smallest board",
        '"OTTER", "FALCON",',
        '"OTTER", "HIPPOPOTAMUSES",',
        ["every_category_word_is_capital_ascii_so_a_byte_is_a_letter"],
    ),
    (
        "cycling categories skips one",
        "            Self::Food => Self::Science,",
        "            Self::Food => Self::Geography,",
        ["cycling_categories_visits_every_one_and_returns"],
    ),
    (
        "the last category does not come round",
        "            Self::Geography => Self::Animals,",
        "            Self::Geography => Self::Geography,",
        ["cycling_categories_visits_every_one_and_returns"],
    ),
    (
        "two categories share an accent",
        "            Self::Science => TEAL,",
        "            Self::Science => GREEN,",
        ["every_category_has_its_own_name_and_its_own_accent"],
    ),
    (
        "two categories draw from the same word list",
        "            Self::Colors => &[\n                \"AZURE\",",
        "            Self::Colors => &[\n                \"TIGER\", \"EAGLE\", \"SHARK\", \"HORSE\", \"WHALE\", \"SNAKE\", \"PANDA\",\n                \"ZEBRA\", \"CAMEL\", \"OTTER\", \"FALCON\", \"PARROT\", \"RABBIT\", \"TURTLE\",\n                \"MONKEY\", \"AZURE\",",
        ["every_category_offers_the_same_number_of_words_and_none_of_them_twice"],
    ),
    # ── Difficulty ────────────────────────────────────────────────────────
    (
        "the easy board is the same size as the medium one",
        "            Self::Easy => 10,",
        "            Self::Easy => 15,",
        ["a_harder_board_is_bigger_and_hides_more"],
    ),
    (
        "the hard board hides fewer words than the easy one",
        "            Self::Hard => 12,",
        "            Self::Hard => 6,",
        ["a_harder_board_is_bigger_and_hides_more"],
    ),
    (
        "a board is asked to hide more words than it has rows",
        "            Self::Easy => 8,",
        "            Self::Easy => 11,",
        ["a_harder_board_is_bigger_and_hides_more"],
    ),
    (
        "the difficulty cycle does not come round",
        "            Self::Hard => Self::Easy,",
        "            Self::Hard => Self::Hard,",
        ["cycling_difficulty_visits_every_one_and_returns"],
    ),
    (
        "two difficulties are drawn in the same colour",
        "            Self::Medium => YELLOW,",
        "            Self::Medium => GREEN,",
        ["cycling_difficulty_visits_every_one_and_returns"],
    ),
    # ── Reading the board ─────────────────────────────────────────────────
    (
        "the column bound is gone, so a column past the end is the next row",
        "        if col >= self.grid_size {\n            return None;\n        }",
        "        #[allow(clippy::needless_bool)]\n        if false {\n            return None;\n        }",
        ["a_column_past_the_end_is_not_the_next_rows_first"],
    ),
    (
        "the grid is indexed column-major",
        "        let index = row.checked_mul(self.grid_size)?.checked_add(col)?;",
        "        let index = col.checked_mul(self.grid_size)?.checked_add(row)?;",
        ["every_hidden_word_can_actually_be_read_off_the_board"],
    ),
    (
        "every cell of every word counts as found",
        "            .any(|w| w.found && w.cells().contains(&(row, col)))",
        "            .any(|w| w.cells().contains(&(row, col)))",
        ["a_found_word_is_greyed_and_its_letters_go_green"],
    ),
    (
        "the clock reads centiseconds as seconds",
        "        self.elapsed_ms / 1_000",
        "        self.elapsed_ms / 100",
        ["the_clock_counts_the_time_it_is_given_not_the_ticks_it_gets"],
    ),
    (
        "an unmarked board previews the cell under the cursor",
        "            Selection::None => Vec::new(),",
        "            Selection::None => vec![self.cursor],",
        ["the_marked_cells_are_the_line_between_the_anchor_and_the_cursor"],
    ),
    (
        "a crooked mark previews the whole board's diagonal instead of nothing",
        "            Selection::From(r, c) => cells_between((r, c), self.cursor).unwrap_or_default(),",
        "            Selection::From(r, c) => {\n                cells_between((r, c), self.cursor).unwrap_or_else(|| vec![(r, c)])\n            }",
        ["a_mark_whose_ends_are_not_on_a_line_finds_nothing"],
    ),
    # ── Generation ────────────────────────────────────────────────────────
    (
        "a new game keeps the clock it had",
        "        self.elapsed_ms = 0;\n        self.hints_remaining = MAX_HINTS;",
        "        self.hints_remaining = MAX_HINTS;",
        ["a_new_game_puts_the_clock_and_the_hints_back"],
    ),
    (
        "a new game does not give the hints back",
        "        self.elapsed_ms = 0;\n        self.hints_remaining = MAX_HINTS;",
        "        self.elapsed_ms = 0;",
        ["a_new_game_puts_the_clock_and_the_hints_back"],
    ),
    (
        "a new game leaves the cursor where it was",
        "        self.cursor = (0, 0);\n        self.selection = Selection::None;",
        "        self.selection = Selection::None;",
        ["a_new_game_puts_the_clock_and_the_hints_back"],
    ),
    (
        "a new game leaves the old mark open",
        "        self.cursor = (0, 0);\n        self.selection = Selection::None;\n        self.dragging = false;",
        "        self.cursor = (0, 0);\n        self.dragging = false;",
        ["a_new_game_puts_the_clock_and_the_hints_back"],
    ),
    (
        "a new game reuses the seed, so every game is the same game",
        "        self.seed = self.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);",
        "        self.seed = self.seed.wrapping_add(0);",
        ["a_new_game_at_the_same_settings_is_a_different_board"],
    ),
    (
        "the board hides one word more than it says",
        "            if self.placed_words.len() >= wanted {",
        "            if self.placed_words.len() > wanted {",
        ["a_harder_puzzle_really_is_a_bigger_board_with_more_words"],
    ),
    (
        "the cells no word reached are left blank",
        "        for cell in &mut self.grid {\n            if *cell == 0 {",
        "        for cell in &mut self.grid {\n            if *cell == u8::MAX {",
        ["a_new_board_is_full_of_letters_with_no_holes"],
    ),
    (
        "the filler is lowercase",
        "    b'A'.saturating_add(offset)",
        "    b'a'.saturating_add(offset)",
        ["a_new_board_is_full_of_letters_with_no_holes"],
    ),
    (
        "every word is laid in whichever direction was tried first",
        "        let Some(&(start, dir)) = spots.get(self.rng.below(spots.len())) else {",
        "        let Some(&(start, dir)) = spots.first() else {",
        ["words_are_hidden_in_more_than_one_direction"],
    ),
    (
        "a word may be laid over a different letter",
        "                Some(existing) => existing == 0 || existing == ch,",
        "                Some(existing) => existing == 0 || existing != ch,",
        ["every_hidden_word_can_actually_be_read_off_the_board"],
    ),
    (
        "a word may run off the edge of the board",
        "                Some(existing) => existing == 0 || existing == ch,\n                None => false,",
        "                Some(existing) => existing == 0 || existing == ch,\n                None => true,",
        ["every_hidden_word_can_actually_be_read_off_the_board"],
    ),
    # "a word longer than the board is placed anyway" used to be here, widening
    # `generate_puzzle`'s `if word.len() > size { continue }`.  It survived, and
    # the guard is gone rather than the mutation being excused: `try_place_word`
    # answers identically for a word that fits nowhere, down to the random
    # stream, so the guard could not change a board even in principle.  The
    # claim it was standing in for is checked as a fact about the word lists in
    # `every_category_word_is_capital_ascii_so_a_byte_is_a_letter`, and a
    # mutation that plants an over-long word does fail that test.
    (
        "the same word is hidden twice",
        "        let mut order: Vec<usize> = (0..all.len()).collect();\n        self.rng.shuffle(&mut order);",
        "        let mut order: Vec<usize> = (0..all.len()).map(|i| i / 2).collect();\n        self.rng.shuffle(&mut order);",
        ["no_two_hidden_words_are_the_same_word"],
    ),
    (
        "a seed does not name a board",
        "        let mut app = Self {",
        "        let seed = seed ^ 1;\n        let mut app = Self {",
        ["one_named_seed_names_one_named_board"],
    ),
    # ── Lines between cells ───────────────────────────────────────────────
    (
        "the still axis of a straight line walks backwards (the shipped bug)",
        "        Ordering::Equal => start,",
        "        Ordering::Equal => start.saturating_sub(i),",
        ["a_line_and_its_reverse_are_the_same_cells_in_the_other_order"],
    ),
    (
        "a line walks away from its far end",
        "        Ordering::Greater => start.saturating_add(i),\n        Ordering::Less => start.saturating_sub(i),",
        "        Ordering::Greater => start.saturating_sub(i),\n        Ordering::Less => start.saturating_add(i),",
        ["a_line_between_two_cells_is_only_a_line_at_the_eight_angles"],
    ),
    (
        "a cell on its own is not a line",
        "        return Some(vec![from]);",
        "        return None;",
        ["a_cell_is_a_line_of_one"],
    ),
    (
        "diagonals are not lines",
        "    } else if dc == 0 || dr == dc {",
        "    } else if dc == 0 {",
        ["a_line_between_two_cells_is_only_a_line_at_the_eight_angles"],
    ),
    (
        "any two cells are a line",
        "    } else if dc == 0 || dr == dc {\n        dr\n    } else {\n        return None;\n    };",
        "    } else {\n        dr.max(dc)\n    };",
        ["a_line_between_two_cells_is_only_a_line_at_the_eight_angles"],
    ),
    (
        "a horizontal line is as long as its row difference",
        "    let steps = if dr == 0 {\n        dc\n    } else if",
        "    let steps = if dr == 0 {\n        dr\n    } else if",
        ["a_line_between_two_cells_is_only_a_line_at_the_eight_angles"],
    ),
    (
        "a line stops one cell short of its far end",
        "    for i in 0..=steps {",
        "    for i in 0..steps {",
        ["a_line_and_its_reverse_are_the_same_cells_in_the_other_order"],
    ),
    # ── The clock's format ────────────────────────────────────────────────
    (
        "the seconds lose their leading zero",
        '    format!("{:02}:{:02}", secs / 60, secs % 60)',
        '    format!("{:02}:{}", secs / 60, secs % 60)',
        ["the_clock_reads_minutes_and_seconds_with_both_digits"],
    ),
    (
        "the minutes and the seconds are the other way round",
        '    format!("{:02}:{:02}", secs / 60, secs % 60)',
        '    format!("{:02}:{:02}", secs % 60, secs / 60)',
        ["the_clock_reads_minutes_and_seconds_with_both_digits"],
    ),
    (
        "the minutes wrap at the hour",
        '    format!("{:02}:{:02}", secs / 60, secs % 60)',
        '    format!("{:02}:{:02}", secs / 60 % 60, secs % 60)',
        ["the_clock_reads_minutes_and_seconds_with_both_digits"],
    ),
]


MUTATIONS += [
    # ── The clock and the hint burn ───────────────────────────────────────
    (
        "the clock runs on after the game is won",
        "        if self.status == GameStatus::Playing {\n            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        "        if self.status != GameStatus::Playing {\n            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        ["finding_every_word_wins_and_stops_the_clock"],
    ),
    (
        "the clock counts wake-ups instead of time",
        "            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        "            self.elapsed_ms = self.elapsed_ms.saturating_add(1_000);",
        ["the_clock_counts_the_time_it_is_given_not_the_ticks_it_gets"],
    ),
    (
        "the hint burns per wake-up instead of per millisecond",
        "            hint.remaining_ms = hint.remaining_ms.saturating_sub(elapsed_ms);",
        "            hint.remaining_ms = hint.remaining_ms.saturating_sub(1);",
        ["a_hint_burns_down_at_wall_clock_speed_not_per_wake_up"],
    ),
    (
        "the hint never goes out",
        "            if hint.remaining_ms == 0 {\n                self.hint = None;\n            }",
        "            if hint.remaining_ms == u64::MAX {\n                self.hint = None;\n            }",
        ["a_hint_fades_on_its_own_and_the_board_goes_back_to_normal"],
    ),
    (
        "a burning hint is not a reason to repaint",
        "            if hint.remaining_ms == 0 {\n                self.hint = None;\n            }\n            moved = EventResult::Consumed;",
        "            if hint.remaining_ms == 0 {\n                self.hint = None;\n            }",
        ["a_hint_still_burns_on_a_board_whose_clock_has_stopped"],
    ),
    (
        "a running clock is not a reason to repaint",
        "            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);\n            moved = EventResult::Consumed;",
        "            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);",
        ["the_clock_counts_the_time_it_is_given_not_the_ticks_it_gets"],
    ),
    (
        "a burning hint does not count as animation",
        "        self.hint.is_some() || self.status == GameStatus::Playing",
        "        self.status == GameStatus::Playing",
        ["a_hint_still_burns_on_a_board_whose_clock_has_stopped"],
    ),
    # ── Moving and marking ────────────────────────────────────────────────
    (
        "the cursor may stand one cell off the board",
        "                if row >= self.grid_size || col >= self.grid_size {\n                    return EventResult::Ignored;\n                }\n                if self.cursor == (row, col) {",
        "                if row > self.grid_size || col > self.grid_size {\n                    return EventResult::Ignored;\n                }\n                if self.cursor == (row, col) {",
        ["the_cursor_cannot_be_sent_off_the_board_by_name_either"],
    ),
    (
        "the cursor may run off the right edge but not the bottom",
        "                if row >= self.grid_size || col >= self.grid_size {\n                    return EventResult::Ignored;\n                }\n                if self.cursor == (row, col) {",
        "                if row >= self.grid_size {\n                    return EventResult::Ignored;\n                }\n                if self.cursor == (row, col) {",
        ["the_cursor_cannot_be_sent_off_the_board_by_name_either"],
    ),
    (
        "a move to where the cursor already is repaints the window",
        "                if self.cursor == (row, col) {\n                    return EventResult::Ignored;\n                }",
        "                if self.cursor == (row, col) {\n                    return EventResult::Consumed;\n                }",
        ["a_move_that_changes_nothing_is_not_a_redraw"],
    ),
    (
        "the arrow keys move the wrong axis",
        "            Key::Up => Some(Action::Move(Step::Back, Step::Stay)),\n            Key::Down => Some(Action::Move(Step::Fwd, Step::Stay)),",
        "            Key::Up => Some(Action::Move(Step::Stay, Step::Back)),\n            Key::Down => Some(Action::Move(Step::Stay, Step::Fwd)),",
        ["the_arrow_keys_walk_the_cursor_one_cell_at_a_time"],
    ),
    (
        "left and right are swapped",
        "            Key::Left => Some(Action::Move(Step::Stay, Step::Back)),\n            Key::Right => Some(Action::Move(Step::Stay, Step::Fwd)),",
        "            Key::Left => Some(Action::Move(Step::Stay, Step::Fwd)),\n            Key::Right => Some(Action::Move(Step::Stay, Step::Back)),",
        ["the_arrow_keys_walk_the_cursor_one_cell_at_a_time"],
    ),
    (
        "a won board can still be marked on",
        "            Action::Anchor => {\n                if self.status == GameStatus::Won {\n                    return EventResult::Ignored;\n                }",
        "            Action::Anchor => {\n                if self.status == GameStatus::Playing {\n                    return EventResult::Ignored;\n                }",
        ["finding_every_word_wins_and_stops_the_clock"],
    ),
    (
        "a press on a won board opens a drag",
        "            Action::Begin(row, col) => {\n                if self.status == GameStatus::Won {\n                    return EventResult::Ignored;\n                }",
        "            Action::Begin(row, col) => {\n                if self.status == GameStatus::Playing {\n                    return EventResult::Ignored;\n                }",
        ["finding_every_word_wins_and_stops_the_clock"],
    ),
    (
        "a press does not open a drag",
        "                self.selection = Selection::From(row, col);\n                self.dragging = true;",
        "                self.selection = Selection::From(row, col);\n                self.dragging = false;",
        ["a_press_anchors_where_it_lands_and_moves_the_cursor_there"],
    ),
    (
        "a press anchors somewhere other than where it landed",
        "                self.cursor = (row, col);\n                self.selection = Selection::From(row, col);",
        "                self.cursor = (row, col);\n                self.selection = Selection::From(0, 0);",
        ["a_press_anchors_where_it_lands_and_moves_the_cursor_there"],
    ),
    (
        "a hover drags the cursor around",
        "                if !self.dragging {\n                    return EventResult::Ignored;\n                }\n                self.apply(Action::Goto(row, col))",
        "                self.apply(Action::Goto(row, col))",
        ["the_pointer_only_drags_the_far_end_while_a_button_is_down"],
    ),
    (
        "a stray release finishes a mark the keyboard opened",
        "            Action::Finish => {\n                if !self.dragging {\n                    return EventResult::Ignored;\n                }",
        "            Action::Finish => {\n                if self.selection == Selection::None {\n                    return EventResult::Ignored;\n                }",
        ["a_release_with_no_drag_open_does_nothing"],
    ),
    (
        "a release leaves the drag open",
        "                self.dragging = false;\n                self.confirm()",
        "                self.confirm()",
        ["dragging_across_a_word_finds_it"],
    ),
    (
        "escape leaves the drag open",
        "                self.selection = Selection::None;\n                self.dragging = false;\n                EventResult::Consumed",
        "                self.selection = Selection::None;\n                EventResult::Consumed",
        ["escape_abandons_a_drag_as_well_as_an_anchor"],
    ),
    (
        "escape with nothing to cancel repaints anyway",
        "                if self.selection == Selection::None && !self.dragging {",
        "                if self.selection == Selection::None && self.dragging {",
        ["escape_throws_a_mark_away_and_finds_nothing"],
    ),
    (
        "confirming does not clear the mark",
        "        self.selection = Selection::None;\n        EventResult::Consumed\n    }",
        "        EventResult::Consumed\n    }",
        ["enter_anchors_and_enter_again_confirms"],
    ),
    (
        "a mark is read only forwards",
        "            own == cells || own == backwards",
        "            own == cells",
        ["a_word_marked_backwards_is_the_same_word"],
    ),
    (
        "a mark is read only backwards",
        "            own == cells || own == backwards",
        "            own == backwards",
        ["a_word_marked_from_its_first_letter_to_its_last_is_found"],
    ),
    (
        "every mark finds a word",
        "            own == cells || own == backwards",
        "            !own.is_empty()",
        ["marking_a_line_that_spells_nothing_finds_nothing"],
    ),
    (
        "the reverse of a mark is the mark itself",
        "        let backwards: Vec<(usize, usize)> = cells.iter().rev().copied().collect();",
        "        let backwards: Vec<(usize, usize)> = cells.clone();",
        ["a_word_marked_backwards_is_the_same_word"],
    ),
    (
        "finding a word does not mark it found",
        "        placed.found = true;",
        "        placed.found |= false;",
        ["a_word_marked_from_its_first_letter_to_its_last_is_found"],
    ),
    (
        "the game is won as soon as any word is found",
        "        if self.placed_words.iter().all(|w| w.found) {",
        "        if self.placed_words.iter().any(|w| w.found) {",
        ["finding_every_word_wins_and_stops_the_clock"],
    ),
    (
        "the game is never won",
        "        if self.placed_words.iter().all(|w| w.found) {\n            self.status = GameStatus::Won;\n        }",
        "        if self.placed_words.is_empty() {\n            self.status = GameStatus::Won;\n        }",
        ["finding_every_word_wins_and_stops_the_clock"],
    ),
    # ── Hints ─────────────────────────────────────────────────────────────
    (
        "a sixth hint is given",
        "        if self.hints_remaining == 0 {\n            return EventResult::Ignored;\n        }\n        let Some(placed) = self.placed_words.get(index) else {",
        "        if self.hints_remaining == usize::MAX {\n            return EventResult::Ignored;\n        }\n        let Some(placed) = self.placed_words.get(index) else {",
        ["hints_run_out_and_the_chip_says_so"],
    ),
    # "a won board still gives hints" used to be here, deleting the
    # `self.status == GameStatus::Won ||` from `hint_for`'s refusal.  It
    # survived, and the disjunct is gone rather than the mutation being
    # excused: `Won` is set in one place, when every word is found, so a won
    # board has no unfound word for the `placed.found` test to admit.  The
    # behaviour is still asserted by `a_won_board_gives_no_more_hints`; it is
    # now guaranteed by the one check that does the work.
    (
        "a hint is spent on a word already found",
        "        if placed.found {\n            return EventResult::Ignored;\n        }",
        "        if !placed.found {\n            return EventResult::Ignored;\n        }",
        ["a_hint_is_never_spent_on_a_word_already_found"],
    ),
    (
        "a hint points at a word's last letter",
        "        let Some(&(row, col)) = placed.cells().first() else {",
        "        let Some(&(row, col)) = placed.cells().last() else {",
        ["a_hint_lights_the_first_letter_of_a_word_and_costs_one"],
    ),
    (
        "a hint costs nothing",
        "        self.hints_remaining = self.hints_remaining.saturating_sub(1);",
        "        self.hints_remaining = self.hints_remaining.saturating_sub(0);",
        ["a_hint_lights_the_first_letter_of_a_word_and_costs_one"],
    ),
    (
        "a hint burns for a different length of time than it says",
        "            remaining_ms: HINT_MS,",
        "            remaining_ms: HINT_MS.saturating_mul(2),",
        ["a_hint_lights_the_first_letter_of_a_word_and_costs_one"],
    ),
    (
        "H points at a word already found",
        "                let Some(index) = self.placed_words.iter().position(|w| !w.found) else {",
        "                let Some(index) = self.placed_words.iter().position(|w| w.found) else {",
        ["a_hint_lights_the_first_letter_of_a_word_and_costs_one"],
    ),
    (
        "a hint for a word this puzzle does not have is given anyway",
        "        let Some(placed) = self.placed_words.get(index) else {\n            return EventResult::Ignored;\n        };",
        "        let Some(placed) = self.placed_words.first() else {\n            return EventResult::Ignored;\n        };",
        ["a_hint_for_a_word_this_puzzle_does_not_have_is_refused"],
    ),
    (
        "the word list is one button rather than one per word",
        "                Some(Target::Word(index)) => self.apply(Action::HintFor(index)),",
        "                Some(Target::Word(_)) => self.apply(Action::UseHint),",
        ["a_hint_can_be_asked_for_by_name_rather_than_always_the_same_word"],
    ),
    # ── Settings ──────────────────────────────────────────────────────────
    (
        "D changes the category instead of the level",
        "                self.new_game(self.difficulty.next(), self.category);",
        "                self.new_game(self.difficulty, self.category.next());",
        ["d_and_c_start_a_new_game_one_step_on"],
    ),
    (
        "C changes the level instead of the category",
        "                self.new_game(self.difficulty, self.category.next());",
        "                self.new_game(self.difficulty.next(), self.category);",
        ["d_and_c_start_a_new_game_one_step_on"],
    ),
    (
        "F2 changes the settings as well as the board",
        "            Action::NewGame => {\n                self.new_game(self.difficulty, self.category);",
        "            Action::NewGame => {\n                self.new_game(self.difficulty.next(), self.category);",
        ["a_new_game_at_the_same_settings_is_a_different_board"],
    ),
    (
        "a named difficulty is ignored",
        "            Action::SetDifficulty(difficulty) => {\n                self.new_game(difficulty, self.category);",
        "            Action::SetDifficulty(_) => {\n                self.new_game(self.difficulty, self.category);",
        ["ctrl_and_a_digit_pick_a_difficulty_outright"],
    ),
    (
        "Ctrl-1 and Ctrl-2 are swapped",
        "                Key::Num1 => Some(Action::SetDifficulty(Difficulty::Easy)),\n                Key::Num2 => Some(Action::SetDifficulty(Difficulty::Medium)),",
        "                Key::Num1 => Some(Action::SetDifficulty(Difficulty::Medium)),\n                Key::Num2 => Some(Action::SetDifficulty(Difficulty::Easy)),",
        ["ctrl_and_a_digit_pick_a_difficulty_outright"],
    ),
    (
        "a plain digit picks a difficulty too",
        "        if key.modifiers != Modifiers::NONE {\n            return None;\n        }\n        match key.key {",
        "        if key.modifiers != Modifiers::NONE {\n            return None;\n        }\n        if key.key == Key::Num1 {\n            return Some(Action::SetDifficulty(Difficulty::Easy));\n        }\n        match key.key {",
        ["ctrl_and_a_digit_pick_a_difficulty_outright"],
    ),
    (
        "a key going up does what a key going down does",
        "        if !key.pressed {\n            return None;\n        }",
        "        if key.pressed && !key.pressed {\n            return None;\n        }",
        ["a_key_this_program_does_not_answer_is_left_alone"],
    ),
    (
        "a modifier held with a plain key is ignored",
        "        if key.modifiers != Modifiers::NONE {\n            return None;\n        }",
        "        if key.modifiers.super_key {\n            return None;\n        }",
        ["a_key_this_program_does_not_answer_is_left_alone"],
    ),
    (
        "H is not a hint",
        "            Key::H => Some(Action::UseHint),",
        "            Key::H => None,",
        ["a_hint_lights_the_first_letter_of_a_word_and_costs_one"],
    ),
    (
        "F2 does nothing",
        "            Key::F2 => Some(Action::NewGame),",
        "            Key::F2 => None,",
        ["a_new_game_puts_the_clock_and_the_hints_back"],
    ),
    # ── The pointer ───────────────────────────────────────────────────────
    (
        "a click is read against the window the program opened at, not the one it is in",
        "            .frame(self.size.0, self.size.1)\n            .hit_test(event.x, event.y);",
        "            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)\n            .hit_test(event.x, event.y);",
        ["a_click_is_read_against_the_window_the_player_is_looking_at"],
    ),
    (
        "a drag begins a new mark at every cell it crosses",
        "                Some(Target::Cell(row, col)) => self.apply(Action::Extend(row, col)),",
        "                Some(Target::Cell(row, col)) => self.apply(Action::Begin(row, col)),",
        ["dragging_across_a_word_finds_it"],
    ),
    (
        "a release throws the mark away instead of reading it",
        "            MouseEventKind::Release(MouseButton::Left) => self.apply(Action::Finish),",
        "            MouseEventKind::Release(MouseButton::Left) => self.apply(Action::Cancel),",
        ["dragging_across_a_word_finds_it"],
    ),
    (
        "the right button marks too",
        "            MouseEventKind::Press(MouseButton::Left) => match hit {",
        "            MouseEventKind::Press(_) => match hit {",
        ["only_the_left_button_marks"],
    ),
    (
        "the chips do nothing",
        "                Some(Target::Difficulty) => self.apply(Action::CycleDifficulty),\n                Some(Target::Category) => self.apply(Action::CycleCategory),",
        "                Some(Target::Difficulty | Target::Category) => EventResult::Ignored,",
        ["the_chips_do_what_the_keys_do"],
    ),
    (
        "the New chip gives a hint instead",
        "                Some(Target::NewGame) => self.apply(Action::NewGame),",
        "                Some(Target::NewGame) => self.apply(Action::UseHint),",
        ["the_chips_do_what_the_keys_do"],
    ),
    (
        "a window may be zero pixels wide as far as the model is concerned",
        "        self.size = (width.max(1.0), height.max(1.0));",
        "        self.size = (width, height);",
        ["a_window_is_never_smaller_than_one_pixel_to_the_model"],
    ),
    (
        "a resize is not recorded",
        "            app.resize(*width as f32, *height as f32);\n            EventResult::Consumed",
        "            let _ = (width, height);\n            EventResult::Consumed",
        ["a_click_is_read_against_the_window_the_player_is_looking_at"],
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
            "wordsearch",
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
    # An unbounded catch-up loop does not fail a test; it runs meter sweeps
    # until the runner kills it.  A harness that only counted named failures
    # would score that as a mutant nobody noticed, which is the opposite of the
    # truth: a hang IS the symptom.  Same for a mutant that aborts the process
    # before any test can report.
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
