"""Mutation test for simon's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Simon is the nineteenth application in this campaign and arrived with the usual
three faults and sixteen of its own: `main` was `let _app = SimonApp::new();`,
`render` was never given the window it drew into (every rectangle in it was a
compile-time constant), and there was no mouse code in the file at all -- for a
game whose entire interface is four big buttons.

Usage:  python -u apps/simon/mutate.py [substring ...]
"""

import re
import subprocess
import sys
from pathlib import Path

SRC = Path(__file__).parent / "src" / "main.rs"
BAK = Path(__file__).parent / "src" / "main.rs.bak"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The grid ──────────────────────────────────────────────────────────
    (
        "grid_pos reads the index down the columns rather than along the rows",
        "    (index / PAD_COLS, index % PAD_COLS)",
        "    (index % PAD_COLS, index / PAD_COLS)",
        ["the_pads_are_numbered_left_to_right_then_top_to_bottom"],
    ),
    (
        "grid_index forgets that the grid has a bottom",
        "    if row >= PAD_ROWS || col >= PAD_COLS {",
        "    if col >= PAD_COLS {",
        ["grid_index_refuses_a_position_off_the_grid"],
    ),
    (
        "grid_index forgets that the grid has a right edge",
        "    if row >= PAD_ROWS || col >= PAD_COLS {",
        "    if row >= PAD_ROWS {",
        ["grid_index_refuses_a_position_off_the_grid"],
    ),
    (
        "grid_index multiplies the column by the row count",
        "    row.checked_mul(PAD_COLS)?.checked_add(col)",
        "    col.checked_mul(PAD_ROWS)?.checked_add(row)",
        ["grid_positions_and_indices_are_inverses"],
    ),
    (
        "from_index always answers with the first colour",
        "        Self::ALL.get(index).copied()",
        "        Self::ALL.first().copied()",
        ["from_index_stops_at_the_last_pad"],
    ),
    (
        "index counts from the far end of the list",
        "        Self::ALL.iter().position(|&c| c == self).unwrap_or(0)",
        "        Self::ALL.iter().rev().position(|&c| c == self).unwrap_or(0)",
        ["a_colours_index_and_the_colour_at_that_index_are_inverses"],
    ),
    # ── The four colours ──────────────────────────────────────────────────
    (
        "a dim red pad is as bright as a lit one",
        "            SimonColor::Red => Color::from_hex(0x8B2240),",
        "            SimonColor::Red => COL_RED,",
        ["a_lit_pad_is_brighter_than_a_dim_one"],
    ),
    (
        "green is dimmed to red's dim shade",
        "            SimonColor::Green => Color::from_hex(0x2D6B3F),",
        "            SimonColor::Green => Color::from_hex(0x8B2240),",
        ["no_two_colours_share_a_label_a_tone_or_a_shade"],
    ),
    (
        "green lights up red",
        "            SimonColor::Green => COL_GREEN,",
        "            SimonColor::Green => COL_RED,",
        ["no_two_colours_share_a_label_a_tone_or_a_shade"],
    ),
    (
        "green is called Red",
        '            SimonColor::Green => "Green",',
        '            SimonColor::Green => "Red",',
        ["no_two_colours_share_a_label_a_tone_or_a_shade"],
    ),
    (
        "green sounds the note red does",
        '            SimonColor::Green => "MID",',
        '            SimonColor::Green => "LOW",',
        ["no_two_colours_share_a_label_a_tone_or_a_shade"],
    ),
    # ── Speed ─────────────────────────────────────────────────────────────
    (
        "the fast flash is slower than the medium one",
        "        Speed::Fast => (300, 150),",
        "        Speed::Fast => (600, 150),",
        ["a_faster_speed_is_faster_in_both_halves"],
    ),
    (
        "the slow gap is shorter than the fast one",
        "        Speed::Slow => (800, 400),",
        "        Speed::Slow => (800, 100),",
        ["a_faster_speed_is_faster_in_both_halves"],
    ),
    (
        "the speed cycle skips medium going up",
        "            Speed::Slow => Speed::Medium,",
        "            Speed::Slow => Speed::Fast,",
        ["the_speed_control_reaches_every_speed_and_comes_back"],
    ),
    (
        "the speed cycle turns back at fast rather than wrapping",
        "            Speed::Fast => Speed::Slow,",
        "            Speed::Fast => Speed::Fast,",
        ["the_speed_control_reaches_every_speed_and_comes_back"],
    ),
    (
        "slow and fast are both called Fast",
        '            Speed::Slow => "Slow",',
        '            Speed::Slow => "Fast",',
        ["no_two_speeds_share_a_label"],
    ),
    # ── The bands ─────────────────────────────────────────────────────────
    (
        "the type is sized for a window much larger than this one",
        "        let font = (h / 34.0).clamp(8.0, 20.0);",
        "        let font = (h / 34.0).clamp(80.0, 200.0);",
        ["every_line_of_type_stays_inside_the_window"],
    ),
    (
        "the padding is free to eat the thing it pads",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 12.0).min(w.min(h) / 4.0);",
        "        let pad = (w.min(h) * 0.02).clamp(2.0, 12.0);",
        ["the_padding_never_eats_what_it_pads"],
    ),
    (
        "the status line is drawn over the header",
        "            Rect::new(0.0, hdr_h, w, st_h)",
        "            Rect::new(0.0, 0.0, w, st_h)",
        ["the_bands_never_overlap_one_another"],
    ),
    (
        "the header is twice as tall as the space reserved for it",
        "            Rect::new(0.0, 0.0, w, hdr_h)",
        "            Rect::new(0.0, 0.0, w, hdr_h * 2.0)",
        ["the_bands_never_overlap_one_another"],
    ),
    (
        "the footer hangs off the bottom of the window",
        "            Rect::new(0.0, h - foot_h, w, foot_h)",
        "            Rect::new(0.0, h, w, foot_h)",
        ["every_band_stays_inside_the_window"],
    ),
    (
        "the grid is centred one and a half times over",
        "        let grid = Rect::new((w - gw) / 2.0, top + pad + (avail_h - gh) / 2.0, gw, gh);",
        "        let grid = Rect::new((w - gw) / 2.0, top + pad + (avail_h - gh) * 1.5, gw, gh);",
        ["every_band_stays_inside_the_window"],
    ),
    (
        "the help sheet is wider than the window",
        "        let help_w = (w * 0.92).min(420.0);",
        "        let help_w = (w * 1.92).min(420.0);",
        ["every_band_stays_inside_the_window"],
    ),
    (
        "the help sheet is taller than the window",
        "        let help_h = (h * 0.92).min(300.0);",
        "        let help_h = (h * 1.92).min(300.0);",
        ["every_band_stays_inside_the_window"],
    ),
    (
        "the bands are given up in the wrong order",
        "const BAND_DROP_ORDER: [usize; 3] = [1, 0, 2];",
        "const BAND_DROP_ORDER: [usize; 3] = [2, 1, 0];",
        ["the_bands_are_given_up_in_the_documented_order"],
    ),
    (
        "the header is given up before the status line",
        "const BAND_DROP_ORDER: [usize; 3] = [1, 0, 2];",
        "const BAND_DROP_ORDER: [usize; 3] = [0, 1, 2];",
        ["the_bands_are_given_up_in_the_documented_order"],
    ),
    (
        "the pads are promised a twentieth of the window instead of half",
        "const PAD_SHARE: f32 = 0.45;",
        "const PAD_SHARE: f32 = 0.05;",
        # The test writes 0.45 out as a literal. Reading `PAD_SHARE` back, as it
        # first did, made it agree with itself and survive this.
        ["the_pads_keep_their_share_of_the_window"],
    ),
    (
        "the bands are budgeted the whole window",
        "        let budget = (h - h * PAD_SHARE - pad * 2.0).max(0.0);",
        "        let budget = h;",
        ["the_pads_keep_their_share_of_the_window"],
    ),
    (
        "the drop ladder stops when the bands do not fit rather than when they do",
        "            if wants.iter().sum::<f32>() <= budget {",
        "            if wants.iter().sum::<f32>() >= budget {",
        ["the_pads_keep_their_share_of_the_window"],
    ),
    (
        "the pads are sized by the window's width alone",
        "        let step = (avail_w / PAD_COLS as f32)\n            .min(avail_h / PAD_ROWS as f32)\n            .max(0.0);",
        "        let step = (avail_w / PAD_COLS as f32).max(0.0);",
        ["the_grid_sits_between_the_bands_that_survived"],
    ),
    (
        "the grid takes the padding at the top without giving it back at the bottom",
        "        let avail_h = (bottom - top - pad * 2.0).max(0.0);",
        "        let avail_h = (bottom - top).max(0.0);",
        ["the_grid_sits_between_the_bands_that_survived"],
    ),
    # ── Rows of buttons ───────────────────────────────────────────────────
    (
        "the footer buttons are spaced with no gap between them",
        "        let bw = ((row.w - gap * (n + 1.0)) / n).max(0.0);",
        "        let bw = (row.w / n).max(0.0);",
        ["the_footer_buttons_fill_the_footer_without_overlapping"],
    ),
    (
        "every footer button is drawn in the same place",
        "            row.x + gap + index as f32 * (bw + gap),",
        "            row.x + gap,",
        ["the_footer_buttons_fill_the_footer_without_overlapping"],
    ),
    (
        "a footer button is taller than the footer",
        "        let bh = (row.h * 0.74).max(0.0);",
        "        let bh = (row.h * 1.74).max(0.0);",
        ["the_footer_buttons_fill_the_footer_without_overlapping"],
    ),
    (
        "a fourth footer button is laid out past the third",
        "        if index >= count {\n            return Rect::EMPTY;\n        }",
        "        if index >= count.saturating_add(1) {\n            return Rect::EMPTY;\n        }",
        ["footer_button_refuses_an_index_past_the_last_one"],
    ),
    # ── The header readouts ───────────────────────────────────────────────
    (
        "the readouts are laid out from the left edge outwards",
        "        let x = right - (bw + gap) * (index as f32 + 1.0) + gap;",
        "        let x = self.header.x + self.pad + (bw + gap) * index as f32;",
        ["the_readouts_are_laid_out_from_the_right_edge_inwards"],
    ),
    (
        "a readout too wide for the header is drawn off the left of it",
        "        if x < self.header.x {\n            return Rect::EMPTY;\n        }",
        "        if false {\n            return Rect::EMPTY;\n        }",
        ["a_header_too_narrow_for_a_readout_drops_it_rather_than_squeezing_it"],
    ),
    (
        "a fourth readout is laid out past the third",
        "        if index >= 3 {\n            return Rect::EMPTY;\n        }",
        "        if index >= 4 {\n            return Rect::EMPTY;\n        }",
        ["score_box_refuses_an_index_past_the_last_one"],
    ),
    (
        "the readouts are stacked on top of one another",
        "        let x = right - (bw + gap) * (index as f32 + 1.0) + gap;",
        "        let x = right - gap * (index as f32 + 1.0) + gap;",
        ["the_readouts_line_up_inside_the_header_without_overlapping"],
    ),
    (
        "a readout is taller than the header it sits in",
        "        let bh = (self.header.h * 0.7).max(1.0);",
        "        let bh = (self.header.h * 1.7).max(1.0);",
        ["the_readouts_line_up_inside_the_header_without_overlapping"],
    ),
    # ── The pads ──────────────────────────────────────────────────────────
    (
        "the gap between pads is added to them rather than taken out",
        "        let gap = (self.step * 0.08).min(16.0);",
        "        let gap = -(self.step * 0.08).min(16.0);",
        ["no_two_pads_overlap"],
    ),
    (
        "the pads are laid out down the columns rather than along the rows",
        "            self.grid.x + col as f32 * self.step + gap / 2.0,\n            self.grid.y + row as f32 * self.step + gap / 2.0,",
        "            self.grid.x + row as f32 * self.step + gap / 2.0,\n            self.grid.y + col as f32 * self.step + gap / 2.0,",
        ["the_pads_are_laid_out_in_the_order_their_numbers_say"],
    ),
    (
        "a pad is wider than it is tall",
        "            (self.step - gap).max(0.0),\n            (self.step - gap).max(0.0),",
        "            (self.step - gap).max(0.0) * 1.4,\n            (self.step - gap).max(0.0),",
        ["the_pads_are_square_and_inside_the_grid"],
    ),
    (
        "a fifth pad is laid out past the fourth",
        "        if self.grid.is_empty() || index >= PAD_COLS * PAD_ROWS {",
        "        if self.grid.is_empty() {",
        ["pad_rect_refuses_an_index_off_the_grid"],
    ),
    (
        "the game-over panel is wider than the window",
        "        let bw = (self.grid.w * 0.94).min(self.window.w);",
        "        let bw = self.grid.w * 9.4;",
        ["every_band_stays_inside_the_window"],
    ),
    (
        "the game-over panel is laid out from the grid's centre rather than round it",
        "        Rect::new(cx - bw / 2.0, cy - bh / 2.0, bw, bh)",
        "        Rect::new(cx, cy, bw, bh)",
        ["every_band_stays_inside_the_window"],
    ),
    # ── Drawing helpers ───────────────────────────────────────────────────
    (
        "type below a pixel is drawn anyway, and the renderer rounds it up",
        "    if body.is_empty() || size < MIN_DRAWN_FONT || max_width.is_some_and(|w| w <= 0.0) {",
        "    if body.is_empty() || max_width.is_some_and(|w| w <= 0.0) {",
        ["a_window_with_no_room_for_words_shows_none"],
    ),
    (
        "the floor under the font size is dropped to nothing",
        "const MIN_DRAWN_FONT: f32 = 1.0;",
        "const MIN_DRAWN_FONT: f32 = 0.0;",
        ["a_window_with_no_room_for_words_shows_none"],
    ),
    (
        "a band with no height counts as showing",
        "        band.w > 0.0 && band.h > 0.0",
        "        band.w >= 0.0 && band.h >= 0.0",
        ["every_line_of_type_stays_inside_the_window"],
    ),
    (
        "a line is centred to the left of the box it is in",
        "        r.x + (r.w - tw).max(0.0) / 2.0,",
        "        r.x,",
        ["a_pads_name_is_centred_on_it_and_its_number_is_in_the_corner"],
    ),
    (
        "a line is centred against the top of the box it is in",
        "        r.y + (r.h - th).max(0.0) / 2.0,",
        "        r.y,",
        ["a_pads_name_is_centred_on_it_and_its_number_is_in_the_corner"],
    ),
    (
        "a pad's number is written flush into its corner",
        "            let inset = (r.w * 0.06).min(10.0);",
        "            let inset = 0.0;",
        ["a_pads_name_is_centred_on_it_and_its_number_is_in_the_corner"],
    ),
    # ── The rules of the game ─────────────────────────────────────────────
    (
        "a new window has nothing to repeat",
        "        game.deal_round();\n        game",
        "        game",
        ["a_new_game_has_one_colour_to_repeat"],
    ),
    (
        "the round number runs one ahead of the sequence",
        "        self.sequence.len()",
        "        self.sequence.len() + 1",
        ["the_round_number_is_the_length_of_the_sequence"],
    ),
    (
        "a new game keeps the sequence it was losing",
        "        self.sequence.clear();\n        self.score = 0;",
        "        self.score = 0;",
        ["a_new_game_keeps_the_best_and_the_losses_and_drops_the_score"],
    ),
    (
        "a new game keeps the score it was on",
        "        self.sequence.clear();\n        self.score = 0;",
        "        self.sequence.clear();",
        ["a_new_game_keeps_the_best_and_the_losses_and_drops_the_score"],
    ),
    (
        # `new_game` no longer assigns `flash = None` -- the pad going out at a
        # restart is derived from the state, so this mutates the derivation.
        "the pad the player lost on stays lit into the next game",
        "            GameState::ShowSequence | GameState::PreSequence => None,",
        "            GameState::ShowSequence => None,\n            GameState::PreSequence => self.flash.map(|(c, _)| c),",
        [
            "the_pad_the_player_lost_on_goes_out",
            "the_players_turn_always_begins_at_the_first_step_with_nothing_lit",
        ],
    ),
    (
        "starting a game counts as losing one",
        "        self.sequence.clear();\n        self.score = 0;",
        "        self.sequence.clear();\n        self.score = 0;\n        self.games_lost = self.games_lost.saturating_add(1);",
        ["only_a_wrong_pad_counts_as_a_loss"],
    ),
    (
        "the sequence is the same colour for ever",
        "        let index = self.rng.below(SimonColor::ALL.len());",
        "        let index = self.rng.below(1);",
        ["the_sequence_is_not_the_same_four_colours_for_ever"],
    ),
    (
        # The `max(1)` floor `flash_pad` used to carry is gone: it was defended
        # by a comment claiming `age_flash` never clears a zero, which was false.
        # These two mutate the arm that is actually load-bearing.
        "a flash with no time left on it stays lit",
        "            Some(0) | None => None,",
        "            None => None,\n            Some(0) => Some((color, 0)),",
        ["a_flash_with_no_time_left_on_it_goes_out_on_the_next_tick"],
    ),
    (
        "a flash goes out a tick before it should",
        "            Some(0) | None => None,\n            Some(rest) => Some((color, rest)),",
        "            Some(0) | Some(1) | None => None,\n            Some(rest) => Some((color, rest)),",
        ["a_flash_with_no_time_left_on_it_goes_out_on_the_next_tick"],
    ),
    (
        "a press flash never runs out",
        "        self.flash = match left.checked_sub(elapsed) {",
        "        self.flash = match left.checked_sub(0) {",
        ["a_press_flash_runs_out_on_its_own"],
    ),
    (
        "the flash is not run down at all",
        "        self.age_flash(elapsed);",
        "        self.age_flash(0);",
        ["a_press_flash_runs_out_on_its_own"],
    ),
    (
        "the best is never raised",
        "        self.best = self.best.max(self.score);",
        "        self.best = self.best;",
        ["the_best_rises_with_the_score_and_stays_up"],
    ),
    (
        "the best follows the score down as well as up",
        "        self.best = self.best.max(self.score);",
        "        self.best = self.score;",
        ["the_best_and_the_score_are_two_numbers_that_can_disagree"],
    ),
    (
        "a completed round does not raise the score",
        "        self.score = self.score.saturating_add(1);",
        "        self.score = self.score;",
        ["the_score_is_the_number_of_rounds_completed"],
    ),
    (
        "a press out of step indexes past the end of the sequence",
        "        let Some(&expected) = self.sequence.get(self.player_index) else {\n            return;\n        };",
        "        let expected = self.sequence[self.player_index];",
        ["a_press_that_arrives_out_of_step_does_nothing_rather_than_ending_the_process"],
    ),
    (
        "every pad is the right one",
        "        if color != expected {",
        "        if false {",
        ["the_wrong_pad_ends_the_game_and_counts_a_loss"],
    ),
    (
        "every pad is the wrong one",
        "        if color != expected {",
        "        if color == expected {",
        ["the_right_pad_advances_the_player_through_the_sequence"],
    ),
    (
        "the wrong pad flashes for as long as a right one",
        "            self.flash_pad(color, ERROR_FLASH_MS);",
        "            self.flash_pad(color, PLAYER_FLASH_MS);",
        ["the_wrong_pad_stays_lit_longer_than_a_right_one"],
    ),
    (
        "a lost game is not counted",
        "            self.games_lost = self.games_lost.saturating_add(1);",
        "            self.games_lost = self.games_lost;",
        ["the_wrong_pad_ends_the_game_and_counts_a_loss"],
    ),
    (
        "a right pad does not move the player on",
        "        self.player_index = self.player_index.saturating_add(1);",
        "        self.player_index = self.player_index;",
        ["the_right_pad_advances_the_player_through_the_sequence"],
    ),
    (
        "the round is scored one pad early",
        "        if self.player_index >= self.sequence.len() {",
        "        if self.player_index >= self.sequence.len().saturating_sub(1) {",
        ["the_right_pad_advances_the_player_through_the_sequence"],
    ),
    (
        "a pressed pad does not take the outline with it",
        "        self.selected = index;\n        self.show_selection = true;",
        "        self.show_selection = true;",
        ["the_outline_follows_a_pad_pressed_by_any_route"],
    ),
    (
        "a pressed pad leaves the outline hidden",
        "        self.selected = index;\n        self.show_selection = true;",
        "        self.selected = index;",
        ["the_outline_follows_a_pad_pressed_by_any_route"],
    ),
    (
        "a pad pressed out of turn is played as a move",
        "        if self.state == GameState::PlayerInput {\n            self.press_colour(color);\n        }",
        "        self.press_colour(color);",
        ["a_pad_pressed_out_of_turn_moves_the_outline_and_nothing_else"],
    ),
    (
        "an arrow key does not move the outline",
        "            Some(index) => {\n                self.selected = index;",
        "            Some(index) => {\n                let _ = index;",
        ["the_arrows_walk_the_outline_round_the_grid"],
    ),
    (
        "an arrow into the wall does not reveal the outline",
        "        let revealed = !self.show_selection;\n        self.show_selection = true;",
        "        let revealed = !self.show_selection;",
        ["an_arrow_into_the_wall_reveals_the_outline_once_and_then_does_nothing"],
    ),
    (
        "an arrow into the wall is consumed every time",
        "            None if revealed => EventResult::Consumed,\n            None => EventResult::Ignored,",
        "            None => EventResult::Consumed,",
        ["an_arrow_into_the_wall_reveals_the_outline_once_and_then_does_nothing"],
    ),
    (
        "the speed control does not change the speed",
        "        self.speed = speed;",
        "        self.speed = self.speed;",
        ["the_speed_key_cycles_and_the_footer_says_what_it_landed_on"],
    ),
    (
        "asking for the speed already set is reported as a change",
        "        if speed == self.speed {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["asking_for_the_speed_it_is_already_at_changes_nothing"],
    ),
    (
        "a speed change carries the old phase into the new duration",
        "        self.playback.phase_ms = 0;\n        EventResult::Consumed",
        "        EventResult::Consumed",
        ["changing_the_speed_mid_playback_restarts_the_phase_rather_than_skipping_a_colour"],
    ),
    (
        "the help sheet is not modal",
        "        if self.show_help {\n            return match intent {",
        "        if false {\n            return match intent {",
        ["the_sheet_swallows_every_other_control"],
    ),
    (
        "the sheet cannot be shut",
        "                Intent::ToggleHelp | Intent::CloseHelp => {\n                    self.show_help = false;",
        "                Intent::ToggleHelp | Intent::CloseHelp => {",
        ["either_key_shuts_the_sheet"],
    ),
    (
        "closing a sheet that is not up redraws the window",
        "            Intent::CloseHelp => EventResult::Ignored,",
        "            Intent::CloseHelp => EventResult::Consumed,",
        ["escape_with_no_sheet_up_does_nothing_at_all"],
    ),
    (
        "Enter on a lost game presses a pad instead of starting another",
        "                if self.state == GameState::GameOver {\n                    self.new_game();",
        "                if false {\n                    self.new_game();",
        ["enter_on_the_lost_game_starts_another_one"],
    ),
    (
        "Enter presses the first pad rather than the outlined one",
        "                    self.press_pad(self.selected)",
        "                    self.press_pad(0)",
        ["enter_presses_the_pad_the_outline_is_round"],
    ),
    (
        "the help button does not open the sheet",
        "            Intent::ToggleHelp => {\n                self.show_help = true;",
        "            Intent::ToggleHelp => {",
        ["clicking_the_help_button_opens_the_sheet"],
    ),
    # ── The clock ─────────────────────────────────────────────────────────
    (
        "a tick of no time is reported as a change",
        "        if elapsed == 0 || !self.clock_runs() {",
        "        if !self.clock_runs() {",
        ["a_tick_of_no_time_changes_nothing"],
    ),
    (
        "the sheet does not pause the game",
        "        if elapsed == 0 || !self.clock_runs() {",
        "        if elapsed == 0 {",
        ["the_sheet_pauses_the_game"],
    ),
    (
        "the pulse clock does not run",
        "        self.clock_ms = self.clock_ms.wrapping_add(elapsed);",
        "        self.clock_ms = self.clock_ms.wrapping_add(0);",
        ["the_pulse_moves_with_the_clock_and_comes_round_once_a_period"],
    ),
    (
        "the pause before playback never ends",
        "                self.pre_ms = self.pre_ms.saturating_add(elapsed);",
        "                self.pre_ms = self.pre_ms.saturating_add(0);",
        ["the_pause_runs_before_anything_lights_up"],
    ),
    (
        "the pause before playback ends at once",
        "                if let Some(over) = self.pre_ms.checked_sub(PRE_SEQUENCE_MS) {",
        "                if let Some(over) = self.pre_ms.checked_sub(0) {",
        ["the_pause_runs_before_anything_lights_up"],
    ),
    (
        "the pause drops its overshoot on the floor",
        "                    self.begin_playback(over);",
        "                    self.begin_playback(0);",
        ["the_pause_carries_its_overshoot_into_the_first_flash"],
    ),
    (
        "a completed round is celebrated for ever",
        "                self.success_ms = self.success_ms.saturating_add(elapsed);",
        "                self.success_ms = self.success_ms.saturating_add(0);",
        ["a_completed_round_deals_another_after_its_pause"],
    ),
    (
        "a completed round never deals another",
        "                if self.success_ms >= SUCCESS_FLASH_MS {\n                    self.deal_round();\n                }",
        "                if self.success_ms >= SUCCESS_FLASH_MS {}",
        ["a_completed_round_deals_another_after_its_pause"],
    ),
    (
        "the playback does not advance",
        "            GameState::ShowSequence => self.advance_playback(elapsed),",
        "            GameState::ShowSequence => {}",
        ["every_colour_of_the_sequence_is_shown_for_the_same_time"],
    ),
    (
        "the playback phase does not accumulate",
        "        self.playback.phase_ms = self.playback.phase_ms.saturating_add(elapsed);",
        "        self.playback.phase_ms = self.playback.phase_ms.saturating_add(0);",
        ["every_colour_of_the_sequence_is_shown_for_the_same_time"],
    ),
    (
        "the gap between two flashes is lit",
        "            GameState::ShowSequence if self.playback.in_flash => {",
        "            GameState::ShowSequence if !self.playback.in_flash => {",
        ["the_gap_between_two_flashes_is_dark"],
    ),
    (
        "the playback never leaves the first colour",
        "            self.playback.step = self.playback.step.saturating_add(1);",
        "            self.playback.step = self.playback.step;",
        ["every_colour_of_the_sequence_is_shown_for_the_same_time"],
    ),
    (
        "the playback never hands over to the player",
        "            if self.sequence.get(self.playback.step).is_none() {\n                self.state = GameState::PlayerInput;",
        "            if false {\n                self.state = GameState::PlayerInput;",
        ["the_playback_hands_over_to_the_player_at_the_end"],
    ),
    (
        "the pause before playback lights the first pad",
        "            GameState::ShowSequence | GameState::PreSequence => None,",
        "            GameState::ShowSequence | GameState::PreSequence => self.sequence.first().copied(),",
        ["the_pause_runs_before_anything_lights_up"],
    ),
    (
        "the panel appears while the losing pad is still lit",
        "        self.state == GameState::GameOver && self.flash.is_none()",
        "        self.state == GameState::GameOver",
        ["the_game_over_panel_waits_for_the_losing_pad_to_go_out"],
    ),
    (
        "the clock is held while the window waits on a person",
        "                GameState::PlayerInput | GameState::GameOver => self.flash.is_some(),",
        "                GameState::PlayerInput | GameState::GameOver => true,",
        ["the_clock_is_asked_for_only_while_something_is_moving"],
    ),
    (
        "the clock is asked for while the sheet is up",
        "        self.clock_runs()\n            && match self.state {",
        "        !self.clock_runs()\n            && match self.state {",
        ["the_clock_is_asked_for_only_while_something_is_moving"],
    ),
    # ── What the keys and the controls ask for ────────────────────────────
    (
        "a key coming back up is a second press",
        "    if !ev.pressed {\n        return None;\n    }",
        "    if false {\n        return None;\n    }",
        ["a_key_coming_back_up_is_not_a_second_press"],
    ),
    (
        "the desktop's accelerators are the game's controls",
        "    if ev.modifiers.ctrl || ev.modifiers.alt {\n        return None;\n    }",
        "    if false {\n        return None;\n    }",
        ["the_window_keeps_its_ctrl_and_alt_combinations"],
    ),
    (
        "holding shift stops every key working",
        "    if ev.modifiers.ctrl || ev.modifiers.alt {",
        "    if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.shift {",
        ["shift_does_not_stop_a_key_working"],
    ),
    (
        "up and down are the wrong way round",
        "        Key::Up => Some(Intent::Move(Dir::Up)),\n        Key::Down => Some(Intent::Move(Dir::Down)),",
        "        Key::Up => Some(Intent::Move(Dir::Down)),\n        Key::Down => Some(Intent::Move(Dir::Up)),",
        ["the_arrow_keys_move_the_outline_the_way_they_point"],
    ),
    (
        "left and right are the wrong way round",
        "        Key::Left => Some(Intent::Move(Dir::Left)),\n        Key::Right => Some(Intent::Move(Dir::Right)),",
        "        Key::Left => Some(Intent::Move(Dir::Right)),\n        Key::Right => Some(Intent::Move(Dir::Left)),",
        ["the_arrow_keys_move_the_outline_the_way_they_point"],
    ),
    (
        "Space no longer confirms",
        "        Key::Enter | Key::Space => Some(Intent::Confirm),",
        "        Key::Enter => Some(Intent::Confirm),",
        ["the_sweep_of_keys_agrees_with_the_handler"],
    ),
    (
        "N no longer starts a new game",
        "        Key::N => Some(Intent::NewGame),\n",
        "",
        ["the_sweep_of_keys_agrees_with_the_handler"],
    ),
    (
        "every key on the keyboard starts a new game",
        "        _ => None,\n    }\n}\n\n/// What clicking a control asks for.",
        "        _ => Some(Intent::NewGame),\n    }\n}\n\n/// What clicking a control asks for.",
        ["the_sweep_of_keys_agrees_with_the_handler"],
    ),
    (
        "the digits 1 and 2 press each other's pads",
        "        Key::Num1 => Some(Intent::Pad(0)),\n        Key::Num2 => Some(Intent::Pad(1)),",
        "        Key::Num1 => Some(Intent::Pad(1)),\n        Key::Num2 => Some(Intent::Pad(0)),",
        ["the_digits_press_the_pads_they_are_printed_on"],
    ),
    (
        "S starts a new game rather than changing the speed",
        "        Key::S => Some(Intent::CycleSpeed),",
        "        Key::S => Some(Intent::NewGame),",
        ["the_speed_key_cycles_and_the_footer_says_what_it_landed_on"],
    ),
    (
        "Escape opens the sheet rather than only closing it",
        "        Key::Escape => Some(Intent::CloseHelp),",
        "        Key::Escape => Some(Intent::ToggleHelp),",
        ["escape_with_no_sheet_up_does_nothing_at_all"],
    ),
    (
        "clicking a pad presses the first pad whichever was clicked",
        "        Target::Pad(colour) => Intent::Pad(colour.index()),",
        "        Target::Pad(_) => Intent::Pad(0),",
        ["clicking_a_pad_presses_that_colour"],
    ),
    (
        "the new-game button changes the speed",
        "        Target::NewGame => Intent::NewGame,",
        "        Target::NewGame => Intent::CycleSpeed,",
        # Where the three boxes *are* is a different question from what each one
        # does; the layout test only answered the first, and nothing clicked the
        # button, so this survived the first sweep.
        ["each_footer_button_does_the_thing_its_label_names"],
    ),
    (
        "the speed button starts a new game",
        "        Target::Speed => Intent::CycleSpeed,",
        "        Target::Speed => Intent::NewGame,",
        ["clicking_the_speed_button_does_what_the_key_does"],
    ),
    (
        "the help button starts a new game",
        "        Target::Help => Intent::ToggleHelp,",
        "        Target::Help => Intent::NewGame,",
        ["clicking_the_help_button_opens_the_sheet"],
    ),
    (
        "clicking the sheet presses a pad instead of shutting it",
        "        Target::HelpSheet => Intent::CloseHelp,",
        "        Target::HelpSheet => Intent::Pad(0),",
        ["clicking_the_sheet_anywhere_shuts_it"],
    ),
    (
        "clicking the lost game does nothing but close a sheet",
        "        Target::GameOver => Intent::Confirm,",
        "        Target::GameOver => Intent::CloseHelp,",
        ["clicking_the_lost_game_panel_starts_another_one"],
    ),
    (
        "a resize is not passed on to the game",
        "            game.resize(*width as f32, *height as f32);",
        "            let _ = (width, height);",
        ["a_resize_event_is_the_size_the_next_click_is_read_against"],
    ),
    (
        "a tick is not passed on to the game",
        "        Event::Tick { elapsed_ms } => game.tick(*elapsed_ms),",
        "        Event::Tick { elapsed_ms } => {\n            let _ = elapsed_ms;\n            EventResult::Ignored\n        }",
        ["a_tick_event_carries_its_milliseconds_into_the_game"],
    ),
    # ── Clicks and the boxes they land in ─────────────────────────────────
    (
        "every pad records its hit box over the whole grid",
        "            f.hit(Target::Pad(colour), r);",
        "            f.hit(Target::Pad(colour), l.grid);",
        ["no_two_hit_boxes_overlap"],
    ),
    (
        "the pads record no hit boxes at all",
        "            f.hit(Target::Pad(colour), r);\n",
        "",
        ["every_pad_records_one_hit_box_and_it_is_the_pad_that_was_drawn"],
    ),
    (
        "a pad takes the click from the whole window",
        "            f.hit(Target::Pad(colour), r);",
        "            f.hit(Target::Pad(colour), l.window);",
        ["a_click_on_nothing_is_ignored"],
    ),
    (
        "the lost-game panel takes no clicks",
        "        f.hit(Target::GameOver, panel);\n",
        "",
        ["the_panel_takes_the_click_from_the_pad_underneath_it"],
    ),
    (
        "the sheet takes no clicks",
        "        f.hit(Target::HelpSheet, l.window);\n",
        "",
        ["the_sheet_covers_the_window_and_takes_every_click"],
    ),
    (
        "the sheet takes clicks only where the sheet itself is",
        "        f.hit(Target::HelpSheet, l.window);",
        "        f.hit(Target::HelpSheet, l.help);",
        ["the_sheet_covers_the_window_and_takes_every_click"],
    ),
    (
        "the lost-game panel is drawn over the sheet",
        "        if self.game_over_shown() {\n            self.draw_game_over(&mut f, &l);\n        }\n        if self.show_help {\n            self.draw_help(&mut f, &l);\n        }",
        "        if self.show_help {\n            self.draw_help(&mut f, &l);\n        }\n        if self.game_over_shown() {\n            self.draw_game_over(&mut f, &l);\n        }",
        ["a_lost_game_can_still_be_read_about"],
    ),
    (
        "any mouse button presses a pad",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if !matches!(ev.kind, MouseEventKind::Press(_)) {",
        ["only_the_left_button_presses_a_pad"],
    ),
    (
        "a release of the mouse is a second click",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_release_of_the_mouse_is_not_a_second_click"],
    ),
    (
        "a click is read against the size the window opened at",
        "        let frame = self.frame(self.width, self.height);",
        "        let frame = self.frame(WINDOW_WIDTH, WINDOW_HEIGHT);",
        ["a_click_is_read_against_the_size_the_window_was_last_drawn_at"],
    ),
    (
        "a click on nothing starts a new game",
        "            None => EventResult::Ignored,\n        }\n    }\n\n    // ── Drawing",
        "            None => self.apply(Intent::NewGame),\n        }\n    }\n\n    // ── Drawing",
        ["a_click_on_nothing_is_ignored"],
    ),
    (
        "a resize is not remembered",
        "        self.width = width;\n        self.height = height;",
        "        let _ = (width, height);",
        ["a_resize_event_is_the_size_the_next_click_is_read_against"],
    ),
    # ── What the window says ──────────────────────────────────────────────
    (
        "the panel is drawn over the pad the player lost on",
        "        if self.game_over_shown() {",
        "        if self.state == GameState::GameOver {",
        ["the_panel_is_not_drawn_over_the_pad_the_player_lost_on"],
    ),
    (
        "the help sheet is never drawn",
        "        if self.show_help {\n            self.draw_help(&mut f, &l);\n        }",
        "        if false {\n            self.draw_help(&mut f, &l);\n        }",
        ["the_sheet_lists_every_control_and_what_it_does"],
    ),
    (
        "the game says the same thing while watching and while playing",
        '            GameState::PlayerInput => format!(\n                "Your turn  {}/{}",',
        '            GameState::PlayerInput => format!(\n                "Watch  {}/{}",',
        ["no_two_states_share_a_status_line_or_a_colour"],
    ),
    (
        "watching and playing are the same colour",
        "            GameState::PlayerInput => COL_TEAL,",
        "            GameState::PlayerInput => COL_MAUVE,",
        ["no_two_states_share_a_status_line_or_a_colour"],
    ),
    (
        "the watch count runs past the end of the sequence",
        "                self.playback.step.saturating_add(1).min(self.round()),",
        "                self.playback.step.saturating_add(1),",
        ["the_count_never_reads_past_the_end_of_the_sequence"],
    ),
    (
        "the player is counted from zero rather than from one",
        "                self.player_index.saturating_add(1).min(self.round()),",
        "                self.player_index.min(self.round()),",
        ["the_status_line_counts_the_player_through_the_sequence"],
    ),
    (
        "the title is given the whole header and runs under the readouts",
        "        let limit = (self.readouts_left(l) - l.pad * 2.0).max(0.0);",
        "        let limit = l.header.w;",
        ["the_title_never_runs_under_the_readouts"],
    ),
    (
        "the title is sized against the rightmost readout rather than the leftmost",
        "            .fold(l.header.right(), f32::min)",
        "            .fold(l.header.x, f32::max)",
        ["the_title_never_runs_under_the_readouts"],
    ),
    (
        "the header shows the score where the best belongs",
        '        self.draw_readout(f, l, 0, "BEST", self.best, COL_YELLOW);',
        '        self.draw_readout(f, l, 0, "BEST", self.score, COL_YELLOW);',
        ["the_header_shows_the_best_the_score_and_the_round"],
    ),
    (
        "the header shows the round where the score belongs",
        '        self.draw_readout(f, l, 1, "SCORE", self.score, COL_GREEN);',
        '        self.draw_readout(f, l, 1, "SCORE", self.round() as u32, COL_GREEN);',
        ["the_header_shows_the_best_the_score_and_the_round"],
    ),
    (
        "the status dot is grey whatever is lit",
        "        disc(f, dot, lit.map_or(COL_SURFACE1, SimonColor::lit));",
        "        disc(f, dot, COL_SURFACE1);",
        ["the_status_dot_takes_the_colour_of_whatever_is_lit"],
    ),
    (
        # `0.0_f32`, not `0.0`: the bare literal is inferred as `f64` and the
        # arithmetic below it is `f32`, so the mutant did not compile.
        "the pulse stands still",
        "        let phase = (self.clock_ms % PULSE_PERIOD_MS) as f32 / PULSE_PERIOD_MS as f32;",
        "        let phase = 0.0_f32;",
        ["the_pulse_moves_with_the_clock_and_comes_round_once_a_period"],
    ),
    (
        "the dot pulses whether or not a pad is lit",
        "        let side = if lit.is_some() {\n            dot_side * grow\n        } else {\n            dot_side\n        };",
        "        let side = dot_side * grow;",
        ["the_pulse_moves_with_the_clock_and_comes_round_once_a_period"],
    ),
    (
        "a tone is named with nothing lit",
        "        let tone = lit.map(SimonColor::tone);",
        "        let tone = Some(SimonColor::Red.tone());",
        ["the_tone_is_named_only_while_a_pad_is_lit"],
    ),
    (
        "the glow is painted over the pad's face",
        "            if is_lit {\n                let grow = (r.w.min(r.h) * 0.04).min(6.0);",
        "            if false {\n                let grow = (r.w.min(r.h) * 0.04).min(6.0);",
        ["the_glow_goes_behind_the_pad_rather_than_over_its_face"],
    ),
    (
        # The clip that used to stand here was dead -- `grow` can never exceed
        # half the gap `pad_rect` insets a pad by, so the halo cannot leave its
        # own cell. These two attack the bound the deleted clip was hiding: at
        # ten times the growth the halo swallows its neighbours, and at a flat
        # number of pixels it leaves a small window entirely.
        "the glow grows ten times as far",
        "                let grow = (r.w.min(r.h) * 0.04).min(6.0);",
        "                let grow = (r.w.min(r.h) * 0.4).min(60.0);",
        ["the_glow_never_reaches_outside_the_window"],
    ),
    (
        "the glow grows by a flat number of pixels a small window has no room for",
        "                let grow = (r.w.min(r.h) * 0.04).min(6.0);",
        "                let grow = 8.0;",
        ["the_glow_never_reaches_outside_the_window"],
    ),
    (
        "every pad is drawn lit",
        "            let is_lit = lit == Some(colour);",
        "            let is_lit = true;",
        ["only_the_lit_pad_is_drawn_lit"],
    ),
    (
        "the outline is drawn round every pad",
        "            if self.show_selection && index == self.selected {",
        "            if self.show_selection {",
        ["the_outline_is_drawn_round_the_pad_it_names_and_no_other"],
    ),
    (
        "a new window shows an outline nobody asked for",
        "            if self.show_selection && index == self.selected {",
        "            if index == self.selected {",
        ["a_new_window_shows_no_outline"],
    ),
    (
        "the speed button does not say which speed it is on",
        '            &format!("Speed: {}", self.speed.label()),',
        '            &format!("Speed"),',
        ["the_speed_key_cycles_and_the_footer_says_what_it_landed_on"],
    ),
    (
        "two footer buttons are drawn in the same place",
        "            l.footer_button(0),\n            Target::NewGame,",
        "            l.footer_button(1),\n            Target::NewGame,",
        ["no_two_hit_boxes_overlap"],
    ),
    (
        "the panel reports the best where the score belongs",
        '                format!("Score: {} rounds", self.score),',
        '                format!("Score: {} rounds", self.best),',
        ["the_panel_reports_the_score_the_best_and_the_losses"],
    ),
    (
        "the panel reports the score where the losses belong",
        '                format!("Games lost: {}", self.games_lost),',
        '                format!("Games lost: {}", self.score),',
        ["the_panel_reports_the_score_the_best_and_the_losses"],
    ),
    (
        "the panel's rows are stacked on the same line",
        "            let r = Rect::new(panel.x, panel.y + i as f32 * row_h, panel.w, row_h);",
        "            let r = Rect::new(panel.x, panel.y, panel.w, row_h);",
        ["the_panels_rows_do_not_overwrite_one_another"],
    ),
    (
        "the panel's heading is taller than the row it is in",
        "                (l.font * 1.2).min(row_h * 0.7)",
        "                l.font * 12.0",
        ["the_panels_rows_do_not_overwrite_one_another"],
    ),
    (
        "the sheet is laid out for fewer rows than it draws",
        "        let rows = HELP_ROWS.len() as f32 + 2.0;",
        "        let rows = HELP_ROWS.len() as f32;",
        ["the_sheets_rows_do_not_overwrite_one_another"],
    ),
    (
        "the sheet's rows are drawn over its title",
        "            let y = l.help.y + (i as f32 + 1.0) * row_h;",
        "            let y = l.help.y + i as f32 * row_h;",
        ["the_sheets_rows_do_not_overwrite_one_another"],
    ),
    (
        "the sheet's type is not shrunk to its rows",
        "        let size = (l.small).min(row_h * 0.6);",
        "        let size = l.small.max(row_h);",
        ["the_sheets_rows_do_not_overwrite_one_another"],
    ),
    (
        "the sheet's closing line is drawn over its last row",
        "            Rect::new(l.help.x, l.help.bottom() - row_h, l.help.w, row_h),",
        "            Rect::new(l.help.x, l.help.bottom() - row_h * 2.0, l.help.w, row_h),",
        ["the_sheets_rows_do_not_overwrite_one_another"],
    ),
    (
        "the sheet's two columns are drawn on top of each other",
        "        let key_w = l.help.w * 0.42;",
        "        let key_w = 0.0;",
        ["a_rows_meaning_is_written_to_the_right_of_the_key_it_explains"],
    ),
    (
        "the sheet's keys are written off the left of it",
        "                l.help.x + l.pad * 2.0,",
        "                l.help.x - l.pad * 2.0,",
        ["a_rows_meaning_is_written_to_the_right_of_the_key_it_explains"],
    ),
    (
        # The row is rewritten rather than removed: `HELP_ROWS` is declared
        # `[(&str, &str); 6]`, so deleting an element does not compile and the
        # mutation tests the type annotation instead of the sheet. Naming a key
        # the game does not have is the same fault -- a sheet that does not
        # describe the keyboard -- and it does compile.
        "the sheet names a key the game does not have",
        '    ("H / Escape", "show or hide this"),',
        '    ("F1", "show or hide this"),',
        ["no_key_that_does_something_is_missing_from_the_sheet"],
    ),
    # ── The window itself ─────────────────────────────────────────────────
    (
        "the window has no name for the taskbar",
        '        "Simon".to_string()\n    }\n\n    fn app_id(&self) -> String {',
        '        String::new()\n    }\n\n    fn app_id(&self) -> String {',
        ["the_window_is_named_for_the_desktop_and_for_the_person"],
    ),
    (
        "the window's id is the name a person reads",
        '        "simon".to_string()\n    }\n\n    fn initial_size',
        '        "Simon".to_string()\n    }\n\n    fn initial_size',
        ["the_window_is_named_for_the_desktop_and_for_the_person"],
    ),
    (
        "the window opens at a size its layout was not written for",
        "    fn initial_size(&self) -> (u32, u32) {\n        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "    fn initial_size(&self) -> (u32, u32) {\n        (200, 200)",
        ["the_window_opens_at_the_size_its_layout_was_written_for"],
    ),
    (
        "the window holds a timer even when nothing is moving",
        "        self.wants_clock().then_some(TICK)",
        "        Some(TICK)",
        ["the_timer_the_window_asks_for_is_the_one_the_game_wants"],
    ),
    (
        "the window never asks for a timer",
        "        self.wants_clock().then_some(TICK)",
        "        None",
        ["the_timer_the_window_asks_for_is_the_one_the_game_wants"],
    ),
    (
        "the close button does not close the window",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        if false {\n            return Response::Exit;\n        }",
        ["the_close_button_closes_the_window"],
    ),
    (
        "an event that changed nothing asks for a frame",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        ["an_event_that_changes_nothing_does_not_ask_for_a_frame"],
    ),
    (
        "an event that changed something does not ask for a frame",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        ["an_event_that_changes_something_asks_for_a_frame"],
    ),
    (
        "drawing the window forgets the size it was drawn at",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["drawing_the_window_remembers_the_size_it_was_drawn_at"],
    ),
    (
        "the window is drawn at the size it opened at whatever size it is",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.resize(width, height);\n        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()",
        ["the_drawn_tree_carries_the_commands_the_frame_recorded"],
    ),
    (
        "the probe draws a window with its sides swapped",
        "        self.frame(size.0, size.1)",
        "        self.frame(size.1, size.0)",
        ["the_probe_draws_the_same_window_the_compositor_gets"],
    ),
    (
        "the probe's default size is the window's, sideways",
        "    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);",
        "    const SIZE: (f32, f32) = (WINDOW_HEIGHT, WINDOW_WIDTH);",
        ["the_probe_draws_the_same_window_the_compositor_gets"],
    ),
    (
        "the frame is left with a clip nobody closed",
        "        fill(&mut f, l.window, COL_BASE, 0.0);",
        "        fill(&mut f, l.window, COL_BASE, 0.0);\n        f.clip(l.window);",
        ["the_frame_is_balanced_at_every_size_and_in_every_state"],
    ),
    # ── Deliberate redundancy probes ──────────────────────────────────────
    # Each of these is expected to SURVIVE.  A line that can be deleted with no
    # test noticing is either a line no test owns -- a gap in the suite -- or a
    # guard standing in front of a rule that already holds, which is a line that
    # should not be there at all (`known-issues.md` lesson 51).  The verdict
    # says which: if the behaviour it claims to protect is genuinely reachable,
    # the suite is short a test; if it is not, the production line goes.
    #
    # The first sweep ran five of these.  All five survived, and all five were
    # right to: `begin_playback`'s second `Playback::new()`, the handover's
    # `player_index = 0` and `flash = None`, `new_game`'s `flash = None`, and
    # `draw_readout`'s empty-box guard have all been deleted from the production
    # code rather than covered by a new test, because in each case the rule they
    # restated already held.  What is left below is the one reset that is not a
    # restatement, plus the mutations that now own the properties the deleted
    # lines were aiming at.
    (
        "PROBE: deal_round does not reset the playback",
        "        self.pre_ms = 0;\n        self.success_ms = 0;\n        self.playback = Playback::new();",
        "        self.pre_ms = 0;\n        self.success_ms = 0;",
        ["the_pause_runs_before_anything_lights_up"],
    ),
    (
        # `deal_round` is now the only place `player_index` is rewound -- the
        # handover's copy is gone -- so this is what holds the rewind.
        "the next round starts part-way through the sequence",
        "        self.player_index = 0;\n        self.state = GameState::PreSequence;",
        "        self.state = GameState::PreSequence;",
        ["the_players_turn_always_begins_at_the_first_step_with_nothing_lit"],
    ),
    (
        # What actually puts the pad out before the handover, now that the
        # handover does not clear the flash itself: the clock runs the press
        # flash down through the celebration and the pause. Stop running it in
        # those two states and the player's last press is still lit when the
        # machine hands the next round back to them.
        "the press flash is only run down while the player is pressing",
        "        self.age_flash(elapsed);",
        "        if self.state == GameState::PlayerInput {\n            self.age_flash(elapsed);\n        }",
        [
            "the_players_turn_always_begins_at_the_first_step_with_nothing_lit",
            "the_pad_the_player_lost_on_goes_out",
        ],
    ),
    # ── END OF LIST ───────────────────────────────────────────────────────
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
            "simon",
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
    # nobody noticed, which is the opposite of the truth: the hang IS the
    # symptom.  Same for a mutant that aborts before any test can report.
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
