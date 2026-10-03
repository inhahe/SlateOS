"""Mutation test for the pinball table: the pointer, nudging and tilt, the
kept high scores, and the questions and pauses around a game.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Nothing answered the pointer; "tilt" was pressing the flippers fast; the high
scores were five invented ones, never kept; N threw away a game in progress;
and the flippers did not move before the ball was launched (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- the pointer --
    (
        "the table's left half is not a flipper",
        "        f.hit(Target::LeftFlipper",
        "        let _ = (Target::LeftFlipper",
        ["the_whole_game_can_be_played_with_the_pointer"],
    ),
    (
        "the plunger lane is not a plunger",
        "            f.hit(\n                Target::Plunger,",
        "            let _ = (\n                Target::Plunger,",
        ["the_whole_game_can_be_played_with_the_pointer"],
    ),
    (
        "letting go of the pointer does not launch",
        "                Some(Hold::Plunger) => {\n                    self.release_plunger();",
        "                Some(Hold::Plunger) => {",
        ["the_whole_game_can_be_played_with_the_pointer"],
    ),
    (
        "a flipper held by the pointer stays up",
        "                    self.set_flipper(FlipperSide::Left, false);",
        "",
        ["the_whole_game_can_be_played_with_the_pointer"],
    ),
    (
        "the card leaves the table clickable",
        "            // Modal: nothing behind the card can be clicked.\n            f.discard_hits();",
        "",
        ["the_whole_game_can_be_played_with_the_pointer"],
    ),
    (
        "the card lets keys through",
        "        if self.show_help {\n            // The list is up",
        "        if false {\n            // The list is up",
        ["the_list_of_keys_reaches_the_window_and_holds_the_keyboard"],
    ),
    (
        "Nudge can be pressed with nothing to nudge",
        "            self.phase == GamePhase::Playing && !self.tilt.tilted,",
        "            true,",
        ["a_button_that_would_do_nothing_takes_no_press"],
    ),
    (
        "the table is drawn in the corner of a larger window",
        "            dx: ((w - WINDOW_WIDTH * scale) / 2.0).max(0.0),",
        "            dx: 0.0,",
        ["a_larger_window_gets_a_larger_table"],
    ),
    # -- asking before a game is thrown away --
    (
        "N throws the game away at once",
        "        if !self.game_in_progress() || self.confirm_new_game {",
        "        if true {",
        ["test_new_game_resets_score", "escape_keeps_the_game_going"],
    ),
    (
        "Escape starts the new game",
        "                    Key::Escape => self.confirm_new_game = false,",
        "                    Key::Escape => self.ask_new_game(),",
        ["escape_keeps_the_game_going"],
    ),
    (
        "the question lets the flippers through",
        "        if self.confirm_new_game {\n            if ke.pressed {",
        "        if false {\n            if ke.pressed {",
        ["escape_keeps_the_game_going"],
    ),
    (
        "the question does not say what is lost",
        '            &format!("This one ends, at {}.", self.score),',
        '            "This one ends.",',
        ["escape_keeps_the_game_going"],
    ),
    # -- nudging and tilt --
    (
        "a nudge does not move the ball",
        "            ball.vel = ball.vel.add(Vec2::new(sideways, -NUDGE_IMPULSE));",
        "            let _ = sideways;",
        ["a_nudge_shoves_the_ball_up_the_table"],
    ),
    (
        "the third nudge is only a warning",
        "        if self.nudges.len() > TILT_WARNINGS {",
        "        if self.nudges.len() > TILT_WARNINGS + 1 {",
        ["a_nudge_or_two_is_a_warning_and_one_more_tilts"],
    ),
    (
        "old nudges are never forgotten",
        "            .retain(|&t| now.saturating_sub(t) < TILT_WINDOW_MS);",
        "            .retain(|_| true);",
        ["nudges_far_apart_never_tilt"],
    ),
    (
        "a tilted table's flippers still work",
        "        let pressed = pressed && !self.tilt.tilted;",
        "        let pressed = pressed;",
        ["a_tilted_table_has_dead_flippers_and_scores_nothing"],
    ),
    (
        "a tilted table scores",
        "    fn award_points(&mut self, base_points: u32) {\n        if self.tilt.tilted {\n            return;\n        }",
        "    fn award_points(&mut self, base_points: u32) {",
        ["a_tilted_table_has_dead_flippers_and_scores_nothing"],
    ),
    (
        "the next ball is still tilted",
        "        self.launch_power = 0.0;\n        self.tilt.reset();",
        "        self.launch_power = 0.0;",
        ["a_tilted_table_has_dead_flippers_and_scores_nothing"],
    ),
    # -- high scores --
    (
        "a new table invents a high score",
        "            high_scores: Vec::new(),",
        "            high_scores: vec![HighScoreEntry {\n"
        "                score: 10000,\n"
        "                day: String::new(),\n"
        "            }],",
        ["a_new_table_invents_no_high_scores"],
    ),
    (
        "a game that scored nothing is a high score",
        "        if score == 0 {\n            return;\n        }",
        "",
        ["test_low_score_not_inserted"],
    ),
    (
        "a high score is not kept",
        "            return;\n        }\n        self.save_scores();",
        "            return;\n        }",
        ["high_scores_are_kept_and_read_back"],
    ),
    (
        "a table not opened from settings writes",
        "        if !self.persist {\n            return;\n        }",
        "",
        ["a_table_that_is_not_opened_from_settings_writes_nothing"],
    ),
    (
        "a file not read whole is saved over",
        "                self.persist = false;\n                self.scores_note = Some(refused(why));",
        "                self.scores_note = Some(refused(why));",
        ["a_scores_file_that_cannot_be_read_whole_is_left_alone"],
    ),
    (
        "any day is a day",
        "        if !day_ok {",
        "        if false {",
        ["the_scores_file_is_its_own_format_and_nothing_else"],
    ),
    (
        "any first line will do",
        "    if lines.next() != Some(SCORES_HEADER) {",
        "    if lines.next().is_none() {",
        ["the_scores_file_is_its_own_format_and_nothing_else"],
    ),
    (
        "a score is not dated",
        "            day: self.today(),",
        "            day: String::new(),",
        ["a_high_score_is_dated_by_the_clock"],
    ),
    # -- the window and the clock --
    (
        "the flippers move only with the ball in play",
        "            GamePhase::ReadyToLaunch => {\n                self.left_flipper.update(dt);",
        "            GamePhase::ReadyToLaunch => {\n                let _ = dt;",
        ["the_flippers_work_before_the_ball_is_launched"],
    ),
    (
        "losing the keyboard does not pause",
        "                    self.toggle_pause();",
        "                    self.release_all();",
        ["losing_the_keyboard_pauses_a_game_and_lets_go_of_the_flippers"],
    ),
    (
        "a held flipper stays up when the window goes away",
        "    fn release_all(&mut self) {\n        self.left_flipper.pressed = false;",
        "    fn release_all(&mut self) {",
        ["losing_the_keyboard_pauses_a_game_and_lets_go_of_the_flippers"],
    ),
    (
        "a half-pulled plunger launches when the window goes away",
        "            // not a shot the player took.\n            self.phase = GamePhase::ReadyToLaunch;\n            self.launch_power = 0.0;",
        "            // not a shot the player took.\n            self.launch_ball();",
        ["losing_the_keyboard_pauses_a_game_and_lets_go_of_the_flippers"],
    ),
    (
        "the clock ticks for a still table",
        "        self.moving().then_some(TICK)",
        "        Some(TICK)",
        ["the_clock_is_asked_for_only_while_something_moves"],
    ),
    (
        "the theme is never taken up",
        "        self.palette = *palette;\n        self.colours = Colours::of(palette);",
        "        let _ = palette;",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "the colours are not rebuilt with the palette",
        "        self.colours = Colours::of(palette);",
        "        let _ = Colours::of(palette);",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "a new game drops the user's colours",
        "        fresh.colours = self.colours;\n",
        "",
        ["a_new_game_keeps_the_users_colours"],
    ),
    (
        "the playing surface is a fixed dark blue",
        "            color: self.palette.mantle,",
        "            color: Color::from_hex(0x252540),",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "the sidebar is a raised grey whatever the look",
        "            .push_surface(cmds, sx, sy, sw, sh, 6.0, Surface::Card);",
        "            .push_surface(cmds, sx, sy, sw, sh, 6.0, Surface::ControlTrack);",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "the overlay's lines have no panel under them",
        "            .push_surface(cmds, card.x, card.y, card.w, card.h, 8.0, Surface::Panel);",
        "            .push_surface(cmds, card.x, card.y, card.w, card.h, 8.0, Surface::Card);",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "the footer's keys are the faintest grey",
        "            // grey is 2.3:1 on a light band.\n            color: self.colours.subtext0,",
        "            // grey is 2.3:1 on a light band.\n            color: self.palette.overlay0,",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "a button that would do nothing looks live",
        "            State {\n                disabled: !enabled,",
        "            State {\n                disabled: false,",
        ["a_button_that_would_do_nothing_is_switched_off"],
    ),
    (
        "the scene is drawn at its design size whatever the window",
        "        let scale = (w / WINDOW_WIDTH).min(h / WINDOW_HEIGHT).max(0.0);",
        "        let scale = 1.0_f32;",
        ["a_larger_window_gets_a_larger_table", "the_whole_scene_shows_in_every_window"],
    ),
    (
        "the scene is fitted to the window's width alone",
        "        let scale = (w / WINDOW_WIDTH).min(h / WINDOW_HEIGHT).max(0.0);",
        "        let scale = (w / WINDOW_WIDTH).max(0.0);",
        ["the_whole_scene_shows_in_every_window"],
    ),
    (
        "a click is read against the scene's own size",
        "            f.hit(*target, fit.rect(*r));",
        "            f.hit(*target, *r);",
        ["a_larger_window_gets_a_larger_table", "a_control_is_clicked_where_it_is_drawn_in_any_window"],
    ),
    (
        "a text keeps its design size",
        "                *font_size = self.len(*font_size);",
        "",
        ["a_fitted_command_is_where_the_fit_says"],
    ),
    (
        "a translation inside the scene is moved twice",
        "                (*dx, *dy) = (self.len(*dx), self.len(*dy));",
        "                (*dx, *dy) = (self.x(*dx), self.y(*dy));",
        ["a_fitted_command_is_where_the_fit_says"],
    ),
    (
        "a line keeps its design width",
        "                *width = self.len(*width);\n            }\n            RenderCommand::PushTranslate",
        "            }\n            RenderCommand::PushTranslate",
        ["a_fitted_command_is_where_the_fit_says"],
    ),
    (
        "a key pressed with a modifier is the table's",
        '        if ke.pressed && !textline::is_plain(ke.modifiers) {',
        '        if false {',
        ['a_key_pressed_with_a_modifier_is_not_the_tables'],
    ),
    (
        "a release with a modifier held is not the table's",
        '        if ke.pressed && !textline::is_plain(ke.modifiers) {',
        '        if !textline::is_plain(ke.modifiers) {',
        ['a_key_pressed_with_a_modifier_is_not_the_tables'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "pinball", timeout=600, only=only))
