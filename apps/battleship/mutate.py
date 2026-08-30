"""Mutation test for the battleship suite.

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
    # -- Layout: the five bands ----------------------------------------
    (
        "the padding is allowed to be wider than the window it pads",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 24.0).min(w.min(h) / 2.0);",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 24.0);",
        ["the_padding_never_vanishes_never_runs_away_and_never_outgrows_the_window"],
    ),
    (
        "the padding vanishes on a small window instead of holding a floor",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 24.0).min(w.min(h) / 2.0);",
        "        let pad = (w.min(h) * 0.02).min(w.min(h) / 2.0);",
        ["the_padding_never_vanishes_never_runs_away_and_never_outgrows_the_window"],
    ),
    (
        "the header is given a share of the width instead of the height",
        "        let header_h = h * 0.075;",
        "        let header_h = w * 0.075;",
        ["the_bands_are_shares_of_the_height_not_the_width"],
    ),
    (
        "the message band starts at the top of the window, not under the header",
        "        let message = Rect::new(0.0, header.bottom(), w, message_h);",
        "        let message = Rect::new(0.0, 0.0, w, message_h);",
        ["the_bands_run_down_the_window_in_order_and_do_not_overlap"],
    ),
    (
        "the stats band is stacked on top of the body it follows",
        "        let stats = Rect::new(0.0, body.bottom(), w, stats_h);",
        "        let stats = Rect::new(0.0, message.bottom(), w, stats_h);",
        ["the_bands_run_down_the_window_in_order_and_do_not_overlap"],
    ),
    (
        "the body keeps the share the help band was to take",
        "        let body_h = rest - help_h;",
        "        let body_h = rest;",
        ["the_bands_run_down_the_window_in_order_and_do_not_overlap"],
    ),
    (
        "what the bands have left is measured from the window, not from itself",
        "        let rest = rest - stats_h;",
        "        let rest = h - stats_h;",
        ["the_bands_run_down_the_window_in_order_and_do_not_overlap"],
    ),
    (
        "a band is given a share of the window rather than all its width",
        "        let help = Rect::new(0.0, stats.bottom(), w, help_h);",
        "        let help = Rect::new(0.0, stats.bottom(), w * 0.5, help_h);",
        ["the_bands_fill_the_window_and_span_its_width"],
    ),
    # -- Layout: the two grids -----------------------------------------
    (
        "the cell is fitted to the width alone, as the old fixed layout was",
        "        let step = (avail_w / (side * 2.0)).min(avail_h / side).max(0.0);",
        "        let step = (avail_w / (side * 2.0)).max(0.0);",
        ["the_two_grids_never_overlap_and_both_stay_inside_the_body"],
    ),
    (
        "the width is measured for one grid when it has to hold two",
        "        let avail_w = (area.w - pad * 3.0 - label * 2.0).max(0.0);",
        "        let avail_w = (area.w - pad * 3.0 - label * 2.0).max(0.0) * 2.0;",
        ["the_two_grids_never_overlap_and_both_stay_inside_the_body"],
    ),
    (
        "the cells are drawn as tall as the step, leaving no gap between them",
        "        let cell = (step * 0.94).max(0.0);",
        "        let cell = step.max(0.0);",
        ["a_bigger_window_gets_bigger_cells"],
    ),
    (
        "the cell is square in name only, the drawn square taking its step",
        "            self.cell,\n            self.cell,\n        )",
        "            self.cell,\n            self.step,\n        )",
        ["the_cells_are_square_and_the_two_grids_are_the_same_size"],
    ),
    (
        "the second grid is laid out on top of the first",
        "            ocean: (left + label * 2.0 + grid_w + pad, top),",
        "            ocean: (left + label, top),",
        ["the_two_grids_never_overlap_and_both_stay_inside_the_body"],
    ),
    (
        "no caption line is left above the 1-10 row",
        "        let top = area.y + pad + caption + label;",
        "        let top = area.y + pad + caption;",
        ["the_row_and_column_labels_have_room_left_of_and_above_the_cells"],
    ),
    (
        "no room is left left of the cells for the A-J column",
        "            own: (left + label, top),",
        "            own: (left, top),",
        ["the_row_and_column_labels_have_room_left_of_and_above_the_cells"],
    ),
    (
        "a cell's column is stepped by the row and its row by the column",
        "            origin.0 + f32_from_usize(col) * self.step,\n"
        "            origin.1 + f32_from_usize(row) * self.step,",
        "            origin.0 + f32_from_usize(row) * self.step,\n"
        "            origin.1 + f32_from_usize(col) * self.step,",
        ["the_cells_are_drawn_where_the_grid_says_they_are"],
    ),
    (
        "the board is measured by the drawn square rather than the step",
        "        let side = self.step * f32_from_usize(GRID_SIZE);",
        "        let side = self.cell * f32_from_usize(GRID_SIZE);",
        ["the_board_behind_the_cells_is_reachable_where_the_cells_are_not"],
    ),
    (
        "the caption is not centred over the grid it names",
        "        let x = board.x + (board.w - w) / 2.0;",
        "        let x = board.x;",
        ["each_grid_s_caption_is_centred_over_the_grid_it_names"],
    ),
    (
        "the whole window is left unclipped",
        "        f.clip(l.window);",
        "        f.clip(Rect::new(0.0, 0.0, f32::MAX, f32::MAX));",
        ["nothing_is_drawn_outside_the_window"],
    ),
    # -- Clicks --------------------------------------------------------
    (
        "a cell records no box for a click to find",
        "                f.hit(Target::Own(byte(r), byte(c)), rect);",
        "                let _ = rect;",
        ["every_cell_of_both_grids_records_a_box_a_click_can_find"],
    ),
    (
        "the ocean records no box for a click to find",
        "                f.hit(Target::Ocean(byte(r), byte(c)), rect);",
        "                let _ = rect;",
        ["every_cell_of_both_grids_records_a_box_a_click_can_find"],
    ),
    (
        "a click on one's own board moves the cursor but never places the ship",
        "                self.clamp_placement();\n                self.try_place_current_ship();",
        "                self.clamp_placement();",
        ["a_click_on_your_own_grid_places_the_ship_being_placed"],
    ),
    (
        "a click places the ship without first pulling it back on the board",
        "                self.clamp_placement();\n                self.try_place_current_ship();",
        "                self.try_place_current_ship();",
        ["a_click_that_would_hang_a_ship_off_the_board_pulls_it_back_on"],
    ),
    (
        "a click on the ocean aims but never fires",
        "                self.cursor_col = usize::from(c);\n                self.fire_at_opponent();",
        "                self.cursor_col = usize::from(c);",
        ["a_click_on_the_ocean_fires_at_that_cell"],
    ),
    (
        "a click on the ocean fires at wherever the cursor already was",
        "                self.cursor_row = usize::from(r);\n"
        "                self.cursor_col = usize::from(c);\n"
        "                self.fire_at_opponent();",
        "                self.fire_at_opponent();",
        ["a_click_on_the_ocean_fires_at_that_cell"],
    ),
    (
        "one's own grid is clickable in the phase it is not for",
        "            Target::Own(r, c) if self.phase == GamePhase::Placement => {",
        "            Target::Own(r, c) => {",
        ["neither_grid_answers_a_click_in_the_phase_it_is_not_for"],
    ),
    (
        "the ocean is clickable in the phase it is not for",
        "            Target::Ocean(r, c) if self.phase == GamePhase::Firing => {",
        "            Target::Ocean(r, c) => {",
        ["neither_grid_answers_a_click_in_the_phase_it_is_not_for"],
    ),
    (
        "every button of the mouse plays the game",
        "        if button != MouseButton::Left {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["only_the_left_button_does_anything"],
    ),
    (
        "a click that hit nothing is answered anyway",
        "            return EventResult::Ignored;\n        };",
        "            return EventResult::Consumed;\n        };",
        ["a_click_outside_everything_is_left_alone"],
    ),
    (
        "the chrome swallows nothing, so a click falls through it",
        "            | Target::Help => EventResult::Consumed,",
        "            | Target::Help => EventResult::Ignored,",
        ["a_click_on_the_chrome_is_answered_and_changes_nothing"],
    ),
    (
        "a click is read against the size the window opened at, not its size now",
        "        let (w, h) = self.size;",
        "        let (w, h) = (WINDOW_WIDTH, WINDOW_HEIGHT);",
        ["a_resize_is_what_the_next_click_is_read_against"],
    ),
    (
        "a resize is not remembered",
        "            app.resize(f32_from_u32(*width), f32_from_u32(*height));",
        "            let _ = (width, height);",
        ["a_resize_is_what_the_next_click_is_read_against"],
    ),
    # -- Keys ----------------------------------------------------------
    (
        "escape deals a new board, as it used to",
        "            return Response::Exit;\n        }\n        match handle_event(self, event) {",
        "            self.new_game();\n            return Response::Redraw;\n        }\n"
        "        match handle_event(self, event) {",
        ["escape_does_not_deal_a_new_board", "escape_closes_the_window"],
    ),
    (
        "N deals a new board only while ships are still being placed",
        "            Key::N => {\n                self.new_game();",
        "            Key::N if self.phase == GamePhase::Placement => {\n                self.new_game();",
        ["n_deals_a_new_board_in_every_phase"],
    ),
    (
        "a key the game has no use for is swallowed anyway",
        "            _ => return EventResult::Ignored,\n        }\n"
        "        // Every move and every rotation is followed by the same clamp",
        "            _ => return EventResult::Consumed,\n        }\n"
        "        // Every move and every rotation is followed by the same clamp",
        ["a_key_the_game_has_no_use_for_is_left_alone"],
    ),
    (
        "a rotation is not followed by the clamp that keeps the ship on the board",
        "            Key::R => self.placement_orientation = self.placement_orientation.toggle(),",
        "            Key::R => {\n"
        "                self.placement_orientation = self.placement_orientation.toggle();\n"
        "                return EventResult::Consumed;\n"
        "            }",
        [
            "the_clamp_keeps_every_ship_on_the_board_from_every_cell",
            "test_placement_clamp_after_rotate",
        ],
    ),
    (
        "the clamp guards the along-axis of the wrong orientation",
        "            Orientation::Horizontal => (LAST_CELL, last_along),\n"
        "            Orientation::Vertical => (last_along, LAST_CELL),",
        "            Orientation::Horizontal => (last_along, LAST_CELL),\n"
        "            Orientation::Vertical => (LAST_CELL, last_along),",
        ["the_clamp_keeps_every_ship_on_the_board_from_every_cell"],
    ),
    (
        "the last legal origin is off by one, so a ship may end one cell over",
        "        let last_along = GRID_SIZE.saturating_sub(size);",
        "        let last_along = GRID_SIZE.saturating_sub(size).saturating_add(1);",
        ["the_clamp_keeps_every_ship_on_the_board_from_every_cell"],
    ),
    (
        "the keyboard and the click do not place the same ship",
        "                self.placement_row = usize::from(r);",
        "                self.placement_row = usize::from(r).saturating_add(1).min(LAST_CELL);",
        ["the_click_and_the_key_place_the_same_ship"],
    ),
    (
        "the keyboard and the click do not fire the same shell",
        "                self.cursor_row = usize::from(r);",
        "                self.cursor_row = usize::from(r).saturating_add(1).min(LAST_CELL);",
        ["the_click_and_the_key_fire_the_same_shell"],
    ),
    # -- What is drawn -------------------------------------------------
    (
        "the phase is named but not coloured by how it is going",
        "            GamePhase::GameOver if self.player_won => (\"Victory!\", GREEN),",
        "            GamePhase::GameOver if self.player_won => (\"Victory!\", RED),",
        ["the_phase_is_named_in_the_header_and_coloured_by_how_it_is_going"],
    ),
    (
        "the phase is drawn from the left, over the title",
        "        let right = Rect::new(band.right() - w, band.y, w, band.h);",
        "        let right = Rect::new(band.x, band.y, w, band.h);",
        ["the_two_halves_of_the_header_do_not_sit_on_top_of_each_other"],
    ),
    (
        "the message bar answers only where its glyphs are",
        "        f.hit(Target::Message, l.message);",
        "        f.hit(Target::Message, Rect::new(l.message.x, l.message.y, 1.0, 1.0));",
        ["the_message_bar_carries_the_message_and_answers_across_its_whole_width"],
    ),
    (
        "the aiming ring is drawn on the cell the cursor is not on",
        "            let rect = g.cell_rect(origin, self.cursor_row, self.cursor_col);",
        "            let rect = g.cell_rect(origin, 0, 0);",
        ["the_aiming_ring_is_drawn_on_the_cell_the_cursor_is_on", "the_ring_follows_the_cursor"],
    ),
    (
        "the aiming ring is drawn in every phase, with nothing to aim at",
        "        if self.phase == GamePhase::Firing {\n            let rect = g.cell_rect(",
        "        if true {\n            let rect = g.cell_rect(",
        ["the_ring_is_drawn_only_while_there_is_something_to_aim_at"],
    ),
    (
        "the ship being placed is previewed on one cell, not the cells it takes",
        "            for (r, c) in ship.cells() {\n"
        "                if r < GRID_SIZE && c < GRID_SIZE {",
        "            for (r, c) in ship.cells().into_iter().take(1) {\n"
        "                if r < GRID_SIZE && c < GRID_SIZE {",
        ["the_ship_being_placed_is_previewed_over_the_cells_it_would_take"],
    ),
    (
        "a placement that would be refused is previewed as if it were fine",
        "            let tint = if self.is_placement_valid() {",
        "            let tint = if true {",
        ["a_placement_that_would_be_refused_is_previewed_in_the_refusing_colour"],
    ),
    (
        "the preview outlives the last ship there is to place",
        "        if self.phase == GamePhase::Placement\n            && let Some(ship) = self.placement_preview_ship()",
        "        if let Some(ship) = self\n            .placement_preview_ship()\n            .or(Some(Ship {\n                kind: ShipKind::Destroyer,\n                row: 0,\n                col: 0,\n                orientation: Orientation::Horizontal,\n            }))",
        ["the_preview_is_gone_once_the_last_ship_is_placed"],
    ),
    (
        "a miss is marked in the colour of a hit",
        "                    color: BLUE,",
        "                    color: RED,",
        ["a_hit_is_a_cross_and_a_miss_is_a_dot"],
    ),
    (
        "the opponent's ships are shown before the game is over",
        "        let reveal = self.phase == GamePhase::GameOver;",
        "        let reveal = true;",
        ["the_opponent_s_ships_are_hidden_until_the_game_is_over"],
    ),
    (
        "a sunk enemy ship is left looking like open water",
        "                let colour = if self.opponent_fleet.is_cell_sunk(r, c) {\n                    OVERLAY0",
        "                let colour = if false {\n                    OVERLAY0",
        ["a_sunk_enemy_ship_is_shown_while_the_battle_is_still_on"],
    ),
    (
        "the stats count the player's shots as the AI's",
        "                (\n                    format!(\"AI Shots: {}\", self.ai_state.shots),",
        "                (\n                    format!(\"AI Shots: {}\", self.player_shots),",
        ["the_stats_say_what_the_game_recorded"],
    ),
    (
        "the hit rate is the count of hits, not a percentage of the shots",
        "fn percent(part: usize, whole: usize) -> f32 {",
        "fn percent(part: usize, whole: usize) -> f32 {\n    if true {\n        return f32_from_usize(part);\n    }",
        ["the_stats_say_what_the_game_recorded"],
    ),
    (
        "a hit rate before the first shot divides by nothing",
        "    if whole == 0 {",
        "    if false {",
        ["a_hit_rate_before_the_first_shot_is_zero_and_not_a_division_by_nothing"],
    ),
    (
        "a fleet down to its last ship is not called out",
        "        let fleet_colour = |left: usize| if left <= 1 { RED } else { GREEN };",
        "        let fleet_colour = |left: usize| if false { RED } else { GREEN };",
        ["a_fleet_down_to_its_last_ship_is_said_in_the_colour_of_alarm"],
    ),
    (
        "the help line is the same in every phase",
        "            GamePhase::Firing => {\n                \"Arrows or click: aim  |  Enter, Space or click: fire  |  N: new game\"\n            }",
        "            GamePhase::Firing => {\n                \"Arrows or click: move  |  R: rotate  |  Enter or click: place  |  N: new game\"\n            }",
        ["the_help_line_says_what_the_keys_do_in_this_phase"],
    ),
    (
        "the row labels run from the wrong letter",
        "            let s = String::from(char::from(b'A'.saturating_add(byte(r))));",
        "            let s = String::from(char::from(b'B'.saturating_add(byte(r))));",
        ["both_grids_are_labelled_a_to_j_down_and_one_to_ten_across"],
    ),
    (
        "the column labels are numbered from zero",
        "            let s = format!(\"{}\", c.saturating_add(1));",
        "            let s = format!(\"{c}\");",
        ["both_grids_are_labelled_a_to_j_down_and_one_to_ten_across"],
    ),
    # -- The rules underneath ------------------------------------------
    (
        "the AI forgets which cells it has already fired at",
        "    fn has_fired(&self, row: usize, col: usize) -> bool {",
        "    fn has_fired(&self, row: usize, col: usize) -> bool {\n        let _ = (row, col);\n        return false;\n        #[expect(unreachable_code, reason = \"mutation\")]",
        ["the_ai_never_fires_at_the_same_cell_twice"],
    ),
    (
        "the AI's five ships stand in the same five places on every machine",
        "        Self::with_seed(randrange::seed_from_system(0x4241_5454_4C45_5348))",
        "        Self::with_seed(0xDEAD_BEEF_CAFE_1234)",
        ["a_launch_does_not_place_the_one_fleet_that_was_hardcoded"],
    ),
    (
        "the fallback board is the one the hardcoded seed dealt",
        "        Self::with_seed(randrange::seed_from_system(0x4241_5454_4C45_5348))",
        "        Self::with_seed(randrange::seed_from_system(0xDEAD_BEEF_CAFE_1234))",
        ["a_launch_does_not_place_the_one_fleet_that_was_hardcoded"],
    ),
    (
        "the window says it is something else",
        '        "Battleship".to_string()',
        '        "Boats".to_string()',
        ["the_window_says_what_it_is"],
    ),
    (
        "a key that changed nothing still asks for a redraw",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["the_window_asks_for_a_redraw_only_when_something_changed"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "battleship", timeout=300, only=sys.argv[1:] or None))
