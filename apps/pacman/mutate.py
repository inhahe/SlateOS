"""Mutation test for the pacman suite.

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
    # -- Layout: the bands ---------------------------------------------
    (
        "the padding is allowed to be wider than the window it pads",
        "        let pad = (w.min(h) * 0.014).clamp(2.0, 12.0).min(w.min(h) / 2.0);",
        "        let pad = (w.min(h) * 0.014).clamp(2.0, 12.0);",
        ["layout_bands_stay_inside_the_window"],
    ),
    (
        "the header is given a share of the width instead of the height",
        "        let header_h = (h * 0.07).clamp(18.0, 52.0).min(h);",
        "        let header_h = (w * 0.07).clamp(18.0, 52.0).min(h);",
        ["the_bands_are_shares_of_the_height_not_the_width"],
    ),
    (
        "the header is allowed to be taller than the window",
        "        let header_h = (h * 0.07).clamp(18.0, 52.0).min(h);",
        "        let header_h = (h * 0.07).clamp(18.0, 52.0);",
        ["layout_bands_stay_inside_the_window"],
    ),
    (
        "the body is measured from the top of the window, not the header",
        "        let body_y = (header.bottom() + pad).min(h);",
        "        let body_y = pad;",
        ["layout_bands_stack_in_order_and_do_not_overlap"],
    ),
    (
        "the footer is allowed to start above the body",
        "        let footer_y = (h - footer_h).max(body_y);",
        "        let footer_y = h - footer_h;",
        ["layout_bands_stack_in_order_and_do_not_overlap"],
    ),
    (
        "the body is allowed a negative height",
        "                (footer_y - body_y - pad).max(0.0),",
        "                footer_y - body_y - pad,",
        ["layout_bands_never_have_a_negative_side"],
    ),
    (
        "the footer is allowed a negative height",
        "                (h - footer_y - pad).max(0.0),",
        "                h - footer_y - pad,",
        ["layout_bands_never_have_a_negative_side"],
    ),
    # -- Layout: the type ----------------------------------------------
    (
        "the sheet's title is smaller than its body text",
        "        let title = (h / 24.0).clamp(14.0, 34.0);",
        "        let title = (h / 90.0).clamp(1.0, 34.0);",
        ["layout_font_sizes_are_positive_and_ordered"],
    ),
    (
        "the type is a constant instead of a share of the window",
        "        let font = (h / 44.0).clamp(9.0, 18.0);",
        "        let font = 16.0;",
        ["a_taller_window_gets_larger_type"],
    ),
    # -- The fitted board ----------------------------------------------
    (
        "the cell is sized by the width alone, so a wide short window overflows",
        "        let cell = (aw / MAZE_COLS as f32).min(ah / MAZE_ROWS as f32).max(0.0);",
        "        let cell = (aw / MAZE_COLS as f32).max(0.0);",
        ["board_never_spills_out_of_the_body"],
    ),
    (
        "the grid is left-aligned in the body instead of centred",
        "            rect: Rect::new(area.x + (aw - w) / 2.0, area.y + (ah - h) / 2.0, w, h),",
        "            rect: Rect::new(area.x, area.y + (ah - h) / 2.0, w, h),",
        ["board_is_centred_in_the_body"],
    ),
    (
        "a cell's column is used for its row",
        "            self.rect.y + row as f32 * self.cell,",
        "            self.rect.y + col as f32 * self.cell,",
        [
            "a_cell_moves_down_with_its_row_and_right_with_its_column",
            "two_different_cells_never_share_a_box",
        ],
    ),
    (
        "the grid is a rectangle of cells that are not square",
        "        let w = cell * MAZE_COLS as f32;",
        "        let w = cell * MAZE_COLS as f32 + 1.0;",
        ["board_cells_are_square_and_fill_the_grid"],
    ),
    # -- The frame's clips ---------------------------------------------
    (
        "the window's clip is never popped",
        "        self.draw_sheet(&mut f, &l);\n        f.unclip();",
        "        self.draw_sheet(&mut f, &l);",
        ["the_frame_is_balanced_in_every_state_at_every_size"],
    ),
    (
        "nothing is clipped to the window",
        "        f.clip(l.window);\n        fill(&mut f, l.window, BASE, CornerRadii::ZERO);",
        "        fill(&mut f, l.window, BASE, CornerRadii::ZERO);",
        # Popping a clip that was never pushed would also unbalance the frame,
        # so the balance test fails alongside the one that owns the fault.
        [
            "a_zero_sized_window_draws_nothing_that_can_be_clicked",
            "the_frame_is_balanced_in_every_state_at_every_size",
        ],
    ),
    (
        "a header reading is written across the maze below it",
        "        f.clip(l.header);\n        let inner = l.pad.max(2.0);\n"
        "        let bold = FontWeightHint::Bold;",
        "        let inner = l.pad.max(2.0);\n        let bold = FontWeightHint::Bold;",
        # Same pairing: the unmatched `unclip` at the end of the header shows up
        # as an unbalanced frame as well as an escaped reading.
        [
            "every_hit_box_stays_inside_the_window",
            "the_frame_is_balanced_in_every_state_at_every_size",
        ],
    ),
    (
        "the board's hit box is the whole body band, not the fitted grid",
        "        f.hit(Target::Board, board.rect);",
        "        f.hit(Target::Board, l.body);",
        ["the_board_hit_covers_the_whole_grid"],
    ),
    # -- The header's readings -----------------------------------------
    (
        "the level reading is placed a fixed 120 pixels from the right edge",
        "            l.header.right() - inner - width,",
        "            l.header.right() - 120.0,",
        [
            "the_level_reading_is_right_aligned_at_every_width",
            "a_wider_level_reading_starts_further_left",
        ],
    ),
    (
        "the level reading is left-aligned, so a longer one runs off the edge",
        "        let width = text::measure(&lvl, l.head, bold);",
        "        let width = 0.0;",
        [
            "the_level_reading_is_right_aligned_at_every_width",
            "a_wider_level_reading_starts_further_left",
        ],
    ),
    # -- The footer's tokens -------------------------------------------
    (
        "the life tokens start a flat fifty pixels in",
        "            let cx = word.right() + inner + token + f32_from_u32(i) * step;",
        "            let cx = 50.0 + f32_from_u32(i) * step;",
        ["the_life_tokens_start_after_the_word_lives"],
    ),
    (
        "the life tokens are all drawn on top of each other",
        "        let step = token * 2.0 + inner;",
        "        let step = 0.0;",
        ["the_life_tokens_do_not_sit_on_top_of_each_other"],
    ),
    (
        "the row of life tokens is not capped",
        "        for i in 0..self.lives.min(MAX_LIVES_SHOWN) {",
        "        for i in 0..self.lives {",
        ["the_footer_shows_no_more_tokens_than_it_has_room_for"],
    ),
    # -- The sheet -----------------------------------------------------
    (
        "the sheet is centred on its first line instead of on the whole stack",
        "        let mut y = cy - total / 2.0;",
        "        let mut y = cy;",
        ["the_sheet_is_centred_on_the_window_at_every_shape"],
    ),
    (
        "the sheet's lines are stacked without allowing for their height",
        "            y += text::line_height(line.size, line.weight) + gap;",
        "            y += gap;",
        ["sheet_lines_are_stacked_top_to_bottom_and_do_not_overlap"],
    ),
    (
        "the sheet is left-aligned instead of centred on the window",
        "        let (cx, cy) = l.window.centre();",
        "        let (cx, cy) = (0.0, l.window.centre().1);",
        ["the_sheet_lines_are_horizontally_centred"],
    ),
    (
        "the pause sheet's resume line is a second new-game line",
        '                    text: "Press P or Esc to resume".to_string(),\n'
        "                    target: Some(Target::Resume),",
        '                    text: "Press P or Esc to resume".to_string(),\n'
        "                    target: Some(Target::NewGame),",
        ["the_pause_sheet_offers_both_ways_out"],
    ),
    (
        "the game-over sheet reports the score without the number",
        '                    text: format!("Score: {}", self.score),',
        '                    text: "Score:".to_string(),',
        ["the_game_over_sheet_reports_the_score_and_the_level_it_ended_on"],
    ),
    (
        "the sheet is drawn over the board during play",
        '            GameState::Playing => return,',
        '            GameState::Playing => ("PAC-MAN", YELLOW, 220),',
        ["nothing_covers_the_board_while_the_game_is_being_played"],
    ),
    # -- Click routing -------------------------------------------------
    (
        "the start line is drawn but does nothing",
        "            Some(Target::NewGame) => {\n                self.start_new_game();",
        "            Some(Target::NewGame) => {\n                self.state = self.state;",
        [
            "clicking_the_start_line_starts_the_game",
            "clicking_new_game_on_the_pause_sheet_restarts",
            "clicking_new_game_after_a_loss_restarts_and_keeps_the_high_score",
        ],
    ),
    (
        "the resume line restarts the game instead of resuming it",
        "            Some(Target::Resume) if self.state == GameState::Paused => {\n"
        "                self.state = GameState::Playing;",
        "            Some(Target::Resume) if self.state == GameState::Paused => {\n"
        "                self.start_new_game();",
        ["clicking_resume_resumes_the_paused_game"],
    ),
    (
        "a click on the dimmed sheet falls through to the board",
        "            Some(\n                Target::Overlay\n                | Target::OverlayTitle\n"
        "                | Target::Resume\n                | Target::Controls(_)\n"
        "                | Target::FinalStat(_),\n            ) => EventResult::Consumed,",
        "            Some(\n                Target::Overlay\n                | Target::OverlayTitle\n"
        "                | Target::Resume\n                | Target::Controls(_)\n"
        "                | Target::FinalStat(_),\n            ) => EventResult::Ignored,",
        [
            "clicking_a_control_line_is_taken_and_changes_nothing",
            "clicking_the_dim_part_of_the_sheet_does_not_reach_the_board",
        ],
    ),
    (
        "every mouse event is treated as a left click",
        "        if ev.kind != MouseEventKind::Press(MouseButton::Left) {\n"
        "            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_right_click_on_the_start_line_does_nothing", "a_release_is_not_a_click"],
    ),
    # -- The size a click is read against ------------------------------
    (
        "clicks are read against the window the app opened at, not the live one",
        "        let target = self.frame(self.size.0, self.size.1).hit_test(ev.x, ev.y);",
        "        let target = self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hit_test(ev.x, ev.y);",
        ["a_resize_event_is_what_moves_them"],
    ),
    (
        "a resize is noticed but not acted on",
        "            app.resize(f32_from_u32(*width), f32_from_u32(*height));",
        "            let _ = (width, height);",
        ["a_resize_event_is_what_moves_them"],
    ),
    (
        "drawing does not adopt the size it was given",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_adopts_the_size_it_was_given"],
    ),
    (
        "a negative window size is stored as it arrived",
        "        self.size = (width.max(0.0), height.max(0.0));",
        "        self.size = (width, height);",
        ["a_negative_size_is_read_as_nothing_rather_than_inside_out"],
    ),
    (
        "the window opens at a size its frames are not measured for",
        "        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "        (800, 600)",
        ["the_window_opens_at_the_size_its_frames_are_measured_for"],
    ),
    (
        "the window is woken too rarely to animate",
        "const TICK: Duration = Duration::from_millis(16);",
        "const TICK: Duration = Duration::from_millis(500);",
        ["the_window_asks_to_be_woken_often_enough_to_animate"],
    ),
    (
        "the close button is ignored",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Idle;\n        }",
        ["closing_the_window_exits"],
    ),
    (
        "every event asks for a redraw, whether it changed anything or not",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["a_consumed_event_asks_for_a_redraw_and_an_ignored_one_does_not"],
    ),
    # -- Game rules the wiring exposed ---------------------------------
    (
        "a live ghost may hide inside the ghost house",
        "            _ => cell.is_walkable(),",
        "            _ => cell.is_walkable() || cell == Cell::GhostDoor || cell == Cell::GhostHouse,",
        ["test_live_ghosts_never_stand_in_the_house"],
    ),
    (
        "a revived ghost is left standing on the house door",
        "                ghost.pos = GHOST_HOUSE_EXIT;\n                ghost.direction = Direction::Up;",
        "                ghost.direction = Direction::Up;",
        ["test_revived_ghost_leaves_the_door_and_keeps_moving"],
    ),
    (
        "starting a new game forgets the window it is played in",
        "        self.high_score = high;\n        self.size = size;",
        "        self.high_score = high;",
        ["starting_a_new_game_keeps_the_window_it_is_played_in"],
    ),
    (
        "starting a new game forgets the high score",
        "        self.high_score = high;\n        self.size = size;",
        "        self.size = size;",
        [
            "clicking_new_game_after_a_loss_restarts_and_keeps_the_high_score",
            "test_high_score_preserved_on_new_game",
        ],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "pacman", timeout=300, only=sys.argv[1:] or None))
