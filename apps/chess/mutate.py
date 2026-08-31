"""Mutation test for chess's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Chess is the thirty-eighth application in this campaign.  Its old suite was
large -- 97 tests -- and it was careful about the rules: castling rights,
en passant, promotion, the fifty-move draw, mate and stalemate all had tests
and all of them passed.  None of that could see what was actually wrong with
the program, because the program had no window.  `main` was
`let _app = ChessApp::new();` -- it built the opening position, dropped it and
exited.  `render` took no width and no height, so the board was drawn from
`SQUARE_SIZE = 64.0` and `BOARD_OFFSET_X` into whatever window it was given,
and `square_at` resolved a click from the same two constants: the picture and
the hit test agreed with each other everywhere and with the screen in exactly
one window size.  Eleven crate-level `#![allow]`s hid 68 arithmetic and 14
indexing findings from the lint that would have named most of it.

Usage:  python -u apps/chess/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The layout follows the window ─────────────────────────────────────
    (
        "the board takes the width without checking the height",
        "        let side = free_w.min(free_h).max(0.0);",
        "        let side = free_w.max(0.0);",
        ["the_board_stays_inside_the_window_it_was_given"],
    ),
    (
        "the square is the old 64 px constant again",
        "        let square = grid / 8.0;",
        "        let square = 64.0;",
        ["the_board_stays_inside_the_window_it_was_given"],
    ),
    (
        "the grid is divided into seven files instead of eight",
        "        let square = grid / 8.0;",
        "        let square = grid / 7.0;",
        ["the_board_stays_inside_the_window_it_was_given"],
    ),
    (
        "the grid starts at the board's edge, under the rank labels",
        "        let origin = (board.x + margin, board.y);",
        "        let origin = (board.x, board.y);",
        ["the_rank_and_file_labels_name_the_ranks_and_files_they_sit_beside"],
    ),
    (
        "the board starts at the top of the window, under the header",
        "        let board = Rect::new(\n            ((free_w - side) / 2.0).max(0.0),\n            header.bottom() + ((free_h - side) / 2.0).max(0.0),",
        "        let board = Rect::new(\n            ((free_w - side) / 2.0).max(0.0),\n            ((free_h - side) / 2.0).max(0.0),",
        ["the_board_stays_inside_the_window_it_was_given"],
    ),
    (
        "the panel is drawn however narrow the window is",
        "        let panel_w = if w - want_panel >= h * 0.45 && want_panel <= w * 0.4 {",
        "        let panel_w = if true {",
        ["the_panel_is_dropped_rather_than_drawn_too_narrow_to_read"],
    ),
    (
        "the panel is never drawn at all",
        "        let panel_w = if w - want_panel >= h * 0.45 && want_panel <= w * 0.4 {",
        "        let panel_w = if false {",
        ["the_panel_is_dropped_rather_than_drawn_too_narrow_to_read"],
    ),
    (
        "the panel is sized without measuring the lines it holds",
        "        let panel_w_min = widest(&CONTROLS, FontWeightHint::Regular)\n            .max(widest(&CAPTURED_HEADINGS, FontWeightHint::Bold))\n            + pad * 2.0;",
        "        let panel_w_min = 20.0;",
        ["the_panel_is_wide_enough_for_the_lines_it_holds"],
    ),
    (
        "the panel overlaps the board it sits beside",
        "        let panel = Rect::new(free_w, header.bottom(), panel_w, free_h);",
        "        let panel = Rect::new(free_w - panel_w, header.bottom(), panel_w, free_h);",
        ["the_board_and_the_panel_do_not_overlap"],
    ),
    # ── The board's rows and columns ──────────────────────────────────────
    (
        "the board is drawn upside down",
        "        let screen_row = f32::from(7i8.saturating_sub(pos.row));",
        "        let screen_row = f32::from(pos.row);",
        ["white_is_drawn_at_the_bottom_of_the_window"],
    ),
    (
        "the files run right to left",
        "            self.origin.0 + f32::from(pos.col) * self.square,",
        "            self.origin.0 + f32::from(7i8.saturating_sub(pos.col)) * self.square,",
        ["files_run_left_to_right"],
    ),
    (
        "every square is drawn in the same place",
        "            self.origin.0 + f32::from(pos.col) * self.square,\n            self.origin.1 + screen_row * self.square,",
        "            self.origin.0,\n            self.origin.1,",
        ["the_squares_do_not_overlap_each_other"],
    ),
    (
        "the row and the column are swapped",
        "        Rect::new(\n            self.origin.0 + f32::from(pos.col) * self.square,\n            self.origin.1 + screen_row * self.square,",
        "        Rect::new(\n            self.origin.0 + screen_row * self.square,\n            self.origin.1 + f32::from(pos.col) * self.square,",
        ["white_is_drawn_at_the_bottom_of_the_window"],
    ),
    # ── The hit boxes are what a click is answered by ─────────────────────
    (
        "no square records a hit box",
        "                f.hit(Target::Square(row, col), r);",
        "                let _ = r;",
        ["every_square_is_clickable_at_every_window_size"],
    ),
    (
        "every square records the a1 hit box",
        "                f.hit(Target::Square(row, col), r);",
        "                f.hit(Target::Square(0, 0), r);",
        ["every_square_is_clickable_at_every_window_size"],
    ),
    (
        "a square is clickable where none of its ink is",
        "                let pos = Pos::new(row, col);\n                let r = l.square_rect(pos);",
        "                let pos = Pos::new(row, col);\n                let r = l.square_rect(pos);\n                f.hit(Target::Square(row, col), Rect::new(0.0, 0.0, 1.0, 1.0));",
        ["a_click_outside_the_board_reaches_nothing"],
    ),
    (
        "the button records no hit box",
        "        f.hit(Target::NewGame, r);",
        "        let _ = r;",
        ["the_new_game_button_is_drawn_and_restarts_the_game"],
    ),
    (
        "a click is read against the default size rather than the drawn one",
        "        let acted = match self\n            .frame(self.size.0, self.size.1)\n            .hit_test(event.x, event.y)",
        "        let acted = match self\n            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)\n            .hit_test(event.x, event.y)",
        ["a_click_reaches_the_square_it_landed_on_in_a_resized_window"],
    ),
    (
        "the frame forgets the size it was drawn at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["a_render_teaches_the_click_handler_the_size_it_drew_at"],
    ),
    (
        "a right click is taken for a move",
        "        let MouseEventKind::Press(MouseButton::Left) = event.kind else {",
        "        let MouseEventKind::Press(_) = event.kind else {",
        ["a_right_click_is_not_a_move"],
    ),
    (
        "a click that reached nothing is claimed anyway",
        "            None => false,\n        };\n        if acted {\n            EventResult::Consumed\n        } else {\n            EventResult::Ignored\n        }\n    }\n\n    /// Handle an event.",
        "            None => true,\n        };\n        if acted {\n            EventResult::Consumed\n        } else {\n            EventResult::Ignored\n        }\n    }\n\n    /// Handle an event.",
        ["a_click_outside_the_board_reaches_nothing"],
    ),
    # ── Selecting and moving ──────────────────────────────────────────────
    (
        "clicking any piece selects it, including Black's",
        "        if let Some(piece) = self.board.get(pos)\n            && piece.side == Side::White",
        "        if let Some(piece) = self.board.get(pos)\n            && piece.side != Side::White",
        ["clicking_a_square_selects_the_piece_standing_on_it"],
    ),
    (
        "a second click on the same piece is claimed as a change",
        "            return !already;",
        "            return true;",
        ["a_second_click_on_the_selected_piece_changes_nothing"],
    ),
    (
        "the board stays live while Black owes a reply",
        "        if self.game_result != GameResult::Ongoing || self.board.side_to_move != Side::White {",
        "        if self.game_result != GameResult::Ongoing {",
        ["the_board_is_not_the_players_to_touch_while_black_owes_a_reply"],
    ),
    (
        "the board stays live after the game has ended",
        "        if self.game_result != GameResult::Ongoing || self.board.side_to_move != Side::White {",
        "        if self.board.side_to_move != Side::White {",
        ["a_game_that_ended_on_whites_own_move_refuses_the_board_too"],
    ),
    # ── The search runs on the tick, not on the click ─────────────────────
    (
        "the click runs the search itself, as it used to",
        "            self.execute_move(mv);\n            return true;\n        }",
        "            self.execute_move(mv);\n            self.think();\n            return true;\n        }",
        ["the_click_that_plays_a_move_does_not_also_run_the_search"],
    ),
    (
        "the tick searches whether or not a reply is owed",
        "        if !self.thinking {\n            return false;\n        }",
        "        if false {\n            return false;\n        }",
        ["a_tick_with_nothing_to_think_about_is_not_consumed"],
    ),
    (
        "every tick claims to have thought",
        "            Event::Tick { .. } if self.think() => EventResult::Consumed,",
        "            Event::Tick { .. } => {\n                self.think();\n                EventResult::Consumed\n            }",
        ["a_tick_with_nothing_to_think_about_is_not_consumed"],
    ),
    (
        "a search with no legal reply leaves the status stuck",
        "        } else {\n            // No legal reply: the position is mate or stalemate, and it is\n            // `update_game_state` that says which. Reaching here without it\n            // having said so would leave the status stuck on \"Black is\n            // thinking\" forever.\n            self.update_game_state();\n        }",
        "        }",
        ["a_search_with_no_reply_still_settles_the_game"],
    ),
    (
        "Black is thought to owe a reply after the game has ended",
        "        self.thinking =\n            self.game_result == GameResult::Ongoing && self.board.side_to_move == Side::Black;",
        "        self.thinking = self.board.side_to_move == Side::Black;",
        ["a_finished_game_refuses_the_board_but_not_the_button"],
    ),
    (
        "White is thought to owe the reply instead of Black",
        "        self.thinking =\n            self.game_result == GameResult::Ongoing && self.board.side_to_move == Side::Black;",
        "        self.thinking =\n            self.game_result == GameResult::Ongoing && self.board.side_to_move == Side::White;",
        ["the_click_that_plays_a_move_does_not_also_run_the_search"],
    ),
    (
        "the window never says the search is running",
        "        if self.thinking {\n            self.status_message = \"Black is thinking\".to_string();\n        }",
        "",
        ["the_status_line_says_black_is_thinking_while_it_is"],
    ),
    # ── The keyboard ──────────────────────────────────────────────────────
    (
        "a key release moves the cursor",
        "        if !event.pressed {\n            return EventResult::Ignored;\n        }",
        "",
        ["a_key_release_moves_nothing"],
    ),
    (
        "Up walks toward White's back rank",
        "            Key::Up => self.step_cursor(1, 0),\n            Key::Down => self.step_cursor(-1, 0),",
        "            Key::Up => self.step_cursor(-1, 0),\n            Key::Down => self.step_cursor(1, 0),",
        ["the_cursor_walks_the_board_in_the_direction_the_key_names"],
    ),
    (
        "Left and Right are swapped",
        "            Key::Left => self.step_cursor(0, -1),\n            Key::Right => self.step_cursor(0, 1),",
        "            Key::Left => self.step_cursor(0, 1),\n            Key::Right => self.step_cursor(0, -1),",
        ["the_cursor_walks_the_board_in_the_direction_the_key_names"],
    ),
    (
        "the cursor runs off the edge of the board",
        "        let row = self.cursor.row.saturating_add(d_row).clamp(0, 7);\n        let col = self.cursor.col.saturating_add(d_col).clamp(0, 7);",
        "        let row = self.cursor.row.saturating_add(d_row);\n        let col = self.cursor.col.saturating_add(d_col);",
        ["a_cursor_against_the_edge_does_not_claim_the_key"],
    ),
    (
        "a cursor already against the edge claims the key",
        "        let moved = (row, col) != (self.cursor.row, self.cursor.col);",
        "        let moved = true;",
        ["a_cursor_against_the_edge_does_not_claim_the_key"],
    ),
    (
        "Enter does not play the square the cursor is on",
        "            Key::Enter | Key::Space => self.click_square(self.cursor),",
        "            Key::Enter | Key::Space => false,",
        ["enter_plays_the_square_the_cursor_is_on"],
    ),
    (
        "Escape claims the key with nothing to deselect",
        "            Key::Escape => self.deselect(),",
        "            Key::Escape => {\n                self.deselect();\n                true\n            }",
        ["escape_drops_a_selection_and_claims_nothing_when_there_is_none"],
    ),
    (
        "a bare N starts a new game",
        "            Key::N if event.modifiers.ctrl => {",
        "            Key::N => {",
        ["ctrl_n_starts_a_new_game_and_a_bare_n_does_not"],
    ),
    # ── New game ──────────────────────────────────────────────────────────
    (
        "a new game keeps the search it was in the middle of",
        "    fn new_game(&mut self) {\n        let size = self.size;\n        *self = Self::new();\n        self.size = size;\n    }",
        "    fn new_game(&mut self) {\n        let size = self.size;\n        let thinking = self.thinking;\n        *self = Self::new();\n        self.size = size;\n        self.thinking = thinking;\n    }",
        ["a_new_game_started_by_the_button_is_not_still_thinking"],
    ),
    (
        "a new game forgets the size of the window it is in",
        "        let size = self.size;\n        *self = Self::new();\n        self.size = size;",
        "        *self = Self::new();",
        ["a_restart_keeps_the_window_size"],
    ),
    (
        "the button does nothing",
        "            Some(Target::NewGame) => {\n                self.new_game();\n                true\n            }",
        "            Some(Target::NewGame) => false,",
        ["the_new_game_button_is_drawn_and_restarts_the_game"],
    ),
    # ── The panel ─────────────────────────────────────────────────────────
    (
        "the move list runs past the foot of the panel",
        "        let rows = ((floor - y) / (l.small * 1.3)).floor().max(0.0) as usize;",
        "        let rows = usize::MAX;",
        ["the_move_list_stops_at_the_foot_of_the_panel"],
    ),
    (
        "the move list shows the opening rather than the moves just played",
        "        for s in pairs.iter().skip(pairs.len().saturating_sub(rows)) {",
        "        for s in pairs.iter().take(rows) {",
        ["the_move_list_shows_the_moves_just_played_not_the_first_ones"],
    ),
    (
        "the captured lists are credited to the wrong side",
        "            .zip([&self.captured_black, &self.captured_white])",
        "            .zip([&self.captured_white, &self.captured_black])",
        ["a_capture_is_listed_under_the_side_that_took_it"],
    ),
    (
        "the labels are drawn with no room for them",
        "        if l.square <= 0.0 || l.margin < l.label {",
        "        if l.square <= 0.0 {",
        ["the_labels_are_dropped_rather_than_drawn_over_the_board"],
    ),
    (
        "the rank labels count from the wrong end",
        "            let Some(s) = Pos::new(rank, 0).rank_char().map(String::from) else {",
        "            let Some(s) = Pos::new(7 - rank, 0).rank_char().map(String::from) else {",
        ["the_rank_and_file_labels_name_the_ranks_and_files_they_sit_beside"],
    ),
    (
        "the file labels count from the wrong end",
        "            let Some(s) = Pos::new(0, file).file_char().map(String::from) else {",
        "            let Some(s) = Pos::new(0, 7 - file).file_char().map(String::from) else {",
        ["the_rank_and_file_labels_name_the_ranks_and_files_they_sit_beside"],
    ),
    # ── The names of squares ──────────────────────────────────────────────
    (
        "a file letter is read off the end of the alphabet",
        "        u8::try_from(self.col)\n            .ok()\n            .filter(|c| *c < 8)",
        "        u8::try_from(self.col).ok().filter(|_| true)",
        ["off_board_squares_have_no_name"],
    ),
    (
        "a rank digit is read off the end of the digits",
        "        u8::try_from(self.row)\n            .ok()\n            .filter(|r| *r < 8)",
        "        u8::try_from(self.row).ok().filter(|_| true)",
        ["off_board_squares_have_no_name"],
    ),
    (
        "the file and the rank are swapped in a square's name",
        "        match (self.file_char(), self.rank_char()) {\n            (Some(file), Some(rank)) => format!(\"{file}{rank}\"),",
        "        match (self.file_char(), self.rank_char()) {\n            (Some(file), Some(rank)) => format!(\"{rank}{file}\"),",
        ["test_pos_algebraic"],
    ),
    # ── The board's own accessors ─────────────────────────────────────────
    (
        # `pos.row as usize` is not a mutation of `usize::try_from(pos.row)`:
        # -1 casts to an enormous index that `slice::get` rejects for the same
        # reason `try_from` does, so the two agree everywhere. What *would*
        # differ is a row folded back onto the board rather than rejected --
        # which is what an `abs()` written for tidiness would do.
        "a square off the top of the board is folded onto one that is on it",
        "        let row = usize::try_from(pos.row).ok()?;\n        let col = usize::try_from(pos.col).ok()?;",
        "        let row = usize::from(pos.row.unsigned_abs());\n        let col = usize::from(pos.col.unsigned_abs());",
        ["test_board_get_out_of_bounds"],
    ),
    (
        "a piece set off the board lands on a square that is on it",
        "        if let Ok(row) = usize::try_from(pos.row)\n            && let Ok(col) = usize::try_from(pos.col)",
        "        if let Ok(row) = usize::try_from(pos.row.unsigned_abs())\n            && let Ok(col) = usize::try_from(pos.col.unsigned_abs())",
        ["test_board_set_out_of_bounds"],
    ),
    # ── The move generators ───────────────────────────────────────────────
    (
        "a ray does not stop at the edge of the board",
        "        std::iter::successors(self.offset(d_row, d_col), move |p| p.offset(d_row, d_col))",
        "        std::iter::successors(Some(self), move |p| Some(Pos::new(p.row + d_row, p.col + d_col)))\n            .skip(1)\n            .take(8)",
        ["test_rook_moves_empty_board"],
    ),
    (
        "a sliding piece passes through the piece in its way",
        "                    Some(p) => {\n                        // The first piece in the way ends the ray; it can be\n                        // captured only if it is not one of ours.\n                        if p.side != side {\n                            moves.push(Move::normal(pos, to));\n                        }\n                        break;\n                    }",
        "                    Some(p) => {\n                        if p.side != side {\n                            moves.push(Move::normal(pos, to));\n                        }\n                    }",
        ["test_rook_blocked_by_own_piece"],
    ),
    (
        "a knight may capture its own side",
        "        for (dr, dc) in KNIGHT_DELTAS {\n            let Some(to) = pos.offset(dr, dc) else {\n                continue;\n            };\n            if self.get(to).is_none_or(|p| p.side != side) {",
        "        for (dr, dc) in KNIGHT_DELTAS {\n            let Some(to) = pos.offset(dr, dc) else {\n                continue;\n            };\n            if true {",
        ["test_knight_blocked_by_own_pieces"],
    ),
    (
        "a pawn may push through a piece",
        "        if let Some(one) = pos.offset(dir, 0)\n            && self.get(one).is_none()",
        "        if let Some(one) = pos.offset(dir, 0)",
        ["test_pawn_blocked"],
    ),
    (
        "a pawn may double-push from any rank",
        "            if pos.row == start_rank\n                && let Some(two) = one.offset(dir, 0)",
        "            if let Some(two) = one.offset(dir, 0)",
        ["test_pawn_no_double_push_after_move"],
    ),
    (
        "a pawn promotes to a queen and nothing else",
        "                for kind in PROMOTION_KINDS {\n                    moves.push(Move::promotion(pos, to, kind));\n                }",
        "                moves.push(Move::promotion(pos, to, PieceKind::Queen));",
        ["test_pawn_promotion_moves_generated"],
    ),
    (
        "en passant is offered on any empty square",
        "                None if self.en_passant == Some(cap) => moves.push(Move::en_passant(pos, cap)),",
        "                None => moves.push(Move::en_passant(pos, cap)),",
        ["test_en_passant_capture"],
    ),
    (
        "the en passant square is set on the wrong side of the pawn",
        "            self.en_passant = mv.to.offset(diff.signum().saturating_neg(), 0);",
        "            self.en_passant = mv.to.offset(diff.signum(), 0);",
        ["test_en_passant_target_set"],
    ),
    # ── Attack detection ──────────────────────────────────────────────────
    (
        "a piece of either side is taken for an attacker",
        "                    .is_some_and(|piece| piece.side == attacker && kinds.contains(&piece.kind))\n            })\n        };\n        // The first piece along each ray",
        "                    .is_some_and(|piece| kinds.contains(&piece.kind))\n            })\n        };\n        // The first piece along each ray",
        ["test_square_attacked_by_knight", "test_square_attacked_by_king"],
    ),
    (
        "a blocked slider still attacks through the piece in the way",
        "                pos.ray(dr, dc)\n                    .find_map(|p| self.get(p))",
        "                pos.ray(dr, dc)\n                    .filter_map(|p| self.get(p))\n                    .last()",
        ["test_attack_blocked_by_piece"],
    ),
    (
        "pawns are thought to attack the way they came",
        "        let pawn_dir: i8 = if attacker == Side::White { -1 } else { 1 };",
        "        let pawn_dir: i8 = if attacker == Side::White { 1 } else { -1 };",
        ["test_square_attacked_by_pawn"],
    ),
    (
        "the king is not an attacker",
        "            || steps_to(&QUEEN_DIRS, &[PieceKind::King])",
        "            || false",
        ["test_square_attacked_by_king", "test_a_king_may_not_step_beside_the_other_king"],
    ),
    # ── The search ────────────────────────────────────────────────────────
    (
        "the evaluation credits both sides the same way",
        "            Side::Black => score.saturating_sub(total),",
        "            Side::Black => score.saturating_add(total),",
        # Not `test_evaluate_material_advantage`: the two kings cancel, so a
        # Black side credited the same way as White leaves that test's "White
        # is a queen up" still true by 40,000 points.
        ["test_evaluate_black_advantage"],
    ),
    (
        "the maximising search never looks past the first move",
        "            let score = minimax(&child, depth.saturating_sub(1), alpha, beta, false);",
        "            let score = minimax(&child, 0, alpha, beta, false);",
        ["test_minimax_finds_mate"],
    ),
    (
        "the leaf is scored from the mover's side rather than White's",
        "        return evaluate(board);",
        "        return if maximizing {\n            evaluate(board)\n        } else {\n            evaluate(board).saturating_neg()\n        };",
        ["the_search_scores_every_position_from_whites_side"],
    ),
    (
        "a mate in three is worth as much as a mate in one",
        "                KING_VALUE.saturating_add(depth).saturating_neg()\n            } else {\n                KING_VALUE.saturating_add(depth)",
        "                KING_VALUE.saturating_neg()\n            } else {\n                KING_VALUE",
        ["a_mate_delivered_sooner_scores_higher_than_the_same_mate_later"],
    ),
    (
        "an empty move list is indexed rather than asked",
        "    let mut best_move = *moves.first()?;",
        "    let mut best_move = Move::normal(Pos::new(0, 0), Pos::new(0, 0));",
        ["test_ai_no_moves_returns_none"],
    ),
    # ── The clocks ────────────────────────────────────────────────────────
    (
        "the halfmove clock never advances",
        "            self.halfmove_clock = self.halfmove_clock.saturating_add(1);",
        "",
        ["test_halfmove_clock_increments"],
    ),
    (
        "the fullmove number never advances",
        "            self.fullmove_number = self.fullmove_number.saturating_add(1);",
        "",
        ["test_fullmove_increments"],
    ),
]


if __name__ == "__main__":
    sweep(SRC, MUTATIONS, "chess", only=sys.argv[1:] or None)
