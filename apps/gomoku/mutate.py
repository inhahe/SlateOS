"""Mutation test for gomoku's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Gomoku is the thirty-seventh application in this campaign.  Its old suite was
large -- 117 tests -- and it was thorough about the rules and the search, but
it could not have noticed most of what was wrong with the program, because the
program had no window: `main` built a `GomokuApp`, dropped it and exited.  The
board was drawn from a `CELL_SIZE` constant and clicks were resolved by
`intersection_near`, a free function of the same constant, so the two agreed
with each other in every window and with the picture in exactly one.  Nothing
in the old file ever built an `Event`, so `handle_key` never having read the
`pressed` field -- which made every arrow step two intersections, leaving every
other row and column unreachable -- was invisible to all 117 of them.

Usage:  python -u apps/gomoku/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The layout ────────────────────────────────────────────────────────
    (
        "the board takes the width without checking the height",
        "        let side = free_w.min(free_h).max(0.0);",
        "        let side = free_w.max(0.0);",
        ["the_board_is_square_and_fits_the_window"],
    ),
    (
        "the cell is the old 36 px constant again",
        "        let cell = grid / (BOARD_SIZE - 1) as f32;",
        "        let cell = 36.0;",
        ["the_grid_is_evenly_spaced_inside_the_board"],
    ),
    (
        "the grid is divided into 15 gaps instead of 14",
        "        let cell = grid / (BOARD_SIZE - 1) as f32;",
        "        let cell = grid / BOARD_SIZE as f32;",
        ["the_grid_is_inset_from_the_edge_of_the_board"],
    ),
    (
        "an intersection reads the row for x and the column for y",
        "            self.origin.0 + col as f32 * self.cell,\n"
        "            self.origin.1 + row as f32 * self.cell,",
        "            self.origin.0 + row as f32 * self.cell,\n"
        "            self.origin.1 + col as f32 * self.cell,",
        ["the_grid_is_evenly_spaced_inside_the_board"],
    ),
    (
        "the grid starts at the edge of the wood with no margin for labels",
        "        let origin = (board.x + margin, board.y + margin);",
        "        let origin = (board.x, board.y);",
        ["the_grid_is_inset_from_the_edge_of_the_board"],
    ),
    (
        "a stone is exactly half a cell, so neighbours touch",
        "            stone: cell * 0.44,",
        "            stone: cell * 0.5,",
        ["stones_on_neighbouring_points_do_not_overlap"],
    ),
    (
        "a stone is a dot rather than a stone",
        "            stone: cell * 0.44,",
        "            stone: cell * 0.2,",
        ["stones_on_neighbouring_points_do_not_overlap"],
    ),
    (
        "the panel is kept however narrow the window is",
        "        let panel_w = if w - want_panel >= h * 0.45 && want_panel <= w * 0.4 {\n"
        "            want_panel\n"
        "        } else {\n"
        "            0.0\n"
        "        };",
        "        let panel_w = want_panel;",
        ["a_window_with_no_room_for_the_panel_drops_it_whole"],
    ),
    (
        "the panel is dropped however wide the window is",
        "        let panel_w = if w - want_panel >= h * 0.45 && want_panel <= w * 0.4 {\n"
        "            want_panel\n"
        "        } else {\n"
        "            0.0\n"
        "        };",
        "        let panel_w = 0.0f32;",
        ["a_window_with_no_room_for_the_panel_drops_it_whole"],
    ),
    (
        "the status band starts at the bottom edge instead of ending there",
        "        let status = Rect::new(0.0, (h - status_h).max(0.0), w, status_h.min(h));",
        "        let status = Rect::new(0.0, h, w, status_h);",
        ["the_bands_stack_without_overlapping"],
    ),
    (
        "the board is placed as if there were no header above it",
        "            header.bottom() + ((free_h - side) / 2.0).max(0.0),",
        "            ((free_h - side) / 2.0).max(0.0),",
        ["the_bands_stack_without_overlapping"],
    ),
    (
        "the panel is placed on top of the board",
        "        let panel = Rect::new(free_w, header.bottom(), panel_w, free_h);",
        "        let panel = Rect::new(0.0, header.bottom(), panel_w, free_h);",
        ["the_bands_stack_without_overlapping"],
    ),
    # ── The window ────────────────────────────────────────────────────────
    (
        "the background is painted at the size the program was written for",
        "        fill(f, l.window, BASE, CornerRadii::all(0.0));",
        "        fill(f, Rect::new(0.0, 0.0, 800.0, 600.0), BASE, CornerRadii::all(0.0));",
        ["the_background_covers_the_window_at_every_size"],
    ),
    # ── The board and the rules ───────────────────────────────────────────
    (
        "reading off the board wraps round instead of saying so",
        "        let row = usize::try_from(row).ok()?;\n"
        "        let col = usize::try_from(col).ok()?;\n"
        "        self.cells.get(row)?.get(col).copied()",
        "        let row = usize::try_from(row).ok()? % BOARD_SIZE;\n"
        "        let col = usize::try_from(col).ok()? % BOARD_SIZE;\n"
        "        self.cells.get(row)?.get(col).copied()",
        ["reading_off_the_board_says_so"],
    ),
    # Not the same break as the one above, and `a_run_does_not_wrap_around_the
    # _edge` is the test that tells them apart.  Wrapping each coordinate
    # separately sends column 15 to column 0 of the *same* row, which no run
    # of five can exploit; reading the board as a flat array sends it to
    # column 0 of the *next* row, which is exactly the false win that test was
    # written for.
    (
        "the board is read as a flat array, so a run crosses onto the next row",
        "        let row = usize::try_from(row).ok()?;\n"
        "        let col = usize::try_from(col).ok()?;\n"
        "        self.cells.get(row)?.get(col).copied()",
        "        let row = usize::try_from(row).ok()?;\n"
        "        let col = usize::try_from(col).ok()?;\n"
        "        let i = row.checked_mul(BOARD_SIZE)?.checked_add(col)?;\n"
        "        self.cells\n"
        "            .get(i / BOARD_SIZE)?\n"
        "            .get(i % BOARD_SIZE)\n"
        "            .copied()",
        ["a_run_does_not_wrap_around_the_edge"],
    ),
    (
        "a write off the board reports success",
        "        let Some(slot) = self.cells.get_mut(row).and_then(|r| r.get_mut(col)) else {\n"
        "            return false;\n"
        "        };",
        "        let Some(slot) = self.cells.get_mut(row).and_then(|r| r.get_mut(col)) else {\n"
        "            return true;\n"
        "        };",
        ["writing_off_the_board_is_refused_rather_than_done_somewhere_else"],
    ),
    (
        "a point that is not on the board counts as an empty one",
        "        let (Ok(r), Ok(c)) = (i32::try_from(row), i32::try_from(col)) else {\n"
        "            return false;\n"
        "        };\n"
        "        self.get(r, c) == Some(Cell::Empty)",
        "        let (Ok(r), Ok(c)) = (i32::try_from(row), i32::try_from(col)) else {\n"
        "            return false;\n"
        "        };\n"
        "        self.get(r, c) != Some(Cell::Black) && self.get(r, c) != Some(Cell::White)",
        ["an_intersection_off_the_board_is_not_empty"],
    ),
    (
        "four in a row wins",
        "        for i in 0..WIN_COUNT as i32 {",
        "        for i in 0..4 {",
        ["five_in_a_row_wins_in_every_direction"],
    ),
    (
        "a line of five is walked in one direction only",
        "            let r = row.saturating_add(dr.saturating_mul(i));\n"
        "            let c = col.saturating_add(dc.saturating_mul(i));",
        "            let r = row.saturating_add(dr.saturating_mul(i));\n"
        "            let c = col;",
        ["five_in_a_row_wins_in_every_direction"],
    ),
    (
        "a board one stone short of full is a draw",
        "        self.stone_count() == BOARD_SIZE * BOARD_SIZE",
        "        self.stone_count() >= BOARD_SIZE * BOARD_SIZE - 1",
        ["a_full_board_is_a_draw_and_a_nearly_full_one_is_not"],
    ),
    (
        "an empty point has an opponent",
        "            Cell::Empty => Cell::Empty,",
        "            Cell::Empty => Cell::Black,",
        ["a_colour_has_an_opponent_and_an_empty_point_does_not"],
    ),
    (
        "the win line names the wrong stones",
        "                        return Some(WinLine { positions });",
        "                        return Some(WinLine {\n"
        "                            positions: positions.into_iter().rev().collect(),\n"
        "                        });",
        ["a_win_names_the_five_stones_that_made_it"],
    ),
    # ── Placing a stone ───────────────────────────────────────────────────
    (
        "a stone can be dropped on top of another",
        "        if !self.board.is_empty(row, col) {\n            return false;\n        }\n\n",
        "",
        ["a_stone_cannot_be_placed_on_a_stone"],
    ),
    (
        "the game accepts moves after it is over",
        "        if self.phase != GamePhase::Playing {\n            return false;\n        }\n\n"
        "        let row = self.cursor_row as usize;",
        "        let row = self.cursor_row as usize;",
        ["nothing_can_be_placed_after_the_game_is_over"],
    ),
    (
        "the turn never changes hands",
        "        self.current_turn = stone.opponent();",
        "        self.current_turn = stone;",
        ["blacks_move_leaves_white_thinking_rather_than_answered"],
    ),
    (
        "Black's move goes straight back to Playing, so White never thinks",
        "        self.phase = if self.current_turn == Cell::White {\n"
        "            GamePhase::Thinking\n"
        "        } else {\n"
        "            GamePhase::Playing\n"
        "        };",
        "        self.phase = GamePhase::Playing;",
        ["blacks_move_leaves_white_thinking_rather_than_answered"],
    ),
    (
        "White's win is credited to Black, as the old duplicate did",
        "                Cell::White => self.scores.1 = self.scores.1.saturating_add(1),",
        "                Cell::White => self.scores.0 = self.scores.0.saturating_add(1),",
        ["a_win_is_credited_to_the_colour_that_won"],
    ),
    (
        "a draw scores nothing",
        "            self.scores.2 = self.scores.2.saturating_add(1);",
        "",
        ["a_board_that_fills_without_a_five_is_a_draw"],
    ),
    (
        "the last stone played is not remembered",
        "        self.last_move = Some((row, col));",
        "        self.last_move = None;",
        ["only_the_last_stone_played_is_marked"],
    ),
    (
        "the winning line is not remembered",
        "            self.win_line = Some(win_line);",
        "            self.win_line = None;",
        ["the_win_is_marked_on_the_five_stones_that_made_it"],
    ),
    (
        "a new game keeps the board it just finished",
        "        self.board = Board::new();\n        self.phase = GamePhase::Playing;",
        "        self.phase = GamePhase::Playing;",
        ["a_new_game_clears_the_board_and_keeps_the_scores"],
    ),
    (
        "a new game clears the scores as well as the board",
        "        self.last_move = None;\n    }",
        "        self.last_move = None;\n        self.scores = (0, 0, 0);\n    }",
        [
            "a_new_game_clears_the_board_and_keeps_the_scores",
            "the_buttons_answer_the_pointer_after_the_game_is_over",
        ],
    ),
    # ── The tick that makes White move ────────────────────────────────────
    (
        "the tick is not handled, so White never answers",
        "            Event::Tick { .. } if self.think() => EventResult::Consumed,\n",
        "",
        ["the_tick_is_what_makes_white_move"],
    ),
    (
        # The guard used to be written twice -- here and in the `Event::Tick`
        # arm that calls this -- so deleting this copy changed nothing and the
        # mutant survived the whole suite.  A condition stated twice has one
        # copy that cannot be reached, and an unreachable guard is one no test
        # can be written for.  `think` now returns whether it ran and the tick
        # arm is answered by that, so there is one guard and it is live.
        "the tick runs the search whatever the game is doing",
        "        if self.phase != GamePhase::Thinking {\n            return false;\n        }\n",
        "",
        ["a_tick_with_nothing_to_think_about_is_ignored"],
    ),
    (
        "the window is never asked for a tick",
        "        Some(Duration::from_millis(60))",
        "        None",
        ["the_app_asks_for_the_tick_that_makes_white_move"],
    ),
    (
        "White answers inside Black's move again, as it used to",
        "        self.place_stone(row, col);\n        true",
        "        self.place_stone(row, col);\n        self.think();\n        true",
        ["blacks_move_leaves_white_thinking_rather_than_answered"],
    ),
    # ── Undo ──────────────────────────────────────────────────────────────
    (
        "undo takes back White's reply and leaves Black's move",
        "        self.take_back(Cell::White);\n        self.take_back(Cell::Black);",
        "        self.take_back(Cell::White);",
        ["undo_takes_back_the_pair_not_just_the_reply"],
    ),
    (
        "take_back lifts whatever stone is on top",
        "        if self.move_history.last().map(|m| m.stone) != Some(stone) {\n"
        "            return;\n"
        "        }\n",
        "",
        ["undoing_blacks_win_does_not_also_take_back_whites_last_reply"],
    ),
    (
        "undo keeps the point the finished game awarded",
        "            match self.winner {\n"
        "                Cell::Black => self.scores.0 = self.scores.0.saturating_sub(1),\n"
        "                Cell::White => self.scores.1 = self.scores.1.saturating_sub(1),\n"
        "                Cell::Empty => self.scores.2 = self.scores.2.saturating_sub(1),\n"
        "            }\n",
        "",
        ["undoing_a_win_takes_back_the_point_it_scored"],
    ),
    (
        "undoing a draw takes the point off Black instead",
        "                Cell::Empty => self.scores.2 = self.scores.2.saturating_sub(1),",
        "                Cell::Empty => self.scores.0 = self.scores.0.saturating_sub(1),",
        ["undoing_a_draw_takes_back_the_point_it_scored"],
    ),
    (
        "undo leaves the game in the phase it ended in",
        "            self.phase = GamePhase::Playing;\n            self.win_line = None;",
        "            self.win_line = None;",
        ["undoing_a_win_takes_back_the_point_it_scored"],
    ),
    (
        "undo does not hand the turn back to Black",
        "        self.current_turn = Cell::Black;\n"
        "        self.last_move = self.move_history.last().map(|m| (m.row, m.col));",
        "        self.last_move = self.move_history.last().map(|m| (m.row, m.col));",
        ["undo_while_white_is_thinking_gives_the_move_back"],
    ),
    # ── The keyboard ──────────────────────────────────────────────────────
    (
        "a key release does its work a second time",
        "        if !event.pressed {\n            return EventResult::Ignored;\n        }\n",
        "",
        ["a_key_acts_on_the_press_and_not_again_on_the_release"],
    ),
    (
        "the up arrow steps two intersections",
        "                self.cursor_row = self.cursor_row.saturating_sub(1);",
        "                self.cursor_row = self.cursor_row.saturating_sub(2);",
        [
            "a_key_acts_on_the_press_and_not_again_on_the_release",
            "the_cursor_is_drawn_where_the_arrows_left_it",
            "the_cursor_stops_at_the_edges",
        ],
    ),
    (
        # Both row arrows stepping two is the fault the *reachability* test
        # names, and the one the unwired program actually had: the cursor
        # oscillates 7 -> 5 -> 7 around an odd row and never lands on it, so
        # every other row is unreachable from the keyboard.  Without this row
        # no mutation would have proved that test owns anything.
        "both row arrows step two, so odd rows cannot be reached",
        "                self.cursor_row = self.cursor_row.saturating_sub(1);\n"
        "            }\n"
        "            KeyEvent { key: Key::Down, .. } if self.cursor_row < LAST_INDEX => {\n"
        "                self.cursor_row = self.cursor_row.saturating_add(1);",
        "                self.cursor_row = self.cursor_row.saturating_sub(2);\n"
        "            }\n"
        "            KeyEvent { key: Key::Down, .. } if self.cursor_row < LAST_INDEX => {\n"
        "                self.cursor_row = self.cursor_row.saturating_add(2);",
        ["the_arrows_can_reach_every_intersection"],
    ),
    (
        "the down arrow steps off the last row",
        "            KeyEvent { key: Key::Down, .. } if self.cursor_row < LAST_INDEX => {",
        "            KeyEvent { key: Key::Down, .. } if self.cursor_row <= LAST_INDEX => {",
        ["the_cursor_stops_at_the_edges"],
    ),
    (
        "space no longer places a stone",
        "            }\n            | KeyEvent {\n                key: Key::Space, ..\n            } => {",
        "            } => {",
        ["space_places_a_stone_as_enter_does"],
    ),
    (
        "a key the game does nothing with is swallowed",
        "            _ => return EventResult::Ignored,",
        "            _ => {}",
        ["a_key_that_changes_nothing_is_ignored"],
    ),
    # ── The pointer ───────────────────────────────────────────────────────
    (
        "any mouse button plays a stone",
        "        let MouseEventKind::Press(MouseButton::Left) = event.kind else {",
        "        let MouseEventKind::Press(_) = event.kind else {",
        ["the_right_button_does_not_place_a_stone"],
    ),
    (
        "a click is read against the size the program was written for",
        "        let frame = self.frame(self.width, self.height);\n"
        "        match frame.hit_test(event.x, event.y) {",
        "        let frame = self.frame(WINDOW_WIDTH, WINDOW_HEIGHT);\n"
        "        match frame.hit_test(event.x, event.y) {",
        ["a_click_is_read_against_the_window_that_was_drawn"],
    ),
    (
        "a click plays even while White is searching",
        "                if self.phase != GamePhase::Playing || self.current_turn != Cell::Black {\n"
        "                    return EventResult::Ignored;\n"
        "                }\n",
        "",
        ["a_click_during_whites_search_is_refused"],
    ),
    (
        "the pointer is refused once the game is over, as it used to be",
        "        let frame = self.frame(self.width, self.height);\n"
        "        match frame.hit_test(event.x, event.y) {",
        "        if self.phase != GamePhase::Playing {\n"
        "            return EventResult::Ignored;\n"
        "        }\n"
        "        let frame = self.frame(self.width, self.height);\n"
        "        match frame.hit_test(event.x, event.y) {",
        ["the_buttons_answer_the_pointer_after_the_game_is_over"],
    ),
    (
        "a click does not move the cursor to the point it hit",
        "                self.cursor_row = i32::try_from(row).unwrap_or(self.cursor_row);\n"
        "                self.cursor_col = i32::try_from(col).unwrap_or(self.cursor_col);\n",
        "",
        ["a_click_lands_on_the_intersection_it_was_aimed_at"],
    ),
    (
        "a click on the background is swallowed",
        "            None => EventResult::Ignored,",
        "            None => EventResult::Consumed,",
        ["a_click_on_the_background_does_nothing"],
    ),
    (
        "no intersection records a hit box",
        "                f.hit(Target::Point(row, col), rect);",
        "",
        ["every_intersection_is_clickable_where_it_is_drawn"],
    ),
    (
        "the hit box is a whole cell, so the wood between the lines is live",
        "        let (x, y) = self.intersection(row, col);\n"
        "        Rect::new(\n"
        "            x - self.stone,\n"
        "            y - self.stone,\n"
        "            self.stone * 2.0,\n"
        "            self.stone * 2.0,\n"
        "        )",
        "        let (x, y) = self.intersection(row, col);\n"
        "        Rect::new(\n"
        "            x - self.cell / 2.0,\n"
        "            y - self.cell / 2.0,\n"
        "            self.cell,\n"
        "            self.cell,\n"
        "        )",
        ["a_click_between_the_lines_places_nothing"],
    ),
    # ── The paint ─────────────────────────────────────────────────────────
    (
        "the stone colours are swapped",
        "                    Cell::Black => (BLACK_STONE, BLACK_STONE_BORDER),\n"
        "                    Cell::White => (WHITE_STONE, WHITE_STONE_BORDER),",
        "                    Cell::Black => (WHITE_STONE, WHITE_STONE_BORDER),\n"
        "                    Cell::White => (BLACK_STONE, BLACK_STONE_BORDER),",
        ["every_stone_is_painted_on_the_point_it_sits_on"],
    ),
    (
        "a stone has no edge, only a body",
        "                    color: border,",
        "                    color: body,",
        ["a_stone_is_drawn_with_an_edge"],
    ),
    (
        "every stone is marked as the last one played",
        "                if self.last_move == Some((row, col)) {",
        "                if self.last_move.is_some() {",
        ["only_the_last_stone_played_is_marked"],
    ),
    (
        "the cursor is drawn whatever the game is doing",
        "        if self.phase != GamePhase::Playing || self.current_turn != Cell::Black {\n"
        "            return;\n"
        "        }\n"
        "        let rect = l.stone_rect(self.cursor_row, self.cursor_col);",
        "        let rect = l.stone_rect(self.cursor_row, self.cursor_col);",
        ["the_cursor_is_hidden_when_there_is_no_move_to_make"],
    ),
    (
        "the cursor is drawn on the centre point wherever it really is",
        "        let rect = l.stone_rect(self.cursor_row, self.cursor_col);",
        "        let rect = l.stone_rect(7, 7);",
        ["the_cursor_is_drawn_where_the_arrows_left_it"],
    ),
    (
        "the status band never says White is thinking",
        '            GamePhase::Thinking => ("White is thinking...", LAVENDER),\n'
        "            GamePhase::Playing => (",
        "            GamePhase::Thinking | GamePhase::Playing => (",
        [
            "the_status_band_says_what_the_game_is_doing",
            "the_frame_after_blacks_move_says_white_is_thinking",
        ],
    ),
    (
        "the header names the wrong colour to play",
        '            GamePhase::Playing if self.current_turn == Cell::Black => ("Black to play", BLUE),',
        '            GamePhase::Playing if self.current_turn == Cell::Black => ("White to play", BLUE),',
        ["the_header_names_the_turn"],
    ),
    (
        "the panel counts the pairs instead of the stones",
        '            format!("Moves: {}", self.move_count),',
        '            format!("Moves: {}", self.move_count / 2),',
        ["the_panel_counts_the_moves_and_the_scores"],
    ),
    (
        "the panel reads the scores off by one column",
        '            (format!("\\u{25CF} Black: {}", self.scores.0), TEXT_COLOR),',
        '            (format!("\\u{25CF} Black: {}", self.scores.1), TEXT_COLOR),',
        ["the_panel_counts_the_moves_and_the_scores"],
    ),
    (
        "Undo is clickable with nothing to undo",
        '            ("Undo (Z)", Target::Undo, !self.move_history.is_empty()),',
        '            ("Undo (Z)", Target::Undo, true),',
        ["undo_is_not_clickable_with_nothing_to_undo"],
    ),
    (
        "a button is drawn without recording where it was drawn",
        "            if enabled {\n                f.hit(target, r);\n            }\n",
        "",
        ["each_button_is_labelled_where_it_is_clickable"],
    ),
    (
        "the button label is drawn away from the button",
        "                r.x + (r.w - tw) / 2.0,\n                r.y + (r.h - l.small) / 2.0,",
        "                l.board.x,\n                l.board.y,",
        ["each_button_is_labelled_where_it_is_clickable"],
    ),
    (
        "the panel is painted even when the layout dropped it",
        "        if l.panel.is_empty() {\n            return;\n        }\n"
        "        fill(f, l.panel, MANTLE, CornerRadii::all(0.0));",
        "        fill(f, l.panel, MANTLE, CornerRadii::all(0.0));",
        ["nothing_is_painted_outside_the_window"],
    ),
    (
        "the coordinate labels are drawn into a margin too small for them",
        "        let gap = l.origin.1 - l.board.y;\n"
        "        if gap < l.label * 1.1 {\n            return;\n        }\n",
        "        let gap = l.origin.1 - l.board.y;\n",
        ["the_coordinate_labels_are_dropped_when_they_do_not_fit"],
    ),
    (
        "a coordinate label is drawn from the line rather than centred on it",
        "                    x - half_l,",
        "                    x,",
        ["a_coordinate_label_is_centred_on_its_line"],
    ),
    # ── The window the OS opens ───────────────────────────────────────────
    (
        "render forgets the size the frame was drawn at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_remembers_the_size_the_click_will_be_read_against"],
    ),
    (
        "render hands the window an empty frame",
        "        self.frame(width, height).into_tree()",
        "        Frame::<Target>::new(width, height).into_tree()",
        ["the_tree_the_window_gets_is_the_frame_that_was_drawn"],
    ),
    (
        "the program calls itself two different things",
        '        String::from("gomoku")',
        '        String::from("Gomoku game")',
        ["the_window_names_itself"],
    ),
]


if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "gomoku", timeout=300))
