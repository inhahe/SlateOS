"""Mutation test for the asteroids suite.

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
    # -- Layout: the bands --------------------------------------------
    (
        "the header is given a share of the width instead of the height",
        "        let header_h = (h * 0.08).clamp(20.0, 56.0).min(h);",
        "        let header_h = (w * 0.08).clamp(20.0, 56.0).min(h);",
        # Not `a_taller_window_gives_the_playfield_the_extra_room`, which this
        # row named at first and which does not fail: it holds the width fixed,
        # and a header measured against a fixed width does not change, so the
        # taller window's extra pixels still all go to the playfield. The
        # window has to get *wider* for the fault to show.
        ["the_header_is_a_share_of_the_height_not_the_width"],
    ),
    (
        "the header is allowed to be taller than the window",
        "        let header_h = (h * 0.08).clamp(20.0, 56.0).min(h);",
        "        let header_h = (h * 0.08).clamp(20.0, 56.0);",
        # Not `no_band_is_ever_drawn_inside_out`, which is what this row named
        # at first. Dropping the `.min(h)` does not turn a band backwards -- it
        # makes the header *overhang* a window shorter than the clamp's floor
        # of 20, and an overhanging band still has a positive height. The
        # symptom is the one the other test owns: a band that left the window.
        ["the_bands_do_not_overlap_and_stay_inside_the_window"],
    ),
    (
        "the body is measured from the top of the window, not the header",
        "        let body_y = (header.bottom() + pad).min(h);",
        "        let body_y = pad;",
        ["the_bands_do_not_overlap_and_stay_inside_the_window"],
    ),
    (
        "the body is allowed to start past the bottom of the window",
        "        let body_y = (header.bottom() + pad).min(h);",
        "        let body_y = header.bottom() + pad;",
        ["the_bands_do_not_overlap_and_stay_inside_the_window"],
    ),
    (
        "the body is allowed a negative height",
        "                (h - body_y - pad).max(0.0),",
        "                h - body_y - pad,",
        ["no_band_is_ever_drawn_inside_out"],
    ),
    (
        "the header is allowed a negative width",
        "            (w - pad * 2.0).max(0.0),\n            (header_h - pad).max(0.0),",
        "            w - pad * 2.0,\n            (header_h - pad).max(0.0),",
        ["no_band_is_ever_drawn_inside_out"],
    ),
    (
        "a negative window is taken at its word",
        "        let w = w.max(0.0);\n        let h = h.max(0.0);",
        "        let w = w;\n        let h = h;",
        ["a_negative_window_is_read_as_no_window"],
    ),
    # -- Field: fitting the world into the window ----------------------
    (
        "the field is stretched to the window instead of fitted to it",
        "        let scale = (area.w / FIELD_WIDTH).min(area.h / FIELD_HEIGHT).max(0.0);\n"
        "        let w = FIELD_WIDTH * scale;\n"
        "        let h = FIELD_HEIGHT * scale;",
        "        let scale = (area.w / FIELD_WIDTH).min(area.h / FIELD_HEIGHT).max(0.0);\n"
        "        let w = area.w;\n"
        "        let h = area.h;",
        [
            "the_field_keeps_the_worlds_proportions_whatever_the_window",
            "a_wide_window_letterboxes_the_field_rather_than_stretching_it",
            "a_tall_window_letterboxes_the_field_above_and_below",
            "the_corners_of_the_world_land_on_the_corners_of_the_field",
        ],
    ),
    (
        "the field takes the larger of the two fits rather than the smaller",
        "        let scale = (area.w / FIELD_WIDTH).min(area.h / FIELD_HEIGHT).max(0.0);",
        "        let scale = (area.w / FIELD_WIDTH).max(area.h / FIELD_HEIGHT).max(0.0);",
        [
            "a_wide_window_letterboxes_the_field_rather_than_stretching_it",
            "a_tall_window_letterboxes_the_field_above_and_below",
        ],
    ),
    (
        "the field is put in the corner rather than the middle",
        "            rect: Rect::new(\n                area.x + (area.w - w) / 2.0,\n"
        "                area.y + (area.h - h) / 2.0,",
        "            rect: Rect::new(\n                area.x,\n                area.y,",
        [
            "a_wide_window_letterboxes_the_field_rather_than_stretching_it",
            "a_tall_window_letterboxes_the_field_above_and_below",
        ],
    ),
    (
        "a window with no room gives the field a backwards scale",
        "        let scale = (area.w / FIELD_WIDTH).min(area.h / FIELD_HEIGHT).max(0.0);",
        "        let scale = (area.w / FIELD_WIDTH).min(area.h / FIELD_HEIGHT);",
        ["a_window_with_no_room_has_a_field_of_nothing_rather_than_a_backwards_one"],
    ),
    (
        "a position is scaled but not moved to where the field is",
        "        (\n            self.rect.x + p.x * self.scale,\n"
        "            self.rect.y + p.y * self.scale,\n        )",
        "        (p.x * self.scale, p.y * self.scale)",
        [
            "the_corners_of_the_world_land_on_the_corners_of_the_field",
            "every_asteroid_is_drawn_where_the_field_puts_it",
        ],
    ),
    (
        "a position is moved to the field but never scaled",
        "        (\n            self.rect.x + p.x * self.scale,\n"
        "            self.rect.y + p.y * self.scale,\n        )",
        "        (self.rect.x + p.x, self.rect.y + p.y)",
        ["the_corners_of_the_world_land_on_the_corners_of_the_field"],
    ),
    (
        "a length is drawn at its game size whatever the window",
        "    pub fn scaled(&self, v: f32) -> f32 {\n        v * self.scale\n    }",
        "    pub fn scaled(&self, v: f32) -> f32 {\n        v\n    }",
        ["a_bigger_window_draws_the_same_game_bigger"],
    ),
    (
        "a line is allowed to thin away to nothing",
        "        (v * self.scale).max(1.0)",
        "        v * self.scale",
        ["a_line_never_thins_away_to_nothing_in_a_small_window"],
    ),
    # -- The header ----------------------------------------------------
    (
        "the high score is left off the header",
        "            (\n                Target::HighScore,\n"
        '                format!("Hi: {}", self.high_score),\n'
        "                YELLOW,\n            ),",
        "            (Target::HighScore, String::new(), YELLOW),",
        ["the_header_names_every_reading"],
    ),
    (
        "the header reads out a score that is not the score",
        '            (Target::Score, format!("Score: {}", self.score), TEXT_COLOR),',
        '            (Target::Score, String::from("Score: 0"), TEXT_COLOR),',
        ["the_score_on_screen_is_the_score"],
    ),
    (
        "the readings go back to their hardcoded offsets",
        "            let width = text::measure(&value, l.head, weight);\n"
        "            let r = take_left(&mut row, width, l.pad);",
        "            let width = 130.0_f32;\n"
        "            let r = take_left(&mut row, width, l.pad);",
        # The growth test alone, and that is the honest list. The two overlap
        # tests are the ones this row named at first, and neither fails: a
        # fixed 130px really is wide enough for all five readings at the sizes
        # they try, so nothing overlaps and nothing is dropped. Their
        # `box >= measured text` assertion is the right property, but it is a
        # *threshold*, and 130 happens to clear it -- passing by luck, not
        # because the layout measured anything. One constant cannot be two
        # widths, so a test that compares a long reading's box against a short
        # one's has no such loophole, and it is the only owner here.
        ["a_longer_reading_is_given_a_wider_box"],
    ),
    (
        "a reading that will not fit is drawn over its neighbour anyway",
        "    if w <= 0.0 || area.w < w {\n        return Rect::EMPTY;\n    }",
        "    if w <= 0.0 {\n        return Rect::EMPTY;\n    }",
        [
            "a_narrow_window_drops_readings_rather_than_drawing_them_over_each_other",
            "a_reading_that_will_not_fit_is_not_drawn_at_all",
        ],
    ),
    (
        "the row does not advance, so every reading is drawn on the first",
        "    area.x += w + gap;\n    area.w = (area.w - w - gap).max(0.0);",
        "    area.w = (area.w - w - gap).max(0.0);",
        ["the_readings_do_not_overlap_each_other"],
    ),
    (
        "a squeezed header stacks the controls line on top of the score",
        "        let (readings, controls) = if inner.h >= hint_h * 2.0 {",
        "        let (readings, controls) = if inner.h >= 0.0 {",
        ["a_header_with_room_for_one_row_keeps_the_score_and_drops_the_controls_line"],
    ),
    (
        "the readings and the controls line are given the same strip",
        "                Rect::new(inner.x, inner.y, inner.w, inner.h - hint_h),",
        "                Rect::new(inner.x, inner.y, inner.w, inner.h),",
        ["the_readings_do_not_overlap_each_other"],
    ),
    # -- The playfield -------------------------------------------------
    (
        "the playfield is laid out against the window rather than the body",
        "        let field = Field::new(l.body);",
        "        let field = Field::new(l.window);",
        ["the_playfield_is_on_screen_and_inside_the_body"],
    ),
    (
        "the playfield's hit box is recorded last and swallows everything",
        # Moved, not deleted. Written as a deletion this row did fail tests --
        # but the ones that fail when the field has no hit box at all
        # (`a_click_during_play_does_nothing`, which clicks it), not the one
        # that owns the *ordering*. The name is a claim about order, so the
        # mutation has to be a reordering: the hit goes to the end, after the
        # asteroids and the ship, where `hit_test`'s reverse search finds it
        # first and it swallows everything drawn inside it.
        "        f.hit(Target::Field, field.rect);\n\n"
        "        draw_stars(&mut f, &field);\n"
        "        self.draw_particles(&mut f, &field);\n"
        "        self.draw_asteroids(&mut f, &field);\n"
        "        self.draw_bullets(&mut f, &field);\n"
        "        if self.ship_alive {\n"
        "            self.draw_ship(&mut f, &field);\n        }",
        "        draw_stars(&mut f, &field);\n"
        "        self.draw_particles(&mut f, &field);\n"
        "        self.draw_asteroids(&mut f, &field);\n"
        "        self.draw_bullets(&mut f, &field);\n"
        "        if self.ship_alive {\n"
        "            self.draw_ship(&mut f, &field);\n        }\n"
        "        f.hit(Target::Field, field.rect);",
        ["an_asteroid_wins_the_hit_test_over_the_playfield_behind_it"],
    ),
    (
        "an asteroid's hit box is put at its game position, not its screen one",
        "            let r = field.scaled(asteroid.radius());\n"
        "            f.hit(\n                Target::Asteroid(index),\n"
        "                Rect::new(cx - r, cy - r, r * 2.0, r * 2.0),\n            );",
        "            let r = field.scaled(asteroid.radius());\n"
        "            f.hit(\n                Target::Asteroid(index),\n"
        "                Rect::new(\n                    asteroid.pos.x - r,\n"
        "                    asteroid.pos.y - r,\n                    r * 2.0,\n"
        "                    r * 2.0,\n                ),\n            );",
        ["every_asteroid_is_drawn_where_the_field_puts_it"],
    ),
    (
        "a bullet's hit box is put where the game keeps it, not where it is drawn",
        "            let box_ = Rect::new(x - r, y - r, r * 2.0, r * 2.0);",
        "            let box_ = Rect::new(bullet.pos.x - r, bullet.pos.y - r, r * 2.0, r * 2.0);",
        ["a_shot_in_the_air_is_drawn_where_the_field_puts_it"],
    ),
    (
        "a destroyed ship is drawn anyway",
        "        if self.ship_alive {\n            self.draw_ship(&mut f, &field);\n        }",
        "        self.draw_ship(&mut f, &field);",
        ["a_dead_ship_is_not_drawn"],
    ),
    (
        "the blinking ship keeps its hit box on the frames it is invisible",
        "        if self.is_invulnerable() && self.frame_counter % 6 < 3 {\n            return;\n        }",
        "        if false {\n            return;\n        }",
        ["a_blinking_ship_is_off_the_screen_on_the_frames_it_is_not_drawn"],
    ),
    (
        "the ship never comes back from blinking",
        "        if self.is_invulnerable() && self.frame_counter % 6 < 3 {",
        "        if self.is_invulnerable() || self.frame_counter % 6 < 3 {",
        ["the_ship_is_on_screen_once_it_has_stopped_blinking"],
    ),
    # -- The overlays --------------------------------------------------
    (
        "the pause sheet does not offer a new game",
        "            (\n                Target::NewGame,\n"
        '                "Press N for new game",\n'
        "                l.font * 0.85,\n"
        "                FontWeightHint::Regular,\n"
        "                TEAL,\n            ),",
        '            (Target::NewGame, "", l.font * 0.85, FontWeightHint::Regular, TEAL),',
        ["the_pause_sheet_names_both_ways_out"],
    ),
    (
        "the game-over box does not say what wave it ended on",
        "            (\n                Target::FinalStat(2),\n"
        '                format!("Wave reached: {}", self.wave),',
        "            (\n                Target::FinalStat(2),\n"
        '                format!("Wave reached: {}", 0),',
        ["the_game_over_box_reports_every_final_number"],
    ),
    (
        "an overlay is drawn while the game is being played",
        "            GameState::Playing => {}",
        "            GameState::Playing => self.draw_pause_overlay(f, l, field),",
        ["there_is_no_overlay_while_the_game_is_being_played"],
    ),
    (
        "the overlay's lines are stacked with no gap between them",
        "    let mut y = area.y + (area.h - total) / 2.0;",
        "    let mut y = area.y + (area.h - total) / 2.0;\n    let gap = -gap;",
        ["the_overlay_lines_do_not_sit_on_top_of_each_other"],
    ),
    (
        "an overlay line with no room is drawn past the bottom of the box",
        "        if row.bottom() > area.bottom() {\n            break;\n        }",
        "        if false {\n            break;\n        }",
        ["an_overlay_in_a_short_window_drops_lines_rather_than_running_out_of_the_box"],
    ),
    (
        "the game-over box goes back to a fixed 300x200",
        "        let box_w = field.scaled(300.0).min(field.rect.w);\n"
        "        let box_h = field.scaled(200.0).min(field.rect.h);",
        "        let box_w = 300.0_f32.min(field.rect.w);\n"
        "        let box_h = 200.0_f32.min(field.rect.h);",
        ["the_game_over_box_takes_its_share_of_the_field_rather_than_a_fixed_size"],
    ),
    # -- Clicks --------------------------------------------------------
    (
        "the sheet's own box is a dead zone again",
        "            (\n                GameState::GameOver,\n"
        "                Target::Overlay | Target::OverlayTitle | Target::Resume | Target::FinalStat(_),\n"
        "            ) => {\n                self.new_game();",
        "            (GameState::GameOver, Target::Overlay) => {\n                self.new_game();",
        [
            "a_click_on_the_game_over_sheet_starts_a_new_game",
            "a_click_on_a_line_of_the_game_over_box_starts_a_new_game",
        ],
    ),
    (
        "the pause sheet's title is a dead zone",
        "            (\n                GameState::Paused,\n"
        "                Target::Overlay | Target::OverlayTitle | Target::Resume | Target::FinalStat(_),\n"
        "            ) => {\n                self.state = GameState::Playing;",
        "            (GameState::Paused, Target::Overlay | Target::Resume) => {\n"
        "                self.state = GameState::Playing;",
        ["a_click_on_the_title_of_the_pause_sheet_resumes"],
    ),
    (
        "the new-game line resumes instead of starting a new game",
        "            (GameState::Paused | GameState::GameOver, Target::NewGame) => {\n"
        "                self.new_game();\n                EventResult::Consumed\n            }",
        "            (GameState::Paused | GameState::GameOver, Target::NewGame) => {\n"
        "                self.state = GameState::Playing;\n"
        "                EventResult::Consumed\n            }",
        ["a_click_on_the_new_game_line_while_paused_starts_a_new_game"],
    ),
    (
        "a click during play does something",
        "        match (self.state, target) {",
        "        if self.state == GameState::Playing {\n"
        "            self.state = GameState::Paused;\n"
        "            return EventResult::Consumed;\n        }\n"
        "        match (self.state, target) {",
        ["a_click_during_play_does_nothing"],
    ),
    (
        "the header is treated as part of an overlay",
        "            _ => EventResult::Ignored,\n        }\n    }\n\n"
        "    // ── Game tick ",
        "            (_, Target::Header) => {\n"
        "                self.new_game();\n"
        "                EventResult::Consumed\n            }\n"
        "            _ => EventResult::Ignored,\n        }\n    }\n\n"
        "    // ── Game tick ",
        ["a_click_on_the_header_does_nothing"],
    ),
    (
        "a click on nothing at all is answered anyway",
        "        let Some(target) = self.frame(w, h).hit_test(ev.x, ev.y) else {\n"
        "            return EventResult::Ignored;\n        };",
        "        let target = self\n            .frame(w, h)\n"
        "            .hit_test(ev.x, ev.y)\n"
        "            .unwrap_or(Target::Overlay);",
        ["a_click_on_nothing_at_all_does_nothing"],
    ),
    (
        "any button is the left one",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if !matches!(ev.kind, MouseEventKind::Press(_)) {",
        ["a_right_click_is_not_a_click"],
    ),
    (
        "a click is read against the size the game opened at",
        "        let (w, h) = self.size();\n"
        "        let Some(target) = self.frame(w, h).hit_test(ev.x, ev.y) else {",
        "        let (w, h) = (WINDOW_WIDTH, WINDOW_HEIGHT);\n"
        "        let Some(target) = self.frame(w, h).hit_test(ev.x, ev.y) else {",
        ["a_click_lands_where_the_window_it_was_resized_to_put_the_control"],
    ),
    (
        "the overlay does not cover the asteroids behind it",
        "        self.draw_overlay(&mut f, &l, &field);\n        f\n    }",
        "        f\n    }",
        [
            "the_overlay_hides_the_asteroids_behind_it_from_a_click",
            "the_pause_sheet_names_both_ways_out",
            "the_game_over_box_reports_every_final_number",
        ],
    ),
    # -- Keys ----------------------------------------------------------
    (
        "a key coming back up is a second press while paused",
        "            GameState::Paused if ev.pressed => self.handle_key_paused(ev.key),",
        "            GameState::Paused => self.handle_key_paused(ev.key),",
        ["a_key_coming_back_up_is_not_a_second_press_while_paused"],
    ),
    (
        "a key coming back up restarts a finished game",
        "            GameState::GameOver if ev.pressed => self.handle_key_game_over(ev.key),",
        "            GameState::GameOver => self.handle_key_game_over(ev.key),",
        ["a_key_coming_back_up_does_not_restart_a_finished_game"],
    ),
    (
        "a key the game does not use is answered anyway",
        "    fn handle_key_playing(&mut self, key: Key, pressed: bool) -> EventResult {\n"
        "        match key {",
        "    fn handle_key_playing(&mut self, key: Key, pressed: bool) -> EventResult {\n"
        "        let _ = pressed;\n        return EventResult::Consumed;\n        #[allow(unreachable_code)]\n"
        "        match key {",
        ["a_key_the_game_does_not_use_is_ignored"],
    ),
    (
        "pausing keeps hold of the keys that were down",
        "        self.state = GameState::Paused;\n        self.input = InputState::new();",
        "        self.state = GameState::Paused;",
        ["pausing_lets_go_of_every_key_that_was_held"],
    ),
    (
        "keys do not reach the app through the window",
        "        Event::Key(ev) => app.handle_key(ev),",
        "        Event::Key(_) => EventResult::Ignored,",
        ["keys_reach_the_app_through_the_window"],
    ),
    # -- The window ----------------------------------------------------
    (
        "the window has no name",
        '        "Asteroids".to_string()',
        "        String::new()",
        ["the_window_has_a_name"],
    ),
    (
        "the window opens at a size nothing was laid out for",
        "        (INITIAL_WINDOW_W, INITIAL_WINDOW_H)",
        "        (640, 480)",
        ["the_window_opens_at_the_size_the_game_was_drawn_for"],
    ),
    (
        "the window is never woken, so nothing moves",
        "    fn tick_interval(&self) -> Option<Duration> {\n        Some(TICK)\n    }",
        "    fn tick_interval(&self) -> Option<Duration> {\n        None\n    }",
        ["the_window_asks_to_be_woken_for_the_animation"],
    ),
    (
        "the close button does not close",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Idle;\n        }",
        ["the_close_button_closes"],
    ),
    (
        "a change does not ask for a redraw",
        "            EventResult::Consumed => Response::Redraw,",
        "            EventResult::Consumed => Response::Idle,",
        [
            "an_event_that_changed_something_asks_for_a_redraw",
            "a_tick_while_playing_is_a_change",
            "keys_reach_the_app_through_the_window",
        ],
    ),
    (
        "every event asks for a redraw whether or not anything changed",
        "            EventResult::Ignored => Response::Idle,",
        "            EventResult::Ignored => Response::Redraw,",
        [
            "an_event_that_changed_nothing_does_not_ask_for_a_redraw",
            "a_tick_while_paused_is_not_a_change",
        ],
    ),
    (
        "a tick is answered while the game is paused",
        "        if self.state != GameState::Playing {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_tick_while_paused_is_not_a_change"],
    ),
    (
        "a resize is thrown away",
        "        Event::Resize { width, height } => {\n"
        "            app.resize(f32_from_u32(*width), f32_from_u32(*height));\n"
        "            EventResult::Consumed\n        }",
        "        Event::Resize { .. } => EventResult::Consumed,",
        [
            "a_resize_is_remembered",
            "a_click_lands_where_the_window_it_was_resized_to_put_the_control",
        ],
    ),
    (
        "the size a frame is drawn at is not the size the next click is read against",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["the_size_a_frame_is_drawn_at_is_the_size_the_next_click_is_read_against"],
    ),
    (
        "starting a new game forgets the window",
        "        *self = Self::with_seed(seed);\n"
        "        self.high_score = high;\n        self.size = size;",
        "        *self = Self::with_seed(seed);\n"
        "        self.high_score = high;\n        let _ = size;",
        ["starting_a_new_game_does_not_forget_the_window"],
    ),
    (
        "the high score does not survive a new game",
        "        self.high_score = high;\n        self.size = size;",
        "        let _ = high;\n        self.size = size;",
        [
            "a_click_on_the_new_game_line_while_paused_starts_a_new_game",
            "test_new_game_preserves_high_score",
            "test_high_score_persists_across_new_game",
        ],
    ),
    (
        "a clip is opened and never closed",
        "        fill(&mut f, l.window, BASE, CornerRadii::ZERO);",
        "        f.clip(l.window);\n        fill(&mut f, l.window, BASE, CornerRadii::ZERO);",
        ["the_frame_is_balanced"],
    ),
    # -- The faults the wiring exposed ---------------------------------
    (
        "a late wave fills the field with asteroids",
        "        let count = INITIAL_ASTEROIDS\n"
        "            .saturating_add(extra)\n"
        "            .min(MAX_WAVE_ASTEROIDS);",
        "        let count = INITIAL_ASTEROIDS.saturating_add(extra);",
        ["a_late_wave_does_not_fill_the_field_with_asteroids"],
    ),
    (
        "the wave stops growing at all",
        "        let count = INITIAL_ASTEROIDS\n"
        "            .saturating_add(extra)\n"
        "            .min(MAX_WAVE_ASTEROIDS);",
        "        let count = INITIAL_ASTEROIDS;",
        [
            "an_early_wave_is_still_one_asteroid_bigger_than_the_last",
            "test_wave_two_has_more_asteroids",
        ],
    ),
    (
        "a score at the ceiling wraps round to nothing",
        "        self.score = self.score.saturating_add(score_gain);",
        "        self.score = self.score.wrapping_add(score_gain);",
        ["a_score_at_the_ceiling_does_not_wrap_round_to_nothing"],
    ),
    (
        "the wave counter wraps round to nothing",
        "        self.wave = self.wave.saturating_add(1);",
        "        self.wave = self.wave.wrapping_add(1);",
        ["the_wave_counter_does_not_wrap_round_to_nothing"],
    ),
    (
        "a window of no size is drawn anyway",
        "fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {\n"
        "    if r.is_empty() {\n        return;\n    }",
        "fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {",
        ["a_window_of_no_size_still_draws_a_frame"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "asteroids", timeout=240, only=sys.argv[1:] or None))
