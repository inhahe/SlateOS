"""Mutation test for the checkers suite.

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
    # -- The board's orientation ---------------------------------------
    #
    # Every one of these was a way the old file could be, and one of them is
    # how it actually was: the `7 - row` flip lived in three places, and the
    # hit test's copy was inverted by hand.
    #
    # None of the three is caught by the test that reads the rank number or
    # the file letter against the square it names, and that is the point of
    # those two tests rather than a hole in them: the labels are placed
    # against `g.square(..)` itself, so they follow the squares wherever the
    # grid puts them and cannot disagree with a mapping that is wrong. What
    # they check is that no second copy of the arithmetic was written for
    # them -- which is exactly the fault this file was rewritten to remove.
    # A board that is upside down or mirrored is caught instead by
    # `the_ranks_read_one_to_eight_upwards_and_the_files_a_to_h_rightwards`,
    # which reads the labels against the *screen*.
    (
        "rank 1 is drawn at the top, the way a screen counts rather than a board",
        "        let screen_row = f32::from(7i8.saturating_sub(row));",
        "        let screen_row = f32::from(row);",
        [
            "rank_one_is_at_the_bottom_and_file_a_at_the_left",
            "the_squares_tile_the_board_edge_to_edge",
            "the_ranks_read_one_to_eight_upwards_and_the_files_a_to_h_rightwards",
            "the_board_is_painted_the_colour_it_says_each_square_is",
        ],
    ),
    (
        "the files run h to a, so the board is mirrored left to right",
        "            self.origin.0 + f32::from(col) * self.step,",
        "            self.origin.0 + f32::from(7i8.saturating_sub(col)) * self.step,",
        [
            "rank_one_is_at_the_bottom_and_file_a_at_the_left",
            "the_ranks_read_one_to_eight_upwards_and_the_files_a_to_h_rightwards",
            "the_squares_tile_the_board_edge_to_edge",
            "the_board_is_painted_the_colour_it_says_each_square_is",
        ],
    ),
    (
        "the rank and the file are swapped, so the board is transposed",
        "        let screen_row = f32::from(7i8.saturating_sub(row));\n"
        "        Rect::new(\n"
        "            self.origin.0 + f32::from(col) * self.step,\n"
        "            self.origin.1 + screen_row * self.step,",
        "        let screen_row = f32::from(7i8.saturating_sub(row));\n"
        "        Rect::new(\n"
        "            self.origin.0 + screen_row * self.step,\n"
        "            self.origin.1 + f32::from(col) * self.step,",
        [
            "rank_one_is_at_the_bottom_and_file_a_at_the_left",
            "the_ranks_read_one_to_eight_upwards_and_the_files_a_to_h_rightwards",
            "the_squares_tile_the_board_edge_to_edge",
        ],
    ),
    # -- The board's geometry ------------------------------------------
    (
        "the board is fitted to the width alone, so it runs off a short window",
        "        let step = ((area.w - label).max(0.0) / side)\n"
        "            .min((area.h - label).max(0.0) / side)\n"
        "            .max(0.0);",
        "        let step = ((area.w - label).max(0.0) / side).max(0.0);",
        ["the_board_stays_inside_the_window"],
    ),
    (
        "the board is fitted to the height alone, so it runs off a narrow window",
        "        let step = ((area.w - label).max(0.0) / side)\n"
        "            .min((area.h - label).max(0.0) / side)\n"
        "            .max(0.0);",
        "        let step = ((area.h - label).max(0.0) / side).max(0.0);",
        ["the_board_and_the_panel_do_not_overlap"],
    ),
    (
        "the squares are as wide as the board is and as tall as one rank",
        "        Rect::new(\n"
        "            self.origin.0 + f32::from(col) * self.step,\n"
        "            self.origin.1 + screen_row * self.step,\n"
        "            self.step,\n"
        "            self.step,\n"
        "        )",
        "        Rect::new(\n"
        "            self.origin.0 + f32::from(col) * self.step,\n"
        "            self.origin.1 + screen_row * self.step,\n"
        "            self.step * 8.0,\n"
        "            self.step,\n"
        "        )",
        [
            "the_squares_are_square_and_all_the_same_size",
            "the_squares_tile_the_board_edge_to_edge",
            "the_board_box_is_exactly_the_squares_it_holds",
        ],
    ),
    (
        "the squares are spread a step and a half apart, leaving gaps between them",
        "            self.origin.1 + screen_row * self.step,",
        "            self.origin.1 + screen_row * self.step * 1.5,",
        [
            "the_squares_tile_the_board_edge_to_edge",
            "the_board_stays_inside_the_window",
            "the_board_box_is_exactly_the_squares_it_holds",
        ],
    ),
    (
        "the board box is nine squares across, not eight",
        "            self.step * 8.0,\n            self.step * 8.0,",
        "            self.step * 9.0,\n            self.step * 9.0,",
        ["the_board_box_is_exactly_the_squares_it_holds"],
    ),
    (
        "the rank gutter is taken out of the board's room but never left empty",
        "            origin: (left + label, top),",
        "            origin: (left, top),",
        ["the_rank_numbers_line_up_with_the_ranks_they_name"],
    ),
    (
        "the board is pinned to the top-left of its area rather than centred",
        "        let left = area.x + (area.w - board - label).max(0.0) / 2.0;\n"
        "        let top = area.y + (area.h - board - label).max(0.0) / 2.0;",
        "        let left = area.x;\n        let top = area.y;",
        ["widening_the_window_moves_the_board_rather_than_the_gap_beside_it"],
    ),
    # -- Clicking ------------------------------------------------------
    (
        "a click is resolved against the opening size rather than the current one",
        "        let (w, h) = self.size;\n"
        "        let Some(target) = self.frame(w, h).hit_test(x, y) else {",
        "        let Some(target) = self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hit_test(x, y) else {",
        # Not `a_click_in_each_corner_of_a_square_reaches_that_square`: it runs
        # at the opening size only -- a corner click is a rounding test and
        # wants one size read closely, not eight read loosely -- so resolving
        # against the opening size is exactly what it already does.
        [
            "a_click_in_a_square_reaches_that_square",
            "a_resize_event_is_what_the_next_click_is_read_against",
        ],
    ),
    (
        "a click lands on the square the cursor is already on, not the one clicked",
        "                self.cursor = Pos::new(row, col);",
        "                let _ = (row, col);",
        [
            "a_click_in_a_square_reaches_that_square",
            "a_click_in_each_corner_of_a_square_reaches_that_square",
            "a_click_selects_a_piece_and_a_second_click_moves_it",
            "test_mouse_click_on_piece",
        ],
    ),
    (
        "a click outside every recorded box is treated as a click on the board",
        "        let Some(target) = self.frame(w, h).hit_test(x, y) else {\n"
        "            return EventResult::Ignored;\n"
        "        };",
        "        let target = self\n"
        "            .frame(w, h)\n"
        "            .hit_test(x, y)\n"
        "            .unwrap_or(Target::Square(0, 0));",
        ["a_click_off_the_board_moves_no_cursor"],
    ),
    (
        "a click on the panel falls through to the window instead of being answered",
        "            _ => EventResult::Consumed,",
        "            _ => EventResult::Ignored,",
        ["a_click_on_the_panel_is_answered_rather_than_dropped"],
    ),
    (
        "a right click plays a move, the same as a left one",
        "        if button != MouseButton::Left {\n"
        "            return EventResult::Ignored;\n"
        "        }",
        "        let _ = button;",
        ["a_right_click_is_not_a_move"],
    ),
    (
        "the new-game button is drawn but does nothing when clicked",
        "            Target::NewGame => {\n"
        "                self.new_game();\n"
        "                EventResult::Consumed\n"
        "            }",
        "            Target::NewGame => EventResult::Consumed,",
        ["the_new_game_button_starts_a_new_game"],
    ),
    # -- What the frame says -------------------------------------------
    (
        "the frame is drawn at the remembered size rather than the one asked for",
        "    fn draw(&self, size: (f32, f32)) -> Frame<Target> {\n"
        "        self.frame(size.0, size.1)",
        "    fn draw(&self, size: (f32, f32)) -> Frame<Target> {\n"
        "        let _ = size;\n"
        "        self.frame(self.size.0, self.size.1)",
        # This one mutates the suite's own window on the app rather than the
        # app, and is here to check that the suite really does vary the size it
        # draws at instead of asking for eight and always getting one.  So the
        # tests that answer are the ones that draw at a size other than the
        # opening one -- which is what makes them worth having.
        [
            "a_click_in_a_square_reaches_that_square",
            "nothing_is_drawn_at_a_zero_sized_window",
            "the_board_stays_inside_the_window",
            "the_whole_frame_is_clipped_to_the_window",
            "widening_the_window_moves_the_board_rather_than_the_gap_beside_it",
        ],
    ),
    (
        "nothing is clipped, so a window too small paints over its neighbours",
        "        f.clip(l.window);",
        "        f.clip(Rect::new(0.0, 0.0, f32::MAX, f32::MAX));",
        ["the_whole_frame_is_clipped_to_the_window"],
    ),
    (
        "a resize is remembered but the redraw is not asked for",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.resize(width, height);\n"
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()",
        ["the_render_pass_draws_at_the_size_the_window_hands_it"],
    ),
    (
        "a resize event is ignored, the way the fixed-pixel version ignored it",
        "        Event::Resize { width, height } => {\n"
        "            app.resize(f32_from_u32(*width), f32_from_u32(*height));\n"
        "            EventResult::Consumed\n"
        "        }",
        "        Event::Resize { .. } => EventResult::Consumed,",
        [
            "a_resize_event_is_what_the_next_click_is_read_against",
            "a_resize_changes_the_size_without_disturbing_the_game",
        ],
    ),
    (
        "a new game snaps the window back to its opening size",
        "    fn new_game(&mut self) {",
        "    fn new_game(&mut self) {\n"
        "        self.size = (WINDOW_WIDTH, WINDOW_HEIGHT);",
        ["a_new_game_keeps_the_window_size"],
    ),
    # -- The pieces ----------------------------------------------------
    (
        "a piece is drawn at the corner of its square rather than the middle",
        "    fn centre(self, row: i8, col: i8) -> (f32, f32) {\n"
        "        let r = self.square(row, col);\n"
        "        (r.x + r.w / 2.0, r.y + r.h / 2.0)",
        "    fn centre(self, row: i8, col: i8) -> (f32, f32) {\n"
        "        let r = self.square(row, col);\n"
        "        (r.x, r.y)",
        [
            "a_piece_is_drawn_in_the_middle_of_the_square_it_stands_on",
            "a_king_wears_a_crown_centred_on_its_own_piece",
        ],
    ),
    (
        "the crown is nudged by hand the way it used to be, so it slides off its piece",
        "            cx - ink.width(crown) / 2.0,\n            cy - ink.height() / 2.0,",
        "            cx - 8.0,\n            cy - 10.0,",
        ["a_king_wears_a_crown_centred_on_its_own_piece"],
    ),
    (
        "the selection ring is drawn on the cursor's square instead of the selected one",
        "                if self.selected == Some(pos) {",
        "                if self.cursor == pos {",
        # Not `the_cursor_is_drawn_on_the_square_it_is_on`: the two rings are
        # different colours and it looks for the cursor's, which this mutation
        # leaves exactly where it was.  What the mutation does is put a *second*
        # ring on top of it, and the test that notices is the one asking where
        # the selection ring went.
        ["the_selection_ring_is_drawn_on_the_selected_square"],
    ),
    # -- The header ----------------------------------------------------
    (
        "the header counts the pieces the other side has",
        "                format!(\"{}: {}\", side.name(), self.board.count_pieces(side))",
        "                format!(\n"
        "                    \"{}: {}\",\n"
        "                    side.name(),\n"
        "                    self.board.count_pieces(side.opponent())\n"
        "                )",
        ["the_header_counts_the_pieces_each_side_has_left"],
    ),
    (
        "the header never mentions kings, however many there are",
        "            let text = if kings == 0 {",
        "            let text = if true {",
        ["the_header_counts_kings_once_there_are_any"],
    ),
    (
        "the chips are placed at a hand-counted offset instead of after the title",
        "        let mut x = title.right() + gap;",
        "        let mut x = band.x + 120.0;",
        ["the_piece_counts_follow_the_title_rather_than_a_fixed_offset"],
    ),
    # -- The panel and the status line ---------------------------------
    (
        "a panel row is drawn wherever the cursor reached, fitting or not",
        "    if row.y < band.y - 0.01 || row.bottom() > band.bottom() + 0.01 {\n"
        "        return Rect::new(row.x, row.y, 0.0, 0.0);\n"
        "    }",
        "",
        ["the_panel_holds_everything_it_draws"],
    ),
    (
        "the help is stacked under the history rather than anchored to the floor",
        "        let help_top = (inner.bottom() - help_h).max(y);",
        "        let help_top = y;",
        ["the_panel_keeps_its_history_clear_of_its_help"],
    ),
    (
        "the history is given a fixed eighteen rows, the way it used to be",
        "        let rows = count_from_f32((help_top - y) / row_h);",
        "        let rows = 18;",
        ["the_panel_keeps_its_history_clear_of_its_help"],
    ),
    (
        "no row is ever drawn, so the panel is an empty box",
        "    label_in(f, row, s, ink)\n}",
        "    let _ = (f, s, ink);\n    row\n}",
        ["the_panel_draws_its_rows_while_they_fit"],
    ),
    (
        "the status line names the side that just moved rather than the side to move",
        "                None => format!(\"{} to move\", self.board.side_to_move.name()),",
        "                None => format!(\"{} to move\", self.board.side_to_move.opponent().name()),",
        ["the_status_line_says_whose_turn_it_is"],
    ),
    (
        "the unseeable thinking notice is put back",
        "        self.do_ai_move();",
        "        self.move_history.push(\"Black thinking...\".to_string());\n"
        "        self.do_ai_move();",
        ["no_frame_ever_says_black_is_thinking"],
    ),
    (
        "the captures are credited to the side taken from, the way they used to be",
        "        self.red_takes = self.red_takes.saturating_add(u32_from_usize(captured));",
        "        self.black_takes = self.black_takes.saturating_add(u32_from_usize(captured));",
        ["the_captures_line_credits_the_side_that_did_the_taking"],
    ),
    (
        "the panel prints the two capture counts the wrong way round",
        '            &format!("Red {}   Black {}", self.red_takes, self.black_takes),',
        '            &format!("Red {}   Black {}", self.black_takes, self.red_takes),',
        ["the_captures_line_credits_the_side_that_did_the_taking"],
    ),
    # -- The keyboard --------------------------------------------------
    (
        "Up lowers the rank, as though rank 1 were at the top",
        "            Key::Up => self.cursor.row = self.cursor.row.saturating_add(1).min(7),\n"
        "            Key::Down => self.cursor.row = self.cursor.row.saturating_sub(1).max(0),",
        "            Key::Up => self.cursor.row = self.cursor.row.saturating_sub(1).max(0),\n"
        "            Key::Down => self.cursor.row = self.cursor.row.saturating_add(1).min(7),",
        ["the_cursor_walks_the_board_and_stops_at_its_edges"],
    ),
    (
        "the cursor walks off the top of the board",
        "            Key::Up => self.cursor.row = self.cursor.row.saturating_add(1).min(7),",
        "            Key::Up => self.cursor.row = self.cursor.row.saturating_add(1),",
        ["the_cursor_walks_the_board_and_stops_at_its_edges"],
    ),
    (
        "the cursor walks off the right of the board",
        "            Key::Right => self.cursor.col = self.cursor.col.saturating_add(1).min(7),",
        "            Key::Right => self.cursor.col = self.cursor.col.saturating_add(1),",
        ["test_cursor_upper_bounds"],
    ),
    # -- The window ----------------------------------------------------
    (
        "a key that changed nothing still asks for a redraw",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["the_window_is_asked_to_redraw_only_when_something_changed"],
    ),
    (
        "the window opens at a size the layout was not written against",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (640, 480)",
        ["the_window_opens_at_the_size_the_layout_is_written_for"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "checkers", timeout=420, only=sys.argv[1:] or None))
