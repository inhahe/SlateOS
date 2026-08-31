"""Mutation test for dots' suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Dots & Boxes is the fortieth application in this campaign.  Its old suite ran
to 200 tests and knew the rules of the game thoroughly -- drawing lines,
completing boxes, the extra turn, the AI's three phases, the win condition and
the seeding all had tests and all of them passed.  None of it could see that
`main` was `let _app = DotsAndBoxes::new();`: the program built a board,
dropped it and exited.  Nothing was ever displayed.

Around that sat the usual fixed-window damage, and one arrangement peculiar to
this app: `render` took no width and no height, and the two functions that
*did* mention the window -- `window_width()` and `window_height()` -- computed
it from the board rather than reading it.  The drawing pass told the window how
big to be.  The footer was painted at `window_height() - FOOTER_HEIGHT`, the
bottom of a window the app had decided on rather than the one it was given; the
scores went at `win_width - 200.0` and the turn at `win_width - 100.0`, a width
used as a coordinate with an offset that suited one set of words; a claimed
box's initial was centred by subtracting a literal 5 and 8; and the end-of-game
card was a fixed 260x140 with its four lines at fixed offsets inside it.

There was no mouse handler worth the name either -- clicks were resolved by
`point_to_segment_distance` against a 12px threshold, an inverse of the drawing
mapping written separately from it -- and the footer was one line of text
naming six keys, a label describing keystrokes where buttons would do.

Twelve crate-level `#![allow]`s hid the 82 findings that would have named most
of the rest.

Usage:  python -u apps/dots/mutate.py [substring ...]
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
        "the layout ignores the window it was given",
        "    fn solve(w: f32, h: f32, grid_size: usize) -> Self {\n        let w = w.max(0.0);\n        let h = h.max(0.0);",
        "    fn solve(w: f32, h: f32, grid_size: usize) -> Self {\n        let _ = (w, h);\n        let w = WINDOW_WIDTH;\n        let h = WINDOW_HEIGHT;",
        ["the_layout_follows_the_window_rather_than_a_constant"],
    ),
    (
        "the type size is a constant rather than a share of the height",
        "        let font = (h / 34.0).clamp(9.0, 18.0);",
        "        let font: f32 = 14.0;",
        ["the_layout_follows_the_window_rather_than_a_constant"],
    ),
    (
        "the board is sized by the width alone",
        "        let span = free.w.min(free.h);",
        "        let span = free.w;",
        # Not the squareness test: a board sized by the width alone is still
        # square, only far too big.  What sees it is the window it overflows.
        ["every_part_of_the_layout_stays_inside_the_window"],
    ),
    (
        "the dots are allowed to grow without limit",
        "        let spacing = (span / DOT_FRACTION.mul_add(2.0, cells)).clamp(0.0, MAX_SPACING);",
        "        let spacing = (span / DOT_FRACTION.mul_add(2.0, cells)).max(0.0);",
        ["the_dots_stop_growing_before_they_become_saucers"],
    ),
    (
        "the board is left-aligned instead of centred",
        "            free.x + (free.w - board_span).max(0.0) / 2.0,\n            free.y + (free.h - board_span).max(0.0) / 2.0,",
        "            free.x,\n            free.y,",
        ["the_board_is_centred_in_what_the_two_bars_leave"],
    ),
    (
        "the footer is placed at the bottom of a window of its own choosing",
        "        let footer = Rect::new(0.0, h - footer_h, w, footer_h);",
        "        let footer = Rect::new(0.0, WINDOW_HEIGHT - footer_h, w, footer_h);",
        ["every_part_of_the_layout_stays_inside_the_window"],
    ),
    (
        "the free band ignores the header and starts at the top of the window",
        "        let free = Rect::new(\n            pad,\n            header.bottom() + pad,",
        "        let free = Rect::new(\n            pad,\n            pad,",
        ["the_header_the_board_and_the_footer_do_not_overlap"],
    ),
    (
        "the board square does not leave room for the discs at its edges",
        "        let board_span = dot_radius.mul_add(2.0, cells * spacing);",
        "        let board_span = cells * spacing;",
        ["the_dots_are_an_even_lattice_that_fills_the_board_square"],
    ),
    # ── The lattice ───────────────────────────────────────────────────────
    (
        "the lattice is laid out from the window rather than from the board",
        "                .mul_add(1.0, self.board.x + col as f32 * self.spacing),",
        "                .mul_add(1.0, self.window.x + col as f32 * self.spacing),",
        ["the_dots_are_an_even_lattice_that_fills_the_board_square"],
    ),
    (
        "the lattice is transposed: rows are read as columns",
        "            self.dot_radius\n                .mul_add(1.0, self.board.x + col as f32 * self.spacing),\n            self.dot_radius\n                .mul_add(1.0, self.board.y + row as f32 * self.spacing),",
        "            self.dot_radius\n                .mul_add(1.0, self.board.x + row as f32 * self.spacing),\n            self.dot_radius\n                .mul_add(1.0, self.board.y + col as f32 * self.spacing),",
        ["every_line_is_painted_between_the_two_dots_it_joins"],
    ),
    (
        "a vertical line is drawn to the dot on its right instead of the one below",
        "            Orientation::Vertical => (\n                start.0,\n                self.dot_pos(line.row.saturating_add(1), line.col).1,\n            ),",
        "            Orientation::Vertical => (\n                self.dot_pos(line.row, line.col.saturating_add(1)).0,\n                start.1,\n            ),",
        ["every_line_is_painted_between_the_two_dots_it_joins"],
    ),
    # ── The hit boxes ─────────────────────────────────────────────────────
    (
        "a line's hit box is not inset, so the bands overlap at every dot",
        # Both arms: dropping the inset from one alone leaves the bands
        # *touching* at the shared dot rather than overlapping, and a
        # zero-area intersection is not a contested pixel.
        "            Orientation::Horizontal => {\n"
        "                Rect::new(x1 + r, y1 - r, (x2 - x1 - r * 2.0).max(0.0), r * 2.0)\n"
        "            }\n"
        "            Orientation::Vertical => {\n"
        "                Rect::new(x1 - r, y1 + r, r * 2.0, (y2 - y1 - r * 2.0).max(0.0))\n"
        "            }",
        "            Orientation::Horizontal => Rect::new(x1, y1 - r, x2 - x1, r * 2.0),\n"
        "            Orientation::Vertical => Rect::new(x1 - r, y1, r * 2.0, y2 - y1),",
        ["no_two_lines_claim_the_same_pixel"],
    ),
    (
        "the hit box is shifted one reach along the line it belongs to",
        "                Rect::new(x1 + r, y1 - r, (x2 - x1 - r * 2.0).max(0.0), r * 2.0)",
        "                Rect::new(x1 + r * 3.0, y1 - r, (x2 - x1 - r * 2.0).max(0.0), r * 2.0)",
        ["every_undrawn_line_has_a_hit_box_and_it_is_on_the_line"],
    ),
    (
        "the hit boxes are recorded for one grid and the lines painted for the other",
        "            if !drawn && self.accepts_moves() {\n                f.hit(Target::Line(line), l.line_box(line));",
        "            if !drawn && self.accepts_moves() {\n                f.hit(\n                    Target::Line(line),\n                    l.line_box(LineId::new(\n                        line.orientation.toggled(),\n                        line.row,\n                        line.col,\n                    )),\n                );",
        ["every_undrawn_line_has_a_hit_box_and_it_is_on_the_line"],
    ),
    (
        "a line already drawn keeps its hit box",
        "            if !drawn && self.accepts_moves() {",
        "            if self.accepts_moves() {",
        ["a_line_already_drawn_stops_offering_a_hit_box"],
    ),
    (
        "the board answers the pointer while the AI is thinking",
        "            if !drawn && self.accepts_moves() {",
        "            if !drawn {",
        ["the_board_stops_answering_the_pointer_while_the_ai_thinks"],
    ),
    (
        "the click is resolved against the window the app opened at",
        "        let (w, h) = self.size;\n        match self.frame(w, h).hit_test(me.x, me.y) {",
        "        let (w, h) = (WINDOW_WIDTH, WINDOW_HEIGHT);\n        match self.frame(w, h).hit_test(me.x, me.y) {",
        ["the_click_resolves_against_the_window_the_player_can_see"],
    ),
    (
        "a new game forgets the window the player resized to",
        "        *self = Self::with_config(grid_size, mode, seed);\n        self.size = window;",
        "        let _ = window;\n        *self = Self::with_config(grid_size, mode, seed);",
        ["a_new_game_keeps_the_window_the_player_resized_to"],
    ),
    # ── The buttons ───────────────────────────────────────────────────────
    (
        "a button that does not fit is drawn off the edge rather than left out",
        "            if x + w > l.footer.right() - l.pad {\n                break;\n            }",
        "            {}",
        ["a_footer_too_narrow_for_a_button_leaves_it_out_rather_than_off_the_edge"],
    ),
    (
        "the size buttons are all labelled for the size the board already is",
        "            buttons.push((format!(\"{n}x{n}\"), Target::Size(n), n == self.grid_size()));",
        "            buttons.push((\n                format!(\"{n}x{n}\"),\n                Target::Size(self.grid_size()),\n                n == self.grid_size(),\n            ));",
        ["the_size_buttons_set_the_size_they_are_labelled_with"],
    ),
    (
        "the mode button keeps the board it was pressed on",
        "                self.mode = match self.mode {\n                    GameMode::VsAi => GameMode::TwoPlayer,\n                    GameMode::TwoPlayer => GameMode::VsAi,\n                };\n                self.new_game();",
        "                self.mode = match self.mode {\n                    GameMode::VsAi => GameMode::TwoPlayer,\n                    GameMode::TwoPlayer => GameMode::VsAi,\n                };",
        ["the_mode_button_swaps_the_opponent_and_starts_over"],
    ),
    (
        "a key and its button do different things",
        "            Key::M => self.activate(Target::Mode),",
        "            Key::M => {\n                self.mode = GameMode::VsAi;\n                EventResult::Consumed\n            }",
        ["the_buttons_and_the_keys_do_the_same_thing"],
    ),
    # ── The board ─────────────────────────────────────────────────────────
    (
        "a box is complete one side early",
        "        self.box_side_count(box_row, box_col) == SIDES_PER_BOX",
        "        self.box_side_count(box_row, box_col) >= SIDES_PER_BOX - 1",
        ["a_box_is_complete_when_and_only_when_it_has_four_sides"],
    ),
    (
        "a box's right-hand side is read from the column it starts at",
        "            self.drawn(Orientation::Vertical, box_row, box_col.saturating_add(1)),",
        "            self.drawn(Orientation::Vertical, box_row, box_col),",
        ["a_box_is_complete_when_and_only_when_it_has_four_sides"],
    ),
    (
        "a box off the board reports the sides of the one at the origin",
        "        if box_row >= bps || box_col >= bps {\n            return 0;\n        }",
        "        let _ = bps;",
        ["a_box_off_the_board_has_no_sides_and_is_never_complete"],
    ),
    (
        "a horizontal line borders only the box below it",
        "                if line.row > 0 {\n                    let br = line.row.saturating_sub(1);",
        "                if false {\n                    let br = line.row.saturating_sub(1);",
        [
            "a_line_borders_the_boxes_on_either_side_of_it_and_no_others",
            "one_line_can_complete_two_boxes_at_once",
        ],
    ),
    (
        "an off-board line is drawn anyway",
        "        self.is_valid_line(line) && !self.is_line_drawn(line)",
        "        !self.is_line_drawn(line)",
        ["a_line_is_drawn_once_and_only_the_lines_that_exist_are"],
    ),
    (
        "a line already drawn is drawn again",
        "        self.is_valid_line(line) && !self.is_line_drawn(line)",
        "        self.is_valid_line(line)",
        ["a_line_is_drawn_once_and_only_the_lines_that_exist_are"],
    ),
    (
        "the vertical grid is given the horizontal grid's shape",
        "            v_lines: vec![vec![false; grid_size]; boxes_per_side],",
        "            v_lines: vec![vec![false; boxes_per_side]; grid_size],",
        ["a_board_has_the_lines_and_boxes_its_grid_size_calls_for"],
    ),
    # ── Turns ─────────────────────────────────────────────────────────────
    (
        "completing a box hands the turn over anyway",
        "        if completed == 0 {\n            self.current_player = self.current_player.other();",
        "        if completed >= 0 {\n            self.current_player = self.current_player.other();",
        ["a_move_that_completes_a_box_keeps_the_turn"],
    ),
    (
        "a move that completes nothing keeps the turn",
        "        if completed == 0 {\n            self.current_player = self.current_player.other();",
        "        if completed != 0 {\n            self.current_player = self.current_player.other();",
        ["a_move_that_completes_nothing_passes_the_turn"],
    ),
    (
        "the board takes moves after the game is over",
        "        self.phase == GamePhase::Playing && !self.ai_pending",
        "        !self.ai_pending",
        ["the_board_takes_no_moves_once_the_game_is_over"],
    ),
    (
        "the guard that freezes the board while the AI thinks is inverted",
        "        self.phase == GamePhase::Playing && !self.ai_pending",
        "        self.phase == GamePhase::Playing",
        ["the_cursor_does_not_move_when_the_board_is_not_taking_moves"],
    ),
    (
        "the winner is whoever has the fewest boxes",
        "            std::cmp::Ordering::Greater => Some(Player::One),\n            std::cmp::Ordering::Less => Some(Player::Two),",
        "            std::cmp::Ordering::Greater => Some(Player::Two),\n            std::cmp::Ordering::Less => Some(Player::One),",
        ["the_winner_is_whoever_has_the_most_boxes"],
    ),
    (
        "the score counts every claimed box rather than one player's",
        "            .filter(|cell| **cell == Some(player))",
        "            .filter(|cell| cell.is_some())",
        ["the_readouts_are_the_board_and_not_a_second_copy_of_it"],
    ),
    (
        "the move count counts only the horizontal lines",
        "        count(&self.h_lines).saturating_add(count(&self.v_lines))",
        "        count(&self.h_lines)",
        ["the_readouts_are_the_board_and_not_a_second_copy_of_it"],
    ),
    # ── The cursor ────────────────────────────────────────────────────────
    (
        "the cursor's column is not clamped when it changes grid",
        "            cursor.col = cursor.col.min(other_cols.saturating_sub(1));",
        "            cursor.col = cursor.col;",
        # Not the walk test: it starts in the horizontal grid, which is the
        # narrower of the two, so no column it can reach needs clamping when
        # the cursor crosses.  The crossing has to be *started* at vertical
        # column 3, which only the wrap test does.
        ["the_cursor_wraps_within_its_row_and_flips_between_the_two_grids"],
    ),
    (
        "a toggle does not clamp the cursor into the grid it lands in",
        "        self.cursor.row = self.cursor.row.min(rows.saturating_sub(1));\n        self.cursor.col = self.cursor.col.min(cols.saturating_sub(1));",
        "        self.cursor.row = self.cursor.row;\n        self.cursor.col = self.cursor.col;",
        ["a_toggle_clamps_the_cursor_into_the_grid_it_lands_in"],
    ),
    (
        "the two line grids are given each other's extents",
        "            Orientation::Horizontal => (gs, bps),\n            Orientation::Vertical => (bps, gs),",
        "            Orientation::Horizontal => (bps, gs),\n            Orientation::Vertical => (gs, bps),",
        ["the_cursor_is_a_line_and_always_a_line_that_exists"],
    ),
    (
        "the cursor's column does not wrap",
        "                let next = self.cursor.col.saturating_add(1);\n                self.cursor.col = if next < cols { next } else { 0 };",
        "                self.cursor.col = self.cursor.col.saturating_add(1);",
        ["the_cursor_is_a_line_and_always_a_line_that_exists"],
    ),
    (
        "stepping off the top enters the other grid at its top",
        "                    flip_to(&mut self.cursor, other_rows.saturating_sub(1));",
        "                    flip_to(&mut self.cursor, 0);",
        ["the_cursor_wraps_within_its_row_and_flips_between_the_two_grids"],
    ),
    (
        "Enter draws a line other than the one the cursor points at",
        "            Key::Enter | Key::Space if self.accepts_moves() => {\n                let line = self.cursor;",
        "            Key::Enter | Key::Space if self.accepts_moves() => {\n                let line = LineId::horizontal(0, 0);",
        ["enter_and_space_draw_the_line_the_cursor_points_at"],
    ),
    (
        "a key release is taken as a press",
        "            Event::Key(ke) if ke.pressed => self.handle_key(ke.key),",
        "            Event::Key(ke) => self.handle_key(ke.key),",
        ["a_key_release_is_not_a_key_press"],
    ),
    # ── The AI ────────────────────────────────────────────────────────────
    (
        "the AI takes the line that completes the fewest boxes",
        "        .max_by_key(|&(count, _)| count);",
        "        .min_by_key(|&(count, _)| count);",
        ["the_ai_prefers_the_line_that_completes_two_boxes"],
    ),
    (
        "the AI never looks for a box to complete",
        "        .filter(|&(count, _)| count > 0)",
        "        .filter(|&(count, _)| count > usize::MAX - 1)",
        ["the_ai_takes_a_box_it_can_complete"],
    ),
    (
        "the AI treats every line as safe",
        "        .filter(|&line| boxes_at(line, SIDES_PER_BOX.saturating_sub(2)) == 0)",
        "        .filter(|&line| boxes_at(line, SIDES_PER_BOX.saturating_sub(2)) >= 0)",
        ["the_ai_does_not_hand_over_a_box_while_a_safe_line_is_left"],
    ),
    (
        "the AI always plays the first safe line rather than one at random",
        "        return safe.get(rng.below(safe.len())).copied();",
        "        return safe.first().copied();",
        ["the_ai_breaks_ties_differently_under_different_seeds"],
    ),
    (
        "the AI moves the instant the turn passes to it",
        "            self.ai_delay_ms = self.ai_delay_ms.saturating_add(elapsed_ms);\n            if self.ai_delay_ms >= AI_DELAY {",
        "            self.ai_delay_ms = self.ai_delay_ms.saturating_add(elapsed_ms);\n            if self.ai_delay_ms >= 0 {",
        ["the_ai_waits_before_it_moves_so_that_thinking_is_a_frame_and_not_a_word"],
    ),
    (
        "the AI hands the turn back after completing a box",
        "            if completed > 0 {\n                // AI gets another turn.\n                self.ai_pending = true;",
        "            if false {\n                // AI gets another turn.\n                self.ai_pending = true;",
        ["the_ai_keeps_playing_while_it_keeps_completing_boxes"],
    ),
    # ── The seed ──────────────────────────────────────────────────────────
    (
        "a fresh game is seeded by a literal again",
        "            seed_from_system(FALLBACK_SEED),",
        "            42,",
        ["a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal"],
    ),
    (
        "a new game replays the last one",
        "        let seed = self.rng.next_u64();",
        "        let seed = FALLBACK_SEED;",
        ["a_new_game_does_not_replay_the_last_one"],
    ),
    # ── The end of the game ───────────────────────────────────────────────
    (
        "the end card is a fixed 260x140 again",
        "        let card_w = (widest + l.pad * 2.0).min(l.window.w);\n        let card_h = (text_h + l.pad * 2.0).min(l.window.h);",
        "        let card_w = 260.0f32;\n        let card_h = 140.0f32;",
        ["the_game_over_card_fits_the_window_it_is_drawn_in"],
    ),
    (
        "the end card names the wrong winner",
        "            Some(Player::One) if self.mode == GameMode::VsAi => String::from(\"You win!\"),",
        "            Some(Player::Two) if self.mode == GameMode::VsAi => String::from(\"You win!\"),",
        ["the_end_card_says_who_won_in_the_words_of_the_mode_being_played"],
    ),
    (
        "a claimed box carries the other player's initial",
        "            (GameMode::VsAi, Player::One) => \"Y\",\n            (GameMode::VsAi, Player::Two) => \"A\",",
        "            (GameMode::VsAi, Player::One) => \"A\",\n            (GameMode::VsAi, Player::Two) => \"Y\",",
        ["a_claimed_box_is_labelled_in_the_words_of_the_mode_being_played"],
    ),
    # ── The window ────────────────────────────────────────────────────────
    (
        "the app asks for no ticks, so the AI never moves",
        "        Some(std::time::Duration::from_millis(40))",
        "        None",
        ["the_app_opens_a_window_of_the_size_it_asks_for"],
    ),
    (
        "the close button does not close the window",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        {}",
        ["a_close_request_closes_the_window"],
    ),
    (
        "a move does not ask for a redraw",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        ["a_move_asks_for_a_redraw_and_a_dead_key_does_not"],
    ),
    (
        "render does not record the window it was given",
        "        self.size = (width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["render_records_the_window_it_was_given"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "dots", timeout=300, only=only))
