"""Mutation test for game2048's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

2048 is the seventeenth application in this campaign, and it arrived in the
same shape as the sixteen before it: `main` built a `Game2048`, dealt two
tiles and dropped it.  Nothing in the file ever built an `Event` or a `Frame`,
so there was no path from a keystroke to a slide to test, and no layout either
-- every rectangle in the window came out of constants that no window had ever
been measured against.

Usage:  python -u apps/game2048/mutate.py [substring ...]
"""

import re
import subprocess
import sys
from pathlib import Path

SRC = Path(__file__).parent / "src" / "main.rs"
BAK = Path(__file__).parent / "src" / "main.rs.bak"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The slide, which is the whole game ────────────────────────────────
    (
        "packing keeps the gaps between the tiles",
        "        for &val in line {\n            if val != 0 {",
        "        for &val in line {\n            if val < u32::MAX {",
        ["a_line_slides_its_tiles_against_the_wall_with_no_gaps_left_between_them"],
    ),
    (
        "a merge makes a tile of the same value",
        "                let merged = val.saturating_mul(2);",
        "                let merged = val;",
        ["two_tiles_alike_merge_into_one_of_twice_the_value"],
    ),
    (
        "a merge scores what it was made from",
        "                points = points.saturating_add(merged);",
        "                points = points.saturating_add(val);",
        ["a_merge_scores_what_it_made_and_not_what_it_was_made_from"],
    ),
    (
        "a merged tile can merge again",
        "                (merged, 2)",
        "                (merged, 1)",
        ["a_tile_merges_at_most_once_in_a_move"],
    ),
    (
        "any two tiles merge, alike or not",
        "            let (value, step) = if pair == val {",
        "            let (value, step) = if pair != 0 {",
        ["unlike_tiles_never_merge"],
    ),
    (
        # Not "the trailing pair merges": with three tiles alike, reaching past
        # a neighbour finds the same value at either distance and the leftovers
        # pack down the same way, so `[2,2,2]` cannot see this (lesson 59).
        "a merge reaches past its neighbour to the tile after it",
        "            let pair = packed.get(read.saturating_add(1)).copied().unwrap_or(0);",
        "            let pair = packed.get(read.saturating_add(2)).copied().unwrap_or(0);",
        ["a_merge_takes_the_tile_next_to_it_and_not_the_one_past_that"],
    ),
    (
        "a line that changed says it did not",
        "        (out, points, out != *line)",
        "        (out, points, false)",
        ["a_line_slides_its_tiles_against_the_wall_with_no_gaps_left_between_them"],
    ),
    (
        "a line that did nothing says it moved",
        "        (out, points, out != *line)",
        "        (out, points, true)",
        ["a_line_with_nothing_to_do_says_it_did_nothing"],
    ),
    # ── A move over the whole board ───────────────────────────────────────
    (
        "right and down are not read backwards",
        "        let reversed = matches!(dir, Direction::Right | Direction::Down);",
        "        let reversed = false;",
        ["sliding_right_is_sliding_left_read_backwards"],
    ),
    (
        "up and down slide rows rather than columns",
        "        let vertical = matches!(dir, Direction::Up | Direction::Down);",
        "        let vertical = false;",
        ["sliding_up_moves_columns_and_not_rows"],
    ),
    (
        "up is reversed and down is not",
        "        let reversed = matches!(dir, Direction::Right | Direction::Down);",
        "        let reversed = matches!(dir, Direction::Right | Direction::Up);",
        ["sliding_down_stacks_against_the_bottom"],
    ),
    (
        "the slid line is not turned back the right way round",
        "            if reversed {\n                out.reverse();\n            }",
        "",
        ["sliding_right_is_sliding_left_read_backwards"],
    ),
    (
        "only the last line counts towards whether the board moved",
        "            moved |= line_moved;",
        "            moved = line_moved;",
        ["a_move_slides_every_row_at_once"],
    ),
    (
        "a move that changed nothing is scored and counted anyway",
        "        if moved {\n            self.score = self.score.saturating_add(points);",
        "        if true {\n            self.score = self.score.saturating_add(points);",
        ["a_move_that_changes_nothing_costs_neither_a_turn_nor_a_point"],
    ),
    (
        "the best score follows the score back down",
        "            self.best_score = self.best_score.max(self.score);",
        "            self.best_score = self.score;",
        ["the_best_score_is_the_high_water_mark_of_the_score"],
    ),
    (
        "a move is not counted",
        "            self.moves = self.moves.saturating_add(1);",
        "",
        ["an_undo_puts_back_the_board_the_score_and_the_move_count"],
    ),
    # ── Reading the board ─────────────────────────────────────────────────
    (
        "a square off the board reads as something rather than nothing",
        "            .copied()\n            .unwrap_or(0)",
        "            .copied()\n            .unwrap_or(2)",
        ["a_square_off_the_board_reads_as_empty_rather_than_wrapping_round"],
    ),
    (
        "the highest tile is the lowest one",
        "        self.grid.iter().flatten().copied().max().unwrap_or(0)",
        "        self.grid.iter().flatten().copied().min().unwrap_or(0)",
        ["the_highest_tile_is_read_off_the_board_rather_than_remembered"],
    ),
    (
        "a free cell is not a move",
        "                let val = self.at(r, c);\n                if val == 0 {",
        "                let val = self.at(r, c);\n                if val == u32::MAX {",
        ["a_board_with_a_free_cell_can_always_move"],
    ),
    (
        "two alike side by side are not a move",
        "                if c.saturating_add(1) < GRID_SIZE && self.at(r, c.saturating_add(1)) == val {",
        "                if false {",
        ["a_full_board_with_two_alike_side_by_side_can_still_move"],
    ),
    (
        "two alike one above the other are not a move",
        "                if r.saturating_add(1) < GRID_SIZE && self.at(r.saturating_add(1), c) == val {",
        "                if false {",
        ["a_full_board_with_two_alike_one_above_the_other_can_still_move"],
    ),
    (
        "a board with nothing to do can still move",
        "        false\n    }\n\n    pub fn has_won(&self) -> bool {",
        "        true\n    }\n\n    pub fn has_won(&self) -> bool {",
        ["a_full_board_with_no_two_neighbours_alike_cannot_move"],
    ),
    (
        "the winning tile is a thousand and twenty four",
        "const WIN_TILE: u32 = 2048;",
        "const WIN_TILE: u32 = 1024;",
        ["the_winning_tile_is_2048_and_not_merely_a_large_one"],
    ),
    # ── Dealing a tile ────────────────────────────────────────────────────
    (
        "a tile is always dealt into the first free cell",
        "        let Some(&(r, c)) = empty.get(rng.below(empty.len())) else {",
        "        let Some(&(r, c)) = empty.first() else {",
        ["the_cell_a_tile_is_dealt_into_is_not_always_the_same_one"],
    ),
    (
        "a tile can be dealt onto an occupied cell",
        "                if val == 0 {\n                    cells.push((r, c));",
        "                if val < u32::MAX {\n                    cells.push((r, c));",
        ["a_tile_is_only_ever_dealt_into_a_free_cell"],
    ),
    (
        "every dealt tile is a four",
        "        let val = if rng.below(FOUR_IN as usize) == 0 {",
        "        let val = if true {",
        ["a_four_turns_up_about_one_time_in_ten"],
    ),
    (
        "every dealt tile is a two",
        "        let val = if rng.below(FOUR_IN as usize) == 0 {",
        "        let val = if false {",
        ["a_four_turns_up_about_one_time_in_ten"],
    ),
    (
        "a four turns up half the time",
        "        let val = if rng.below(FOUR_IN as usize) == 0 {",
        "        let val = if rng.below(2usize) == 0 {",
        ["a_four_turns_up_about_one_time_in_ten"],
    ),
    (
        "a dealt tile is an eight",
        "        let val = if rng.below(FOUR_IN as usize) == 0 {\n            4\n        } else {\n            2\n        };",
        "        let val = if rng.below(FOUR_IN as usize) == 0 {\n            8\n        } else {\n            2\n        };",
        ["a_spawned_tile_is_a_two_or_a_four_and_never_anything_else"],
    ),
    (
        "a game opens with one tile",
        "        self.board.spawn_tile(&mut self.rng);\n        self.board.spawn_tile(&mut self.rng);",
        "        self.board.spawn_tile(&mut self.rng);",
        ["a_new_game_deals_exactly_two_tiles"],
    ),
    # ── A move, from the game's side ──────────────────────────────────────
    (
        "a frozen game can still be played",
        "        if !self.can_play() {\n            return false;\n        }",
        "",
        # Not `a_lost_game_refuses_every_direction`: a lost board has no move
        # left in it, so it refuses whether this guard is there or not, and only
        # a won game -- frozen with moves still available -- can tell the guard
        # from the board's own emptiness (lesson 58).
        ["a_won_game_refuses_to_move_until_the_player_says_to_keep_going"],
    ),
    (
        "a won game is frozen and one being played is not",
        "            GameStatus::Playing | GameStatus::WonContinuing\n        )",
        "            GameStatus::Won | GameStatus::Lost\n        )",
        ["a_direction_that_cannot_be_played_is_reported_as_unhandled"],
    ),
    (
        "the snapshot is taken after the move rather than before it",
        "        let before = UndoEntry::of(&self.board);\n        if !self.board.apply_move(dir) {\n            return false;\n        }\n        self.push_undo(before);",
        "        if !self.board.apply_move(dir) {\n            return false;\n        }\n        self.push_undo(UndoEntry::of(&self.board));",
        ["an_undo_puts_back_the_board_the_score_and_the_move_count"],
    ),
    (
        "a move that changed nothing goes into the history",
        "        if !self.board.apply_move(dir) {\n            return false;\n        }",
        "        self.board.apply_move(dir);",
        ["a_move_that_changed_nothing_leaves_nothing_to_undo"],
    ),
    (
        "no tile is dealt after a move",
        "        self.push_undo(before);\n        self.board.spawn_tile(&mut self.rng);",
        "        self.push_undo(before);",
        ["a_move_that_changed_the_board_deals_a_tile_and_one_that_did_not_does_not"],
    ),
    (
        "a game already won is announced as won again",
        "        if self.board.status == GameStatus::Playing && self.board.has_won() {",
        "        if self.board.has_won() {",
        ["winning_a_second_time_is_not_offered_again"],
    ),
    (
        "a win is never noticed",
        "            self.board.status = GameStatus::Won;",
        "",
        ["reaching_the_winning_tile_wins_the_game"],
    ),
    (
        "a board with nothing left to do is never noticed",
        "        } else {\n            self.check_stuck();\n        }\n        true",
        "        }\n        true",
        ["filling_the_board_with_nothing_left_to_do_loses_the_game"],
    ),
    (
        "a board with moves left is called lost",
        "        if self.can_play() && !self.board.can_move() {",
        "        if self.can_play() {",
        ["an_ordinary_move_leaves_the_game_being_played"],
    ),
    (
        "keeping going does not re-ask whether the board is dead",
        "        self.board.status = GameStatus::WonContinuing;\n        // The winning move",
        "        self.board.status = GameStatus::WonContinuing;\n        return true;\n        // The winning move",
        ["winning_on_a_dead_board_ends_the_game_when_the_player_keeps_going"],
    ),
    (
        "keeping going is allowed from a game that has not won",
        "        if self.board.status != GameStatus::Won {\n            return false;\n        }",
        "",
        ["keeping_going_is_refused_by_a_game_that_has_not_won"],
    ),
    # ── Undo ──────────────────────────────────────────────────────────────
    (
        "an undo does not put the board back",
        "        board.grid = self.grid;",
        "",
        ["an_undo_puts_back_the_board_the_score_and_the_move_count"],
    ),
    (
        "an undo does not put the score back",
        "        board.score = self.score;",
        "",
        ["an_undo_puts_back_the_board_the_score_and_the_move_count"],
    ),
    (
        "an undo does not put the move count back",
        "        board.moves = self.moves;",
        "",
        ["an_undo_puts_back_the_board_the_score_and_the_move_count"],
    ),
    (
        "an undo does not put the status back",
        "        board.status = self.status;",
        "",
        ["an_undo_puts_back_the_status_so_a_win_can_be_taken_back"],
    ),
    (
        "an undo takes the best score back too",
        "        board.grid = self.grid;\n        board.score = self.score;",
        "        board.grid = self.grid;\n        board.score = self.score;\n        board.best_score = self.score;",
        ["the_best_score_survives_an_undo"],
    ),
    (
        "the history grows for ever",
        "        if self.undo_stack.len() > MAX_UNDO {\n            self.undo_stack.remove(0);\n        }",
        "",
        ["the_history_forgets_its_oldest_move_rather_than_growing_for_ever"],
    ),
    (
        "the history forgets its newest move rather than its oldest",
        "            self.undo_stack.remove(0);",
        "            self.undo_stack.pop();",
        ["the_history_forgets_its_oldest_move_rather_than_growing_for_ever"],
    ),
    (
        "the history is five moves deep",
        "const MAX_UNDO: usize = 50;",
        "const MAX_UNDO: usize = 5;",
        ["the_history_forgets_its_oldest_move_rather_than_growing_for_ever"],
    ),
    (
        "an undo with nothing behind it claims to have undone something",
        "            None => false,",
        "            None => true,",
        ["an_undo_with_nothing_behind_it_is_refused_rather_than_pretended"],
    ),
    # ── A new game ────────────────────────────────────────────────────────
    (
        "a new game forgets the best score",
        "        self.board = Board::new();\n        self.board.best_score = best;",
        "        self.board = Board::new();",
        ["a_new_game_throws_the_history_away_and_keeps_the_best_score"],
    ),
    (
        "a new game keeps the history of the old one",
        "        self.undo_stack.clear();\n        self.deal();",
        "        self.deal();",
        ["a_new_game_throws_the_history_away_and_keeps_the_best_score"],
    ),
    # ── What a key asks for ───────────────────────────────────────────────
    (
        "ctrl+z is not an undo",
        "    if ev.key == Key::Z && ev.modifiers.ctrl {",
        "    if false {",
        ["ctrl_z_undoes_and_a_bare_z_does_nothing"],
    ),
    (
        "a bare z is an undo",
        "    if ev.key == Key::Z && ev.modifiers.ctrl {",
        "    if ev.key == Key::Z {",
        ["ctrl_z_undoes_and_a_bare_z_does_nothing"],
    ),
    (
        "the window's own key combinations reach the board",
        "    if ev.modifiers.ctrl || ev.modifiers.alt {\n        return None;\n    }",
        "",
        ["a_ctrl_or_alt_arrow_belongs_to_the_window_and_not_to_the_board"],
    ),
    (
        "alt combinations reach the board and ctrl ones do not",
        "    if ev.modifiers.ctrl || ev.modifiers.alt {",
        "    if ev.modifiers.ctrl {",
        ["a_ctrl_or_alt_arrow_belongs_to_the_window_and_not_to_the_board"],
    ),
    (
        "up slides down",
        "        Key::Up | Key::W => Some(Intent::Move(Direction::Up)),",
        "        Key::Up | Key::W => Some(Intent::Move(Direction::Down)),",
        ["every_direction_key_and_its_letter_ask_for_the_same_move"],
    ),
    (
        "left slides right",
        "        Key::Left | Key::A => Some(Intent::Move(Direction::Left)),",
        "        Key::Left | Key::A => Some(Intent::Move(Direction::Right)),",
        ["every_direction_key_and_its_letter_ask_for_the_same_move"],
    ),
    (
        "the letter beside the arrow does nothing",
        "        Key::Left | Key::A => Some(Intent::Move(Direction::Left)),",
        "        Key::Left => Some(Intent::Move(Direction::Left)),",
        ["every_direction_key_and_its_letter_ask_for_the_same_move"],
    ),
    (
        "r does not start a new game",
        "        Key::N | Key::R => Some(Intent::NewGame),",
        "        Key::N => Some(Intent::NewGame),",
        ["the_keys_the_help_sheet_names_are_the_keys_the_program_reads"],
    ),
    (
        "enter does not keep the game going",
        "        Key::C | Key::Enter => Some(Intent::Continue),",
        "        Key::C => Some(Intent::Continue),",
        ["the_keys_the_help_sheet_names_are_the_keys_the_program_reads"],
    ),
    (
        "h opens the help sheet but cannot shut it",
        "        Key::H => Some(Intent::ToggleHelp),",
        "        Key::H => Some(Intent::CloseHelp),",
        ["h_toggles_the_help_rather_than_only_opening_it"],
    ),
    (
        "escape does not close the help sheet",
        "        Key::Escape => Some(Intent::CloseHelp),",
        "",
        ["escape_closes_the_help_and_otherwise_does_nothing"],
    ),
    (
        "a key release is a second press",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }",
        "",
        ["a_key_release_is_not_a_second_press"],
    ),
    (
        "a key the game has no use for is swallowed",
        "        let Some(intent) = key_intent(ev) else {\n            return EventResult::Ignored;\n        };",
        "        let Some(intent) = key_intent(ev) else {\n            return EventResult::Consumed;\n        };",
        ["a_key_the_game_has_no_use_for_is_left_for_someone_else"],
    ),
    # ── What a click asks for ─────────────────────────────────────────────
    (
        "a right click is a left click",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if false {",
        ["a_right_click_is_not_a_left_click"],
    ),
    (
        "a click on nothing at all is swallowed",
        "        let Some(target) = self.target_at(ev.x, ev.y) else {\n            return EventResult::Ignored;\n        };",
        "        let Some(target) = self.target_at(ev.x, ev.y) else {\n            return EventResult::Consumed;\n        };",
        ["a_click_on_nothing_at_all_is_left_for_someone_else"],
    ),
    (
        "a click is read against a fixed size rather than the window's",
        "        self.frame(self.width, self.height).hit_test(x, y)",
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hit_test(x, y)",
        ["a_click_is_read_against_the_size_the_frame_was_drawn_at"],
    ),
    (
        "clicking undo starts a new game",
        "        Target::Undo => Intent::Undo,",
        "        Target::Undo => Intent::NewGame,",
        ["every_key_and_every_button_go_through_the_same_intent"],
    ),
    (
        "clicking the help sheet does something else entirely",
        "        Target::HelpSheet => Intent::CloseHelp,",
        "        Target::HelpSheet => Intent::NewGame,",
        ["every_key_and_every_button_go_through_the_same_intent"],
    ),
    (
        "a refused move is reported as handled",
        "            Intent::Move(dir) => {\n                if self.make_move(dir) {\n                    EventResult::Consumed\n                } else {\n                    EventResult::Ignored\n                }\n            }",
        "            Intent::Move(dir) => {\n                self.make_move(dir);\n                EventResult::Consumed\n            }",
        ["a_direction_that_cannot_be_played_is_reported_as_unhandled"],
    ),
    (
        "a refused undo is reported as handled",
        "            Intent::Undo => {\n                if self.undo() {\n                    EventResult::Consumed\n                } else {\n                    EventResult::Ignored\n                }\n            }",
        "            Intent::Undo => {\n                self.undo();\n                EventResult::Consumed\n            }",
        ["an_undo_with_nothing_behind_it_is_refused_rather_than_pretended"],
    ),
    (
        "closing a sheet that is already shut is reported as handled",
        "            Intent::CloseHelp => {\n                if self.show_help {\n                    self.show_help = false;\n                    EventResult::Consumed\n                } else {\n                    EventResult::Ignored\n                }\n            }",
        "            Intent::CloseHelp => {\n                self.show_help = false;\n                EventResult::Consumed\n            }",
        ["escape_closes_the_help_and_otherwise_does_nothing"],
    ),
    (
        "toggling the help only ever opens it",
        "                self.show_help = !self.show_help;",
        "                self.show_help = true;",
        ["h_toggles_the_help_rather_than_only_opening_it"],
    ),
    (
        "a resize is not remembered",
        "            app.resize(*width as f32, *height as f32);",
        "            app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);",
        # The anchor is in the `Resize` handler, not in `render`, so the test
        # that draws through `render` never reaches it.
        ["resizing_the_window_moves_the_board"],
    ),
    # ── The window ────────────────────────────────────────────────────────
    (
        "the title and the app id disagree",
        '    fn title(&self) -> String {\n        "2048".to_string()',
        '    fn title(&self) -> String {\n        "Game".to_string()',
        ["the_program_names_itself_the_same_way_everywhere"],
    ),
    (
        "the app id is not the name the program is launched under",
        '    fn app_id(&self) -> String {\n        "game2048".to_string()',
        '    fn app_id(&self) -> String {\n        "2048".to_string()',
        ["the_program_names_itself_the_same_way_everywhere"],
    ),
    (
        "the window opens too small for its own layout",
        "fn initial_size(&self) -> (u32, u32) {\n        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "fn initial_size(&self) -> (u32, u32) {\n        (1, 1)",
        ["a_window_opens_at_a_size_its_own_layout_can_use"],
    ),
    (
        "the window will not close when it is asked to",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "",
        ["the_window_closes_when_it_is_asked_to"],
    ),
    (
        "an event the game ignored still asks for a repaint",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["a_move_asks_for_a_repaint_and_a_refused_one_does_not"],
    ),
    (
        "a move does not ask for a repaint",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        ["a_move_asks_for_a_repaint_and_a_refused_one_does_not"],
    ),
    (
        "the frame does not remember the size it was drawn at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["a_click_is_read_against_the_size_the_frame_was_drawn_at"],
    ),
    (
        "the window draws something other than the frame",
        "        self.frame(width, height).into_tree()",
        "        Frame::new(width, height).into_tree()",
        ["what_the_window_draws_is_what_the_frame_drew"],
    ),
    # ── Layout: the bands ─────────────────────────────────────────────────
    (
        "the type does not grow with the window",
        "        let font = (h / 40.0).clamp(8.0, 18.0);",
        # Typed, because `12.0` on its own leaves the numeric type ambiguous
        # at `(font - 3.0).max(7.0)` and the mutant would not compile.
        "        let font = 12.0_f32;",
        ["the_type_grows_with_the_window"],
    ),
    (
        "the small type is the body type",
        "        let small = (font - 3.0).max(7.0);",
        "        let small = font;",
        ["the_type_grows_with_the_window"],
    ),
    (
        "the padding can eat the thing it pads",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 12.0).min(w.min(h) / 4.0);",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 12.0);",
        ["the_board_keeps_its_share_of_every_window"],
    ),
    (
        "the board's share of the window is not reserved",
        "        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);",
        "        let budget = h;",
        ["the_board_keeps_its_share_of_every_window"],
    ),
    (
        "the board is promised a tenth of the window rather than its share",
        "const BOARD_SHARE: f32 = 0.42;",
        "const BOARD_SHARE: f32 = 0.1;",
        ["the_board_keeps_its_share_of_every_window"],
    ),
    (
        "the bands are given up from the top down",
        "const BAND_DROP_ORDER: [usize; 4] = [3, 1, 0, 2];",
        "const BAND_DROP_ORDER: [usize; 4] = [0, 1, 2, 3];",
        ["the_bands_are_given_up_from_the_bottom_up_as_the_window_shrinks"],
    ),
    (
        "the direction pad is the first band given up",
        "const BAND_DROP_ORDER: [usize; 4] = [3, 1, 0, 2];",
        "const BAND_DROP_ORDER: [usize; 4] = [2, 3, 1, 0];",
        ["the_bands_are_given_up_from_the_bottom_up_as_the_window_shrinks"],
    ),
    (
        "a band is shrunk rather than given up whole",
        "            if let Some(band) = wants.get_mut(i) {\n                *band = 0.0;\n            }",
        "            if let Some(band) = wants.get_mut(i) {\n                *band *= 0.5;\n            }",
        ["a_band_that_did_not_fit_is_gone_rather_than_flat"],
    ),
    (
        "a band that did not fit is flat rather than gone",
        "        let header = if hdr_h > 0.0 {\n            Rect::new(0.0, 0.0, w, hdr_h)\n        } else {\n            Rect::EMPTY\n        };",
        "        let header = Rect::new(0.0, 0.0, w, hdr_h);",
        ["a_band_that_did_not_fit_is_gone_rather_than_flat"],
    ),
    (
        "the info band sits at the top of the window",
        "            Rect::new(0.0, hdr_h, w, inf_h)",
        "            Rect::new(0.0, 0.0, w, inf_h)",
        ["the_bands_stack_down_the_window_in_the_order_they_are_named"],
    ),
    (
        "the direction pad sits below the footer",
        "            Rect::new(0.0, h - foot_h - dpad_h, w, dpad_h)",
        "            Rect::new(0.0, h - dpad_h, w, dpad_h)",
        ["the_bands_stack_down_the_window_in_the_order_they_are_named"],
    ),
    (
        "a band with no height still takes the clicks aimed at it",
        "    pub fn shows(&self, band: Rect) -> bool {\n        band.w > 0.0 && band.h > 0.0",
        "    pub fn shows(&self, band: Rect) -> bool {\n        true || band.w > 0.0 && band.h > 0.0",
        ["a_button_in_a_band_that_was_dropped_has_no_rectangle"],
    ),
    # ── Layout: the board ─────────────────────────────────────────────────
    (
        "the board is as wide as the window allows and as tall as it likes",
        "        let side = (w - pad * 2.0)\n            .max(0.0)\n            .min((bottom - top - pad * 2.0).max(0.0));",
        "        let side = (w - pad * 2.0).max(0.0);",
        ["the_board_is_square_and_inside_the_window_at_every_size"],
    ),
    (
        "the board is pinned to the left of the window",
        "            (w - side) / 2.0,\n            top + (bottom - top - side) / 2.0,",
        "            0.0,\n            top + (bottom - top - side) / 2.0,",
        ["the_board_is_square_and_inside_the_window_at_every_size"],
    ),
    (
        "a cell reads its row where it should read its column",
        "            self.board.x + col as f32 * step + gap / 2.0,",
        "            self.board.x + row as f32 * step + gap / 2.0,",
        ["a_tile_is_painted_in_the_cell_it_sits_in_and_not_its_mirror"],
    ),
    (
        "the gap is added to the step rather than taken out of the cell",
        "            (step - gap).max(0.0),\n            (step - gap).max(0.0),",
        "            step,\n            step,",
        ["the_gap_is_taken_out_of_the_cell_rather_than_added_to_the_step"],
    ),
    (
        "a cell off the board still has a rectangle",
        "        if self.board.is_empty() || row >= GRID_SIZE || col >= GRID_SIZE {",
        "        if self.board.is_empty() {",
        ["a_cell_off_the_board_has_no_rectangle_at_all"],
    ),
    # ── Layout: the score boxes, the banner, the buttons ──────────────────
    (
        "the two score boxes sit on top of one another",
        "        let x = right - (bw + gap) * (index as f32 + 1.0) + gap;",
        "        let x = right - (bw + gap) + gap;",
        ["the_score_boxes_sit_side_by_side_inside_the_header"],
    ),
    (
        "the score boxes are laid out from the left edge inwards",
        "        let x = right - (bw + gap) * (index as f32 + 1.0) + gap;",
        "        let x = self.header.x + self.pad + (bw + gap) * index as f32;",
        # The title test measures the title against wherever the boxes turn out
        # to be, so it cannot see the boxes themselves move (lesson 52).
        ["the_score_boxes_sit_side_by_side_inside_the_header"],
    ),
    (
        "a score box with no room left is drawn off the edge",
        "        if x < self.header.x {\n            return Rect::EMPTY;\n        }",
        "",
        ["a_score_box_with_no_room_left_is_dropped_rather_than_drawn_off_the_edge"],
    ),
    (
        "the title runs under the score boxes",
        "            .fold(l.header.right(), f32::min);",
        "            .fold(l.header.right(), f32::max);",
        ["the_title_makes_room_for_whichever_score_boxes_are_drawn"],
    ),
    (
        "the title makes room for the best box alone",
        "        let limit = [best, score]",
        "        let limit = [best]",
        ["the_title_makes_room_for_whichever_score_boxes_are_drawn"],
    ),
    (
        "the buttons in a row overlap one another",
        "        let bw = ((row.w - gap * (n + 1.0)) / n).max(0.0);",
        "        let bw = (row.w / n).max(0.0);",
        ["the_direction_buttons_sit_in_a_row_inside_the_pad_and_do_not_overlap"],
    ),
    (
        "the buttons in a row are all the first one",
        "            row.x + gap + index as f32 * (bw + gap),",
        "            row.x + gap,",
        ["the_direction_buttons_sit_in_a_row_inside_the_pad_and_do_not_overlap"],
    ),
    (
        "a button past the end of its row still has a rectangle",
        "        if index >= count {\n            return Rect::EMPTY;\n        }",
        "",
        # Owned by the row tests, which ask for a button past the end. The
        # dropped-band test only ever asks for index 0.
        [
            "the_direction_buttons_sit_in_a_row_inside_the_pad_and_do_not_overlap",
            "the_footer_buttons_sit_in_a_row_inside_the_footer_and_do_not_overlap",
        ],
    ),
    (
        "the banner covers more of the window than the board does",
        "        let bh = (self.board.h * 0.42).min(200.0);",
        "        let bh = self.board.h * 2.0;",
        ["the_banner_and_the_button_on_it_stay_inside_the_board"],
    ),
    (
        "the button on the banner runs off it",
        "        let bh = (b.h * 0.28).min(40.0);",
        "        let bh = b.h * 2.0;",
        ["the_banner_and_the_button_on_it_stay_inside_the_board"],
    ),
    # ── Drawing: what is written, and where ───────────────────────────────
    (
        "text is drawn at a size the renderer will not honour",
        "|| size < MIN_DRAWN_FONT ||",
        "|| size <= 0.0 ||",
        ["no_text_is_drawn_outside_the_window_it_belongs_to"],
    ),
    (
        "an empty line is drawn",
        "    if body.is_empty() || size < MIN_DRAWN_FONT",
        "    if size < MIN_DRAWN_FONT",
        ["a_label_with_no_room_left_is_not_drawn_at_all"],
    ),
    (
        "a label with no room to draw it in is drawn anyway",
        " || max_width.is_some_and(|w| w <= 0.0) {",
        " {",
        # No size in `WINDOWS` crowds a label down to no width at all, so the
        # owner is the test that searches for a width that does.
        ["a_title_crowded_out_by_the_score_boxes_is_not_drawn_at_all"],
    ),
    (
        "a line is written from the left edge of its box rather than centred",
        "        r.x + (r.w - tw).max(0.0) / 2.0,",
        "        r.x,",
        ["a_label_in_a_button_is_written_across_the_middle_of_it"],
    ),
    (
        "a line is written from the top of its box rather than centred",
        "        r.y + (r.h - th).max(0.0) / 2.0,",
        "        r.y,",
        ["a_label_in_a_button_is_written_across_the_middle_of_it"],
    ),
    (
        "a line taller than its box begins above it",
        "        r.y + (r.h - th).max(0.0) / 2.0,",
        "        r.y + (r.h - th) / 2.0,",
        # The layout sizes every line to fit its band, so an overflow only
        # happens at the rounding edge of a font size and no window in the
        # fixture reaches one. The rule belongs to `centred` and is asked of
        # `centred` directly.
        ["a_line_too_big_for_its_box_still_starts_inside_it"],
    ),
    (
        "a line wider than its box begins to the left of it",
        "        r.x + (r.w - tw).max(0.0) / 2.0,",
        "        r.x + (r.w - tw) / 2.0,",
        ["a_line_too_big_for_its_box_still_starts_inside_it"],
    ),
    (
        "an empty cell is written with a nought",
        "                if val == 0 {\n                    continue;\n                }",
        "",
        ["an_empty_cell_has_no_number_written_on_it"],
    ),
    (
        "a long number is not shrunk to fit its cell",
        "                if width > r.w * 0.84 && width > 0.0 {\n                    size *= r.w * 0.84 / width;\n                }",
        "",
        ["a_long_number_is_shrunk_to_fit_the_cell_it_is_in"],
    ),
    (
        "every tile is painted the colour of an empty one",
        "                fill(f, r, tile_face(val), (r.h * 0.12).min(8.0));",
        "                fill(f, r, tile_face(0), (r.h * 0.12).min(8.0));",
        ["a_tile_is_painted_in_its_own_colour_and_not_the_empty_one"],
    ),
    (
        "every tile takes the same ink",
        "        2 | 4 => COL_CRUST,\n        _ => COL_TEXT,",
        "        _ => COL_TEXT,",
        ["a_pale_tile_takes_dark_ink_and_a_dark_tile_light_ink"],
    ),
    (
        "the info line does not say how many moves have been made",
        "                self.board.moves,\n                self.board.highest_tile()",
        "                0,\n                self.board.highest_tile()",
        ["the_moves_and_the_highest_tile_are_both_on_screen"],
    ),
    (
        "the info line does not say the highest tile",
        "                self.board.moves,\n                self.board.highest_tile()",
        "                self.board.moves,\n                0",
        ["the_moves_and_the_highest_tile_are_both_on_screen"],
    ),
    (
        "the best box shows the score rather than the best",
        '        self.draw_score_box(f, l, best, "BEST", self.board.best_score);',
        '        self.draw_score_box(f, l, best, "BEST", self.board.score);',
        ["the_score_and_the_best_score_are_both_on_screen_and_are_told_apart"],
    ),
    (
        "both readouts are drawn in the same box",
        '        self.draw_score_box(f, l, score, "SCORE", self.board.score);',
        '        self.draw_score_box(f, l, best, "SCORE", self.board.score);',
        ["the_score_and_the_best_score_are_both_on_screen_and_are_told_apart"],
    ),
    (
        "a score box is drawn without its caption",
        "        let cap_h = (r.h * 0.4).max(0.0);",
        "        let cap_h = 0.0;",
        ["the_score_and_the_best_score_are_both_on_screen_and_are_told_apart"],
    ),
    # ── Drawing: the controls ─────────────────────────────────────────────
    (
        "the pad's four buttons are all the first one",
        "in Direction::ALL.iter().enumerate() {\n            let r = l.dpad_button(i);",
        "in Direction::ALL.iter().enumerate() {\n            let r = l.dpad_button(0);",
        ["the_direction_pad_reads_left_up_down_right"],
    ),
    (
        "the pad reads up, down, left, right",
        "    pub const ALL: [Direction; 4] = [\n        Direction::Left,\n        Direction::Up,\n        Direction::Down,\n        Direction::Right,\n    ];",
        "    pub const ALL: [Direction; 4] = [\n        Direction::Up,\n        Direction::Down,\n        Direction::Left,\n        Direction::Right,\n    ];",
        ["the_direction_pad_reads_left_up_down_right"],
    ),
    (
        "the up and down glyphs are swapped",
        '            Direction::Up => "^",\n            Direction::Down => "v",',
        '            Direction::Up => "v",\n            Direction::Down => "^",',
        ["the_direction_pad_reads_left_up_down_right"],
    ),
    (
        "the direction pad is never greyed",
        "                if playable { COL_SURFACE1 } else { COL_SURFACE0 },",
        "                COL_SURFACE1,",
        ["the_direction_buttons_are_greyed_while_the_board_is_frozen"],
    ),
    (
        "the undo button is never greyed",
        '            (Target::Undo, "Undo", !self.undo_stack.is_empty()),',
        '            (Target::Undo, "Undo", true),',
        ["the_undo_button_is_greyed_while_there_is_nothing_to_undo"],
    ),
    (
        "the footer's three buttons are all the first one",
        "in entries.iter().enumerate() {\n            let r = l.footer_button(i);",
        "in entries.iter().enumerate() {\n            let r = l.footer_button(0);",
        # Naming a control is not placing it: `control_names` reads the target
        # off the hit box and never asks where the box is (lesson 57).
        ["every_control_records_a_hit_box_where_it_was_painted"],
    ),
    (
        "a button records no hit box",
        "    f.hit(target, r);",
        "",
        ["every_control_the_program_has_can_be_reached_with_a_mouse"],
    ),
    (
        "the sheet takes only the clicks that land on it",
        "        f.hit(Target::HelpSheet, l.window);",
        "        f.hit(Target::HelpSheet, h);",
        ["the_help_sheet_hides_what_it_covers_from_a_click"],
    ),
    (
        "a key played a move on the board behind the sheet",
        "        if self.show_help && !matches!(intent, Intent::ToggleHelp | Intent::CloseHelp) {\n            self.show_help = false;\n            return EventResult::Consumed;\n        }",
        "",
        ["a_key_pressed_over_the_help_sheet_shuts_it_rather_than_playing_a_move"],
    ),
    (
        # The anchor is the *window*-wide hit box, not the sheet's own
        # rectangle: recording nothing at all leaves the sheet transparent to
        # the pointer, which is a different fault from the narrowing above.
        "the help sheet takes no clicks",
        "        f.hit(Target::HelpSheet, l.window);",
        "",
        ["the_help_sheet_swallows_the_click_that_closes_it"],
    ),
    (
        "the help sheet does not cover what it is drawn over",
        "        if self.show_help {\n            self.draw_help(&mut f, &l);\n        }",
        "        if self.show_help {\n            self.draw_help(&mut f, &l);\n        }\n        self.draw_dpad(&mut f, &l);",
        ["the_help_sheet_hides_what_it_covers_from_a_click"],
    ),
    # ── Drawing: the banner and the help sheet ────────────────────────────
    (
        "the loss banner offers to keep going",
        '            GameStatus::Lost => self.draw_banner(&mut f, &l, "Game over", COL_RED, false),',
        '            GameStatus::Lost => self.draw_banner(&mut f, &l, "Game over", COL_RED, true),',
        ["the_win_banner_offers_to_keep_going_and_the_loss_banner_does_not"],
    ),
    (
        "the win banner does not offer to keep going",
        '            GameStatus::Won => self.draw_banner(&mut f, &l, "You win!", COL_GREEN, true),',
        '            GameStatus::Won => self.draw_banner(&mut f, &l, "You win!", COL_GREEN, false),',
        ["the_win_banner_offers_to_keep_going_and_the_loss_banner_does_not"],
    ),
    (
        "a banner is drawn over a game still being played",
        "            GameStatus::Playing | GameStatus::WonContinuing => {}",
        '            GameStatus::Playing | GameStatus::WonContinuing => {\n                self.draw_banner(&mut f, &l, "You win!", COL_GREEN, true);\n            }',
        ["no_banner_is_drawn_over_a_game_still_being_played"],
    ),
    (
        "the banner does not say the score the game ended on",
        '            &format!("Score {}", self.board.score),',
        '            "Score",',
        ["the_banner_shows_the_score_the_game_ended_on"],
    ),
    (
        "the help sheet is drawn over a game nobody asked it about",
        "        if self.show_help {\n            self.draw_help(&mut f, &l);",
        "        if true {\n            self.draw_help(&mut f, &l);",
        ["the_help_sheet_is_only_drawn_when_it_is_open"],
    ),
    (
        "the help sheet has no band for its heading",
        "        let head_h = (h.h * 0.16).max(0.0);",
        "        let head_h = 0.0;",
        ["the_help_sheet_names_every_control_the_game_has"],
    ),
    (
        "the sheet's ladder has no band for the line that shuts it",
        "        let step = body_h / (rows + 1.0);",
        "        let step = body_h / rows;",
        ["every_line_of_the_help_sheet_is_written_on_the_sheet"],
    ),
    (
        "the sheet's rows are sized to the sheet rather than to their band",
        "        let size = l.small.min(step * 0.7);",
        "        let size = l.small;",
        ["every_line_of_the_help_sheet_is_written_on_the_sheet"],
    ),
    (
        "the sheet's rows are all written on one line",
        "            let y = h.y + head_h + l.pad + i as f32 * step;",
        "            let y = h.y + head_h + l.pad;",
        ["the_help_sheet_puts_each_key_beside_its_own_meaning"],
    ),
    (
        "the line that shuts the sheet is written over the last row",
        "            Rect::new(h.x, h.y + head_h + l.pad + rows * step, h.w, step),",
        "            Rect::new(h.x, h.y + head_h + l.pad, h.w, step),",
        ["the_help_sheet_puts_each_key_beside_its_own_meaning"],
    ),
    (
        "the key column takes the whole width of the sheet",
        "        let key_w = (h.w * 0.42).max(0.0);",
        "        let key_w = h.w;",
        ["the_help_sheet_puts_each_key_beside_its_own_meaning"],
    ),
    (
        "the meaning is written over the key it explains",
        "                h.x + l.pad + key_w,",
        "                h.x + l.pad,",
        ["the_help_sheet_puts_each_key_beside_its_own_meaning"],
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
            "game2048",
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
