"""Mutation test for the reversi suite.

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
    # -- Layout: the three bands ---------------------------------------
    (
        "the padding is allowed to be wider than the window it pads",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 20.0).min(w.min(h) / 2.0);",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 20.0);",
        ["the_padding_never_vanishes_never_runs_away_and_never_outgrows_the_window"],
    ),
    (
        "the padding vanishes on a small window instead of holding a floor",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 20.0).min(w.min(h) / 2.0);",
        "        let pad = (w.min(h) * 0.02).min(w.min(h) / 2.0);",
        ["the_padding_never_vanishes_never_runs_away_and_never_outgrows_the_window"],
    ),
    (
        "the padding runs away on a 4K window instead of holding a ceiling",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 20.0).min(w.min(h) / 2.0);",
        "        let pad = (w.min(h) * 0.02).max(2.0).min(w.min(h) / 2.0);",
        ["the_padding_never_vanishes_never_runs_away_and_never_outgrows_the_window"],
    ),
    (
        "the header is given a share of the width instead of the height",
        "        let header_h = h * 0.09;",
        "        let header_h = w * 0.09;",
        ["the_bands_are_shares_of_the_height_not_the_width"],
    ),
    (
        "the body starts at the top of the window, not under the header",
        "        let body = Rect::new(0.0, header.bottom(), w, body_h);",
        "        let body = Rect::new(0.0, 0.0, w, body_h);",
        ["the_bands_run_down_the_window_in_order_and_do_not_overlap"],
    ),
    (
        "the status bar is stacked on top of the body it follows",
        "        let status = Rect::new(0.0, body.bottom(), w, status_h);",
        "        let status = Rect::new(0.0, header.bottom(), w, status_h);",
        ["the_bands_run_down_the_window_in_order_and_do_not_overlap"],
    ),
    (
        "the status bar takes its share of the whole window, so the bands overrun it",
        "        let status_h = rest * 0.08;",
        "        let status_h = h * 0.08;",
        # Not the ordering test: both spellings still sum to the height, since
        # the body absorbs the difference, so the bands stay in order either
        # way and only a test that names the share can see it.
        ["the_status_bar_takes_its_share_of_what_the_header_left"],
    ),
    (
        "the panel is allowed to be wider than half the window",
        "        let panel_w = (w * 0.26).clamp(110.0, 260.0).min(w / 2.0);",
        "        let panel_w = (w * 0.26).clamp(110.0, 260.0);",
        ["the_panel_width_has_a_floor_a_ceiling_and_a_cap"],
    ),
    (
        "the panel loses its floor and becomes illegible on a narrow window",
        "        let panel_w = (w * 0.26).clamp(110.0, 260.0).min(w / 2.0);",
        "        let panel_w = (w * 0.26).min(260.0).min(w / 2.0);",
        ["the_panel_width_has_a_floor_a_ceiling_and_a_cap"],
    ),
    (
        "the panel loses its ceiling and eats a quarter of a wide window",
        "        let panel_w = (w * 0.26).clamp(110.0, 260.0).min(w / 2.0);",
        "        let panel_w = (w * 0.26).max(110.0).min(w / 2.0);",
        ["the_panel_width_has_a_floor_a_ceiling_and_a_cap"],
    ),
    (
        "the panel is laid over the board instead of beside it",
        "        let panel = Rect::new(board_area.right(), body.y, panel_w.min(body.w), body.h);",
        "        let panel = Rect::new(body.x, body.y, panel_w.min(body.w), body.h);",
        ["the_panel_sits_beside_the_board_and_neither_leaves_the_window"],
    ),
    (
        "the board area keeps the width the panel took",
        "        let board_area = Rect::new(body.x, body.y, (body.w - panel_w).max(0.0), body.h);",
        "        let board_area = Rect::new(body.x, body.y, body.w, body.h);",
        ["the_panel_sits_beside_the_board_and_neither_leaves_the_window"],
    ),
    # -- Board geometry ------------------------------------------------
    (
        "the board is fitted to the width alone and runs off a tall window",
        "        let step = ((area.w - label).max(0.0) / side)\n            .min((area.h - label).max(0.0) / side)\n            .max(0.0);",
        "        let step = ((area.w - label).max(0.0) / side).max(0.0);",
        ["the_board_fits_the_room_it_was_given_at_every_size"],
    ),
    (
        "the board is fitted to the height alone and runs off a wide window",
        "        let step = ((area.w - label).max(0.0) / side)\n            .min((area.h - label).max(0.0) / side)\n            .max(0.0);",
        "        let step = ((area.h - label).max(0.0) / side).max(0.0);",
        ["the_board_fits_the_room_it_was_given_at_every_size"],
    ),
    (
        "the labels are not counted when the board is fitted, so it overflows",
        "        let step = ((area.w - label).max(0.0) / side)\n            .min((area.h - label).max(0.0) / side)\n            .max(0.0);",
        "        let step = (area.w / side).min(area.h / side).max(0.0);",
        ["the_board_fits_the_room_it_was_given_at_every_size"],
    ),
    (
        "the squares are stepped by row where they should be stepped by column",
        "            self.origin.0 + f32_from_i32(col) * self.step,\n            self.origin.1 + f32_from_i32(row) * self.step,",
        "            self.origin.0 + f32_from_i32(row) * self.step,\n            self.origin.1 + f32_from_i32(col) * self.step,",
        ["the_board_is_lettered_and_numbered_the_othello_way"],
    ),
    (
        "the squares overlap by a fraction of a step",
        "            self.step,\n            self.step,\n        )\n    }\n\n    /// The middle of the square",
        "            self.step * 1.1,\n            self.step * 1.1,\n        )\n    }\n\n    /// The middle of the square",
        ["no_two_squares_share_a_pixel", "the_squares_tile_the_board_edge_to_edge"],
    ),
    (
        "the piece is drawn at the corner of its square rather than the middle",
        "        (r.x + r.w / 2.0, r.y + r.h / 2.0)",
        "        (r.x, r.y)",
        ["the_centre_of_a_square_is_its_middle"],
    ),
    (
        "the board rect does not reach the last square",
        "        let board = self.step * f32_from_usize(BOARD_SIZE);\n        Rect::new(self.origin.0, self.origin.1, board, board)",
        "        let board = self.step * f32_from_usize(BOARD_SIZE - 1);\n        Rect::new(self.origin.0, self.origin.1, board, board)",
        ["the_squares_tile_the_board_edge_to_edge"],
    ),
    # -- The two spellings of the board size ---------------------------
    (
        "the signed spelling of the board size drifts from the unsigned one",
        "const SIDE: i32 = 8;",
        "const SIDE: i32 = 7;",
        ["the_two_spellings_of_the_board_size_agree"],
    ),
    (
        "the last index is the side rather than one short of it",
        "const LAST: i32 = SIDE - 1;",
        "const LAST: i32 = SIDE;",
        ["the_two_spellings_of_the_board_size_agree"],
    ),
    # -- The rules -----------------------------------------------------
    (
        "the opening position is dealt with the colours the other way round",
        "        board.set(Pos::new(3, 3), Cell::White);\n        board.set(Pos::new(3, 4), Cell::Black);",
        "        board.set(Pos::new(3, 3), Cell::Black);\n        board.set(Pos::new(3, 4), Cell::White);",
        ["the_opening_position_is_the_standard_one"],
    ),
    (
        "a run closed by nothing is flanked anyway",
        "        Vec::new()\n    }\n\n    /// Everything `color` would flip",
        "        flipped\n    }\n\n    /// Everything `color` would flip",
        ["a_flank_needs_ones_own_piece_to_close_it", "a_run_that_reaches_the_edge_flips_nothing"],
    ),
    (
        "an empty square in the middle of a run is walked through",
        "            } else {\n                break;\n            }\n            at = Pos::new(at.row.saturating_add(dr), at.col.saturating_add(dc));",
        "            }\n            at = Pos::new(at.row.saturating_add(dr), at.col.saturating_add(dc));",
        ["a_flank_needs_ones_own_piece_to_close_it"],
    ),
    (
        "a square that already holds a piece may be played on top of",
        "        if !pos.in_bounds() || self.get(pos) != Cell::Empty {\n            return false;\n        }",
        "        if !pos.in_bounds() {\n            return false;\n        }",
        ["an_occupied_or_off_board_square_is_never_legal"],
    ),
    (
        "flips are collected from one direction instead of all eight",
        "        DIRECTIONS\n            .iter()\n            .flat_map(|&(dr, dc)| self.flips_in_direction(pos, color, dr, dc))\n            .collect()",
        "        self.flips_in_direction(pos, color, 0, 1)",
        ["a_move_flips_in_every_direction_at_once"],
    ),
    (
        "an illegal move is played anyway, flipping nothing",
        "        let flips = self.get_flips(pos, color);\n        if flips.is_empty() {\n            return 0;\n        }",
        "        let flips = self.get_flips(pos, color);",
        ["an_illegal_move_changes_nothing_at_all"],
    ),
    (
        "the played square is not claimed, only the pieces it flanks",
        "        self.set(pos, color);\n        for flip_pos in &flips {",
        "        for flip_pos in &flips {",
        ["a_move_flips_in_every_direction_at_once"],
    ),
    (
        "the empty count is the count of one colour",
        "        self.count(Cell::Empty)",
        "        self.count(Cell::Black)",
        ["the_opening_position_is_the_standard_one"],
    ),
    (
        "a tie is scored as a win for black",
        "            Ordering::Equal => Cell::Empty,",
        "            Ordering::Equal => Cell::Black,",
        ["the_winner_is_whoever_has_more_and_equal_is_neither"],
    ),
    (
        "the game is over as soon as one side cannot move",
        "        !self.has_legal_move(Cell::Black) && !self.has_legal_move(Cell::White)",
        "        !self.has_legal_move(Cell::Black) || !self.has_legal_move(Cell::White)",
        ["the_game_is_over_only_when_neither_side_can_move"],
    ),
    (
        "columns are lettered from A rather than a",
        "        .and_then(|c| b'a'.checked_add(c))",
        "        .and_then(|c| b'A'.checked_add(c))",
        ["column_letters_run_a_to_h_and_refuse_everything_else"],
    ),
    (
        "a column off the board is lettered as though it were on it",
        "        .filter(|_| (0..SIDE).contains(&col))",
        "        .filter(|_| true)",
        ["column_letters_run_a_to_h_and_refuse_everything_else"],
    ),
    (
        "ranks are numbered from zero, not from one",
        "            self.pos.row.saturating_add(1),",
        "            self.pos.row,",
        ["notation_names_the_square_the_way_othello_does"],
    ),
    # -- The search ----------------------------------------------------
    (
        "the evaluation counts the opponent's pieces as its own",
        "        .saturating_sub(board.count(opponent))",
        "        .saturating_add(board.count(opponent))",
        ["the_evaluation_is_exactly_the_opposite_from_the_other_side"],
    ),
    (
        "a corner is worth no more than the square that gives it away",
        "        if cell == color {\n            score = score.saturating_add(w);\n        } else if cell == opponent {\n            score = score.saturating_sub(w);\n        }",
        "        let _ = w;",
        ["the_evaluation_prefers_a_corner_to_the_square_that_gives_one_away"],
    ),
    (
        "a square off the board is weighted as though it were the first one",
        "    POSITION_WEIGHTS.get(r).and_then(|row| row.get(c)).copied()",
        "    Some(POSITION_WEIGHTS.get(r).and_then(|row| row.get(c)).copied().unwrap_or(100))",
        ["the_evaluation_prefers_a_corner_to_the_square_that_gives_one_away"],
    ),
    (
        "a pass costs the search nothing, so the ply buys a move as well",
        "        return minimax(\n            board,\n            depth.saturating_sub(1),",
        "        return minimax(\n            board,\n            depth,",
        # Not the termination test: a board where *both* sides pass is a board
        # `is_game_over` has already returned on, so the line never runs there.
        # It takes a one-sided board to reach it at all.
        ["a_pass_costs_the_search_a_ply_exactly_as_a_move_does"],
    ),
    (
        "the search minimises where it should maximise",
        "        if best.is_none_or(|(seen, _)| score > seen) {",
        "        if best.is_none_or(|(seen, _)| score < seen) {",
        ["the_search_takes_a_corner_when_one_is_offered"],
    ),
    (
        "the search reads its own reply as the opponent's",
        "        let score = minimax(\n            &next,\n            AI_DEPTH.saturating_sub(1),\n            i32::MIN,\n            i32::MAX,\n            false,",
        "        let score = minimax(\n            &next,\n            AI_DEPTH.saturating_sub(1),\n            i32::MIN,\n            i32::MAX,\n            true,",
        # Not the corner test: a free corner is the best move under either
        # convention, so it takes a position where the two disagree.
        ["the_search_reads_its_reply_as_the_opponents_and_not_as_its_own"],
    ),
    # -- Turns ---------------------------------------------------------
    (
        "white opens instead of the human",
        "            current_turn: Cell::Black, // Black, the human, opens.",
        "            current_turn: Cell::White,",
        ["black_opens_with_the_cursor_on_the_board"],
    ),
    (
        "the cursor may be walked off the board",
        "        if want.in_bounds() && want != self.cursor {",
        "        if want != self.cursor {",
        ["the_arrow_keys_walk_the_cursor_and_stop_at_the_edges"],
    ),
    (
        "an edge press claims to have moved the cursor",
        "        } else {\n            // The key was for us and we could not act on it; the window still\n            // has no reason to redraw.\n            EventResult::Ignored\n        }",
        "        } else {\n            EventResult::Consumed\n        }",
        ["the_arrow_keys_walk_the_cursor_and_stop_at_the_edges"],
    ),
    (
        "the arrow keys are transposed",
        "            Key::Up => self.move_cursor(-1, 0),\n            Key::Down => self.move_cursor(1, 0),",
        "            Key::Up => self.move_cursor(0, -1),\n            Key::Down => self.move_cursor(0, 1),",
        ["the_arrow_keys_walk_the_cursor_and_stop_at_the_edges"],
    ),
    (
        "an illegal move is refused in silence",
        '            self.notice = Some(String::from(\n                "Illegal move -- a move must flip at least one piece.",\n            ));\n            return;',
        "            return;",
        ["an_illegal_square_is_refused_with_a_notice_and_no_piece"],
    ),
    (
        "an illegal move is played anyway",
        "        if !self.board.is_legal_move(pos, self.current_turn) {",
        "        if false {",
        ["an_illegal_square_is_refused_with_a_notice_and_no_piece"],
    ),
    (
        "the notice from the move before is left standing over the next one",
        "        self.notice = None;\n        let pos = self.cursor;",
        "        let pos = self.cursor;",
        ["a_refusal_does_not_outlive_the_move_that_answers_it"],
    ),
    (
        "the turn is handed on whether or not the next player can move",
        "        if self.board.has_legal_move(next) {\n            self.current_turn = next;\n        } else if self.board.has_legal_move(self.current_turn) {",
        "        if true {\n            self.current_turn = next;\n        } else if self.board.has_legal_move(self.current_turn) {",
        ["a_pass_notice_lives_long_enough_to_be_read"],
    ),
    (
        "a player who has to pass is not told so",
        '            self.notice = Some(format!(\n                "{} cannot move -- {} plays again.",\n                color_name(next),\n                color_name(self.current_turn)\n            ));',
        "            self.notice = None;",
        ["a_pass_notice_lives_long_enough_to_be_read"],
    ),
    (
        "the game runs on after neither side can move",
        "        } else {\n            self.phase = Phase::GameOver;\n            return;\n        }",
        "        } else {\n            return;\n        }",
        ["the_game_ends_when_the_move_leaves_neither_side_a_move"],
    ),
    (
        "the search is never asked for a reply",
        "        if self.current_turn == Cell::White {\n            self.do_ai_move();\n        }",
        "        if false {\n            self.do_ai_move();\n        }",
        ["a_legal_square_is_played_and_the_ai_answers_in_the_same_event"],
    ),
    (
        "a search that finds no move leaves white to move forever",
        '                self.current_turn = Cell::Black;\n                self.notice = Some(String::from("White did not move."));\n                return;',
        "                return;",
        ["an_ai_that_finds_no_move_hands_the_turn_back_rather_than_freezing"],
    ),
    (
        "white keeps the turn after playing its reply",
        "            self.current_turn = Cell::Black;\n\n            if self.board.has_legal_move(Cell::Black) {",
        "            if self.board.has_legal_move(Cell::Black) {",
        ["a_legal_square_is_played_and_the_ai_answers_in_the_same_event"],
    ),
    (
        "keys are accepted while the search is thinking",
        "        if self.current_turn != Cell::Black {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["keys_are_ignored_while_it_is_not_blacks_turn"],
    ),
    (
        "a finished game still walks its cursor around",
        "            Phase::GameOver => self.handle_game_over_key(key),",
        "            Phase::GameOver => self.handle_playing_key(key),",
        ["once_it_is_over_only_a_new_game_key_does_anything"],
    ),
    (
        "a new game forgets how big the window is",
        "        let size = self.size;\n        *self = Self::new();\n        self.size = size;",
        "        *self = Self::new();",
        ["a_new_game_forgets_the_position_and_keeps_the_window"],
    ),
    (
        "N does not deal a new game",
        "            Key::N => {\n                self.restart();\n                EventResult::Consumed\n            }",
        "            Key::N => EventResult::Consumed,",
        ["a_new_game_forgets_the_position_and_keeps_the_window"],
    ),
    (
        "a negative window size is taken at face value",
        "        self.size = (width.max(0.0), height.max(0.0));",
        "        self.size = (width, height);",
        ["a_negative_size_is_read_as_no_size_at_all"],
    ),
    (
        "the status line is the standing one even once the game is over",
        "        if self.phase == Phase::GameOver {\n            return self.game_over_message();\n        }",
        "        if false {\n            return self.game_over_message();\n        }",
        ["the_game_ends_when_the_move_leaves_neither_side_a_move"],
    ),
    (
        "the notice is composed and then not shown",
        "        if let Some(notice) = &self.notice {\n            return notice.clone();\n        }",
        "        if let Some(notice) = &self.notice {\n            let _ = notice;\n        }",
        ["a_pass_notice_lives_long_enough_to_be_read", "an_illegal_square_is_refused_with_a_notice_and_no_piece"],
    ),
    (
        "a win for white is announced as a win for black",
        '            Cell::White => "White wins!",',
        '            Cell::White => "Black wins!",',
        # Not the end-of-game test: the only finished game it reaches is one
        # Black wins, so the white arm is never read there.
        ["the_result_line_names_whoever_actually_won"],
    ),
    # -- Drawing -------------------------------------------------------
    (
        "the window is not painted before anything is drawn on it",
        "        f.push(RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: l.window.w,\n            height: l.window.h,\n            color: BASE,\n            corner_radii: CornerRadii::ZERO,\n        });",
        "",
        ["the_frame_paints_the_whole_window_and_closes_every_clip"],
    ),
    (
        "the clip is pushed and never popped",
        "        self.draw_status(&l, &mut f);\n        f.unclip();",
        "        self.draw_status(&l, &mut f);",
        ["the_frame_paints_the_whole_window_and_closes_every_clip"],
    ),
    (
        "a square's hit box is not the square that was painted",
        "            f.hit(Target::Square(byte(pos.row), byte(pos.col)), square);",
        "            f.hit(\n                Target::Square(byte(pos.row), byte(pos.col)),\n                Rect::new(square.x, square.y, square.w * 0.5, square.h * 0.5),\n            );",
        ["the_hit_box_of_a_square_is_the_square_that_was_painted"],
    ),
    (
        "the row and column of a square's hit box are swapped",
        "            f.hit(Target::Square(byte(pos.row), byte(pos.col)), square);",
        "            f.hit(Target::Square(byte(pos.col), byte(pos.row)), square);",
        ["the_hit_box_of_a_square_is_the_square_that_was_painted"],
    ),
    (
        "the board keeps no box of its own, so nothing can name it",
        "        f.hit(Target::Board, board);\n",
        "",
        # Deleting it cannot swallow a click -- it is recorded *before* the
        # squares, and the frame answers with whatever was recorded last. What
        # it does is leave the board unnameable, and the test that measures the
        # labels against the board is the one that says so.
        ["the_labels_sit_outside_the_board_not_on_it"],
    ),
    (
        "the board's own box is recorded over its squares, swallowing every click",
        "            f.hit(Target::Square(byte(pos.row), byte(pos.col)), square);\n        }",
        "            f.hit(Target::Square(byte(pos.row), byte(pos.col)), square);\n        }\n        f.hit(Target::Board, board);",
        ["clicking_a_square_plays_it"],
    ),
    (
        "the column letters are drawn over the board rather than above it",
        "                board.y - g.label + (g.label - ink.height()) / 2.0,",
        "                board.y + (g.label - ink.height()) / 2.0,",
        ["the_labels_sit_outside_the_board_not_on_it"],
    ),
    (
        "the row numbers are drawn over the board rather than beside it",
        "                board.x - g.label + (g.label - nw) / 2.0,",
        "                board.x + (g.label - nw) / 2.0,",
        ["the_labels_sit_outside_the_board_not_on_it"],
    ),
    (
        "the ranks are numbered upward, the way chess numbers them",
        '            let number = format!("{}", i.saturating_add(1));',
        '            let number = format!("{}", SIDE - i);',
        ["the_board_is_lettered_and_numbered_the_othello_way"],
    ),
    (
        "every square takes the piece branch, so no square is ever dotted",
        "            let cell = self.board.get(pos);\n            if cell.is_piece() {",
        "            let cell = self.board.get(pos);\n            if true {",
        # Not the pieces test: `draw_piece` returns on an empty cell, so this
        # draws no piece that was not there. What it does is take the branch
        # away from the dot, which is the `else` of the same `if`.
        ["the_legal_squares_are_dotted_and_only_they_are"],
    ),
    (
        "the piece is drawn bigger than the square it sits on",
        "                draw_piece(f, cx, cy, g.step * 0.37, cell);",
        "                draw_piece(f, cx, cy, g.step * 0.9, cell);",
        ["a_piece_fits_inside_the_square_it_sits_on"],
    ),
    (
        "every empty square is dotted, not only the legal ones",
        "                && legal.contains(&pos)",
        "                && !legal.contains(&pos)",
        ["the_legal_squares_are_dotted_and_only_they_are"],
    ),
    (
        "a finished game still dots the squares nobody may play",
        "            } else if self.phase == Phase::Playing\n                && self.current_turn == Cell::Black",
        "            } else if self.current_turn == Cell::Black",
        ["a_finished_game_shows_no_cursor_and_no_dots"],
    ),
    (
        "the cursor is ringed on every square at once",
        "            if pos == self.cursor && self.phase == Phase::Playing {",
        "            if self.phase == Phase::Playing {",
        ["the_cursor_is_ringed_where_it_stands_and_nowhere_else"],
    ),
    (
        "a finished game still invites a move with a cursor",
        "            if pos == self.cursor && self.phase == Phase::Playing {",
        "            if pos == self.cursor {",
        ["a_finished_game_shows_no_cursor_and_no_dots"],
    ),
    (
        "every square is highlighted as the last move",
        "            if self.last_move == Some(pos) {",
        "            if self.last_move.is_some() {",
        ["the_last_move_is_highlighted_on_the_square_it_was_played"],
    ),
    (
        "the panel says the game is over while it is being played",
        '            (Phase::Playing, Cell::White) => ("White to move", PEACH),\n            (Phase::Playing, _) => ("Your turn (Black)", BLUE),',
        '            (Phase::Playing, Cell::White) => ("Game Over", PEACH),\n            (Phase::Playing, _) => ("Game Over", BLUE),',
        # Nothing asserted the line while the game was being played: the
        # finished-game tests only ever read the arm that was not changed.
        ["the_panel_names_whose_turn_it_is_and_only_says_so_while_there_is_one"],
    ),
    (
        "the score bar is split by the count of moves, not of pieces",
        "        let black_w = f32_from_i32(black) / total * bar.w;",
        "        let black_w = f32_from_i32(black) / total * bar.w * 0.5;",
        ["the_score_bar_is_split_in_proportion_to_the_two_counts"],
    ),
    (
        "an empty board divides the score bar by zero",
        # The guard is the `if`, not a floor under the divisor: a `.max(1.0)`
        # outside the `if` could never fire, so the sweep could delete it and
        # no test could notice. The division now lives inside the guard.
        "        if black > 0 {",
        "        if true {",
        ["an_empty_board_does_not_divide_the_score_bar_by_zero"],
    ),
    (
        "the move count is the count of pieces",
        '            &format!("Moves: {}", self.move_history.len()),',
        '            &format!("Moves: {}", self.board.count(Cell::Black)),',
        ["the_panel_says_the_score_the_board_holds"],
    ),
    (
        "the empty count in the panel is not the board's",
        '            &format!("Empty: {}", self.board.empty_count()),',
        '            &format!("Empty: {}", self.board.count(Cell::Black)),',
        ["the_panel_says_the_score_the_board_holds"],
    ),
    (
        "the last-move line names the first move instead",
        "        if let Some(last) = self.move_history.last() {",
        "        if let Some(last) = self.move_history.first() {",
        ["the_panel_says_the_score_the_board_holds"],
    ),
    (
        "the history fills the panel and runs through the help text",
        "        let rows = count_from_f32((help_top - y) / row_ink.height());",
        "        let rows = self.move_history.len();",
        ["the_history_shows_the_newest_moves_and_never_runs_into_the_help"],
    ),
    (
        "the history shows the oldest moves rather than the newest",
        "        let start = self.move_history.len().saturating_sub(rows);",
        "        let start = 0;",
        ["the_history_shows_the_newest_moves_and_never_runs_into_the_help"],
    ),
    (
        "the help floats where the history left off instead of sitting on the floor",
        "        let help_top = (inner.bottom() - help_h).max(y);",
        "        let help_top = y;",
        ["the_history_shows_the_newest_moves_and_never_runs_into_the_help"],
    ),
    (
        "the status band shows something other than the status",
        "        let drawn = label_in(f, band, &self.status(), ink);",
        '        let drawn = label_in(f, band, "Reversi", ink);',
        ["the_status_line_is_drawn_and_is_the_line_the_state_derives"],
    ),
    (
        "the panel is drawn outside the window it belongs to",
        "        let band = inset(l.panel, l.pad);\n        f.push(RenderCommand::FillRect {\n            x: band.x,",
        "        let band = inset(l.panel, l.pad).translated(0.0, 20.0);\n        f.push(RenderCommand::FillRect {\n            x: band.x,",
        # Not a window-containment test: the frame clips to the window, so a
        # band moved off the panel is still painted somewhere the window
        # allows. It has to be measured against the panel.
        ["the_panel_is_painted_in_the_room_the_layout_gave_it"],
    ),
    (
        "an inset box is allowed to turn itself inside out",
        "        (rect.w - pad * 2.0).max(0.0),\n        (rect.h - pad * 2.0).max(0.0),",
        "        rect.w - pad * 2.0,\n        rect.h - pad * 2.0,",
        # Reachable only where a band is thinner than twice the padding, which
        # no window in `SIZES` is -- so the test that finds it calls `inset`
        # directly rather than waiting for a window small enough to show it.
        ["an_inset_never_turns_a_box_inside_out"],
    ),
    # -- The wiring ----------------------------------------------------
    (
        "a click is read against the size the window opened at, not its size now",
        "        let (w, h) = self.size;\n        let Some(target) = self.frame(w, h).hit_test(x, y) else {",
        "        let Some(target) = self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hit_test(x, y) else {",
        ["a_resize_event_is_the_size_the_next_click_is_read_against"],
    ),
    (
        "a click plays a square other than the one it landed on",
        "                self.cursor = Pos::new(i32::from(row), i32::from(col));",
        "                self.cursor = Pos::new(i32::from(col), i32::from(row));",
        ["clicking_a_square_plays_it"],
    ),
    (
        "a click moves the cursor but does not play",
        "                self.cursor = Pos::new(i32::from(row), i32::from(col));\n                self.try_place_piece();",
        "                self.cursor = Pos::new(i32::from(row), i32::from(col));",
        ["clicking_a_square_plays_it"],
    ),
    (
        "a click on the furniture falls through to the window",
        "            _ => EventResult::Consumed,\n        }\n    }\n\n    // \u2500\u2500 Drawing",
        "            _ => EventResult::Ignored,\n        }\n    }\n\n    // \u2500\u2500 Drawing",
        ["a_click_on_the_furniture_is_answered_and_changes_nothing"],
    ),
    (
        "a finished game accepts a click as a move",
        "            Target::Square(row, col)\n                if self.phase == Phase::Playing && self.current_turn == Cell::Black =>",
        "            Target::Square(row, col) if self.current_turn == Cell::Black =>",
        ["a_click_once_the_game_is_over_is_answered_and_plays_nothing"],
    ),
    (
        "any button plays, not only the left one",
        "        if button != MouseButton::Left {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["only_the_left_button_plays"],
    ),
    (
        "a key release is acted on as though it were a press",
        "        Event::Key(KeyEvent {\n            key, pressed: true, ..\n        }) => app.handle_key(*key),",
        "        Event::Key(KeyEvent { key, .. }) => app.handle_key(*key),",
        ["a_key_arriving_at_the_window_reaches_the_game"],
    ),
    (
        "a resize event is not noted, so the next click is read against the old size",
        "            app.resize(f32_from_u32(*width), f32_from_u32(*height));",
        "            let _ = (width, height);",
        ["a_resize_event_is_the_size_the_next_click_is_read_against"],
    ),
    (
        "rendering does not note the size it was given",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_notes_the_size_it_was_given"],
    ),
    (
        "the close button does not close the window",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        if false {\n            return Response::Exit;\n        }",
        ["the_window_closes_on_the_close_button_and_on_escape"],
    ),
    (
        "escape does not close the window",
        "                key: Key::Escape,\n                pressed: true,",
        "                key: Key::F1,\n                pressed: true,",
        ["the_window_closes_on_the_close_button_and_on_escape"],
    ),
    (
        "a key that changed nothing still asks for a redraw",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["a_key_that_changes_something_asks_for_a_redraw_and_one_that_does_not_does_not"],
    ),
    (
        "the window opens at a size the layout was not written against",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (640, 480)",
        ["the_window_names_itself"],
    ),
    (
        "the window names itself something else",
        '        "Reversi".to_string()',
        '        "Othello".to_string()',
        ["the_window_names_itself"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "reversi", timeout=300, only=sys.argv[1:] or None))
