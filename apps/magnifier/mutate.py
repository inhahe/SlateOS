"""Mutation test for the magnifier suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program -- and this app is the strongest case for
that yet made in this tree: it shipped, under a green build and with no
warnings, a lens-resize binding that was *unreachable*, a "pause" that paused
only the picture, a ruler that divided where it should have multiplied and drew
every measurement flat, a colour readout that could never be dismissed, and no
window at all.  `#![allow(dead_code)]` on line one is what let the unreachable
binding through; nothing else would have.
"""

import re
import subprocess
import sys
from pathlib import Path

SRC = Path(__file__).parent / "src" / "main.rs"
BAK = Path(__file__).parent / "src" / "main.rs.bak"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── Screen-to-window geometry ─────────────────────────────────────────
    (
        # Fault ten, restored.  `move_center` clamped the *centre* to the
        # screen, which keeps the aim point on the display and lets the region
        # around it hang off the edge -- so at the corners three quarters of the
        # magnifier showed the black `sample_pixel` returns out of bounds.
        "the region is clamped by its centre rather than by its edges",
        "        (centre.0 - w / 2.0).clamp(0.0, screen.0 - w)",
        "        centre.0.clamp(0.0, screen.0) - w / 2.0",
        ["the_magnified_region_slides_to_stay_on_the_screen"],
    ),
    (
        # The same on the other axis, so neither is left resting on the other's
        # test.
        "the region is clamped by its centre on the vertical too",
        "        (centre.1 - h / 2.0).clamp(0.0, screen.1 - h)",
        "        centre.1.clamp(0.0, screen.1) - h / 2.0",
        ["the_magnified_region_slides_to_stay_on_the_screen"],
    ),
    (
        # A region wider than the screen has nothing to slide into.  Clamping it
        # anyway gives `clamp(0.0, negative)`, which panics in debug and pins the
        # region to one edge in release -- all the black on one side.
        "a region wider than the screen is pinned to an edge",
        "    let x = if w >= screen.0 {\n        (screen.0 - w) / 2.0\n    } else {",
        "    let x = if false {\n        (screen.0 - w) / 2.0\n    } else {",
        ["a_region_wider_than_the_screen_is_centred_on_it"],
    ),
    (
        "the window-to-screen mapping ignores the pane it is reading from",
        "    (src.x + fx * src.w, src.y + fy * src.h)",
        "    (fx * src.w, fy * src.h)",
        [
            "a_window_point_and_a_screen_point_are_inverses",
            "a_click_aims_at_the_screen_point_under_the_pixel_it_landed_on",
        ],
    ),
    (
        "the screen-to-window mapping ignores where the pane starts",
        "    (pane.x + fx * pane.w, pane.y + fy * pane.h)",
        "    (fx * pane.w, fy * pane.h)",
        [
            "a_window_point_and_a_screen_point_are_inverses",
            "the_lens_sits_over_the_part_of_the_screen_it_is_showing",
        ],
    ),
    (
        # One block per screen pixel is right until the region is the whole
        # screen, at which point it is two million rectangles per frame per
        # pane.  Removing the ceiling does not fail a test -- it hangs the
        # suite, which the runner scores as a catch.
        "the sample grid has no ceiling on how many blocks it draws",
        "    (wanted as usize).clamp(1, MAX_BLOCKS)",
        "    (wanted as usize).max(1)",
        ["the_sample_grid_is_one_block_a_pixel_until_that_is_too_many"],
    ),
    (
        # A magnifier pointed past the edge of the display must show the edge of
        # the display, not a sample wrapped round from the far side.
        "a pixel off the screen is fetched from the other side",
        "    if x < 0 || y < 0 || x >= screen_w || y >= screen_h {\n        return (0, 0, 0);\n    }",
        "",
        ["a_pixel_off_the_screen_is_black_and_not_wrapped_around"],
    ),
    # ── Colour filters ────────────────────────────────────────────────────
    # NOT a mutation: `sum.clamp(0.0, 255.0) as u8` -> `sum.min(255.0) as u8`.
    # It *was* one in the first sweep, and it survived; the reason is worth
    # writing down rather than papering over with a stronger test.  A Rust `as`
    # cast from float to integer saturates at *both* ends, so the negative that
    # the missing lower clamp lets through becomes 0 regardless and no test can
    # tell the two forms apart.  The clamp is unobservable today, and it stays:
    # "clamped at both ends" is what the line means to say, and the equivalence
    # is a property of the cast rather than of the arithmetic, so any later move
    # to `try_into` or to a wider intermediate makes it load-bearing again.
    # `a_colour_matrix_is_clamped_at_both_ends_not_just_the_top` still earns its
    # place -- it pins the behaviour of `mix` at both ends of the range, however
    # that behaviour is currently arrived at.
    (
        "the two-tone filters split somewhere other than the middle",
        "const CONTRAST_SPLIT: u8 = 128;",
        "const CONTRAST_SPLIT: u8 = 250;",
        ["a_high_contrast_filter_splits_at_the_middle_and_keeps_only_two_colours"],
    ),
    (
        # BT.601's three coefficients sum to exactly one, which is what makes
        # white come back as 255 rather than as an almost-white that a
        # high-contrast filter would then call dark.
        "the luma weights do not sum to one",
        "    let l = f32::from(r) * 0.299 + f32::from(g) * 0.587 + f32::from(b) * 0.114;",
        "    let l = f32::from(r) * 0.299 + f32::from(g) * 0.487 + f32::from(b) * 0.114;",
        ["luma_runs_the_whole_range_because_its_weights_sum_to_one"],
    ),
    (
        "inverting a pixel loses a channel",
        "                u8::MAX.saturating_sub(g),",
        "                g,",
        ["inverting_twice_gives_the_pixel_back"],
    ),
    (
        "greyscale puts the luma on one channel only",
        "                let l = Self::luma(r, g, b);\n                (l, l, l)",
        "                let l = Self::luma(r, g, b);\n                (l, g, b)",
        ["greyscale_puts_the_luma_on_all_three_channels"],
    ),
    (
        # A dichromacy matrix whose rows do not sum to one is simulating
        # something nobody has: it changes the brightness of every pixel,
        # including the ones that carry no colour at all.
        #
        # Note which test catches this and which cannot. The corners of the
        # colour cube cannot: black is the zero vector and survives any matrix,
        # and white saturates, so a row summing to 1.4 still comes back 255. It
        # takes a grey in between, which is the whole point of the separate
        # `a_simulation_filter_leaves_a_grey_exactly_as_grey_as_it_was`.
        "a colour-blindness matrix has an offset in it",
        "                [0.0, 0.475, 0.525],",
        "                [0.4, 0.475, 0.525],",
        ["a_simulation_filter_leaves_a_grey_exactly_as_grey_as_it_was"],
    ),
    (
        "the filter cycle skips one",
        "    pub fn next(self) -> Self {\n        next_in(&FILTERS, self)\n    }",
        "    pub fn next(self) -> Self {\n        next_in(&FILTERS, next_in(&FILTERS, self))\n    }",
        ["the_filter_key_walks_all_nine_filters_and_comes_back_round"],
    ),
    # ── The ruler ─────────────────────────────────────────────────────────
    (
        # Fault nine.  The old `screen_distance` divided by the zoom, so
        # magnifying a thing made the reading of it *smaller*: at 10x a hundred
        # screen pixels were announced as ten.
        "the ruler divides by the zoom instead of multiplying",
        "        self.screen_length() * zoom.max(0.0)",
        "        self.screen_length() / zoom.max(0.01)",
        ["the_length_as_shown_is_the_screen_length_multiplied_by_the_zoom"],
    ),
    (
        # The old drawing took min_x, max_x and min_y and never read end_y, so
        # every measurement was drawn as a flat bar however it had been taken.
        "a measurement is drawn flat whatever it was taken across",
        "        line(f, x1, y1, x2, y2, YELLOW, 2.0);",
        "        line(f, x1, y1, x2, y1, YELLOW, 2.0);",
        ["a_diagonal_measurement_is_drawn_as_a_diagonal"],
    ),
    (
        "the ruler measures only across, not down",
        "                let dy = ey - sy;\n                (dx * dx + dy * dy).sqrt()",
        "                let dy = ey - sy;\n                let _ = dy;\n                dx.abs()",
        ["a_measurement_runs_between_the_two_points_it_was_taken_between"],
    ),
    (
        "a finished measurement cannot be cleared",
        '            Ruler::Done { .. } => {\n                self.status = "Ruler cleared".to_string();\n                Ruler::Off\n            }',
        '            Ruler::Done { start, end } => {\n                self.status = "Ruler cleared".to_string();\n                Ruler::Done { start, end }\n            }',
        [
            "the_ruler_key_opens_it_closes_it_and_then_clears_it",
            "the_ruler_button_says_which_of_the_three_things_it_will_do",
        ],
    ),
    # ── Keys ──────────────────────────────────────────────────────────────
    (
        # Fault four, restored.  The old handler took a `&str` nothing produced
        # and never asked whether the key was going down, so every keystroke ran
        # its action twice.
        "a key coming back up is a second press again",
        "        if !ev.pressed {\n            return None;\n        }",
        "",
        ["a_key_coming_back_up_is_not_a_second_press"],
    ),
    (
        "a modifier the program does not use is answered anyway",
        "        if ev.modifiers.alt || ev.modifiers.super_key {\n            return None;\n        }",
        "",
        ["a_modifier_the_program_does_not_use_is_left_alone"],
    ),
    (
        # Fault five, restored exactly.  The old program's four `shift` arms sat
        # *below* the four plain-arrow ones, which test `!ctrl` rather than
        # `!shift` -- so shift-Left matched the earlier arm, panned, and the lens
        # could not be resized from the keyboard at all.  Deleting the shift
        # block is the same fall-through by a shorter route.
        "the shift-arrow bindings are unreachable behind the plain ones",
        "        if ev.modifiers.shift {\n"
        "            return match ev.key {\n"
        "                Key::Left => Some(Action::ResizeLens(-LENS_STEP, 0.0)),\n"
        "                Key::Right => Some(Action::ResizeLens(LENS_STEP, 0.0)),\n"
        "                Key::Up => Some(Action::ResizeLens(0.0, -LENS_STEP)),\n"
        "                Key::Down => Some(Action::ResizeLens(0.0, LENS_STEP)),\n"
        "                _ => None,\n"
        "            };\n"
        "        }",
        "",
        ["shift_and_an_arrow_key_resizes_the_lens"],
    ),
    (
        "the open sheet does not swallow the keys behind it",
        "        if self.show_help {\n"
        "            return match ev.key {\n"
        "                Key::Escape | Key::H | Key::F1 | Key::Enter | Key::Space => Some(Action::CloseHelp),\n"
        "                _ => None,\n"
        "            };\n"
        "        }",
        "",
        ["the_open_sheet_swallows_the_keys_that_are_not_about_it"],
    ),
    (
        # The old program bound six of the ten presets, so 6x, 15x and 20x could
        # only be reached by stepping through everything below them.
        "four of the ten zoom presets have no key of their own",
        "            Key::Num6 => Some(Action::SetPreset(5)),",
        "            Key::Num6 => None,",
        ["every_preset_has_a_key_of_its_own"],
    ),
    (
        "the fast pan is no faster than the slow one",
        "                Key::Right => Some(Action::Pan(PAN_STEP_FAST, 0.0)),",
        "                Key::Right => Some(Action::Pan(PAN_STEP, 0.0)),",
        ["ctrl_and_an_arrow_key_moves_further_than_the_arrow_alone"],
    ),
    # ── Actions ───────────────────────────────────────────────────────────
    (
        # Fault seven, restored.  The old `enabled` flag was read in `render` and
        # nowhere else, so every key and every button still worked while
        # "paused" and you found out what you had done when you resumed.
        "pausing pauses only the picture",
        '        if self.paused && !action.allowed_while_paused() {\n            self.status = "Paused — Esc to resume".to_string();\n            return;\n        }',
        "",
        ["pausing_stops_every_control_and_not_merely_the_picture"],
    ),
    (
        # The other half of the same gate: a pause nothing can undo is worse
        # than no pause at all.
        "pausing locks out the way back out of it",
        "        matches!(\n            self,\n            Self::TogglePause | Self::ToggleHelp | Self::CloseHelp | Self::ToggleChrome\n        )",
        "        false",
        ["pausing_leaves_the_ways_out_of_it_working"],
    ),
    (
        # Panning from the centre throws away whatever of the last pan had not
        # eased in yet, so a held arrow key crawls instead of moving.
        "a pan starts from where the view is rather than where it is heading",
        "                let (tx, ty) = self.target;\n                self.aim_at(tx + dx, ty + dy);",
        "                let (cx, cy) = self.centre;\n                self.aim_at(cx + dx, cy + dy);",
        ["holding_an_arrow_key_pans_from_where_it_is_heading_not_from_where_it_is"],
    ),
    (
        "the view can be aimed off the screen",
        "        self.target = (x.clamp(0.0, self.screen.0), y.clamp(0.0, self.screen.1));",
        "        self.target = (x, y);",
        ["the_view_cannot_be_panned_off_the_screen"],
    ),
    (
        "the zoom runs off the top of the presets",
        "                self.set_preset(\n                    self.preset\n                        .saturating_add(1)\n                        .min(ZOOM_PRESETS.len().saturating_sub(1)),\n                );",
        "                self.set_preset(self.preset.saturating_add(1) % ZOOM_PRESETS.len());",
        ["the_zoom_walks_the_presets_and_stops_at_both_ends"],
    ),
    (
        "the lens can be shrunk away to nothing",
        "                self.lens_w = (self.lens_w + dw).clamp(LENS_MIN, LENS_MAX);",
        "                self.lens_w = self.lens_w + dw;",
        ["the_lens_cannot_be_shrunk_away_or_grown_without_end"],
    ),
    (
        "the docked strip can take the whole window or none of it",
        "                self.dock = (self.dock + d).clamp(DOCK_MIN, DOCK_MAX);",
        "                self.dock += d;",
        ["the_brackets_resize_the_docked_strip_within_bounds"],
    ),
    (
        # Switching smoothing off while a pan is running leaves the view
        # stranded: nothing will ask for another tick, so it sits where it got
        # to until something else moves it.
        "switching smoothing off strands a pan that is still running",
        "                if !self.smooth {\n                    self.centre = self.target;\n                }",
        "",
        ["switching_smoothing_off_mid_pan_does_not_strand_the_view"],
    ),
    (
        # The readout is here so someone who cannot make out a colour can be
        # told what it is, and what they are looking at is the filtered picture.
        "the colour readout reports the unfiltered pixel as what is shown",
        "        let shown = self.filter.apply(r, g, b);\n        self.picked = Some(shown);",
        "        let shown = self.filter.apply(r, g, b);\n        self.picked = Some((r, g, b));",
        ["the_readout_gives_the_filtered_colour_and_the_unfiltered_one_beside_it"],
    ),
    (
        "a screenshot is not counted",
        "                self.shots = self.shots.saturating_add(1);",
        "",
        ["a_screenshot_is_counted_and_the_count_is_shown"],
    ),
    (
        "one shot is reported in the plural",
        '            if self.shots == 1 { "" } else { "s" },',
        '            "s",',
        ["a_screenshot_is_counted_and_the_count_is_shown"],
    ),
    (
        "the mode cycle skips one",
        "    pub fn next(self) -> Self {\n        next_in(&MODES, self)\n    }",
        "    pub fn next(self) -> Self {\n        next_in(&MODES, next_in(&MODES, self))\n    }",
        ["the_frame_is_well_formed_at_every_size_mode_and_state"],
    ),
    (
        "the tracking cycle skips one",
        "    pub fn next(self) -> Self {\n        next_in(&TRACKINGS, self)\n    }",
        "    pub fn next(self) -> Self {\n        next_in(&TRACKINGS, next_in(&TRACKINGS, self))\n    }",
        ["the_tracking_key_walks_the_three_settings_and_comes_back_round"],
    ),
    # ── The clock ─────────────────────────────────────────────────────────
    (
        # Lesson 47, and the magnifier is its seventh case.  An app that leaves
        # `tick_interval` at the default receives no ticks at all -- so the
        # smooth tracking the old `smooth_edges` field promised could never have
        # eased anything even had anything read it.
        "the window is never asked for a clock",
        "        self.easing().then_some(TICK)",
        "        None",
        ["the_window_is_asked_for_a_clock_exactly_while_the_view_is_moving"],
    ),
    (
        # The other half: a clock that never stops wakes the compositor thirty
        # times a second to redraw a picture that is not moving.
        "the clock is asked for even when nothing is moving",
        "        self.easing().then_some(TICK)",
        "        Some(TICK)",
        ["the_window_is_asked_for_a_clock_exactly_while_the_view_is_moving"],
    ),
    (
        # The interval asked for is a floor, not a promise.  Easing by the
        # constant makes a pan run slow by however much the loop was busy, and
        # run slow silently.
        "the easing goes by the interval it asked for",
        "        let seconds = elapsed_ms as f32 / 1000.0;",
        "        let seconds = TICK.as_millis() as f32 / 1000.0;",
        ["the_easing_goes_by_the_time_that_passed_not_by_the_interval_asked_for"],
    ),
    (
        "a tick with nothing to do still asks for a frame",
        "        if !self.easing() {\n            // Land exactly, so `easing` cannot answer true forever on a\n            // fraction of a pixel that never quite closes.\n            self.centre = self.target;\n            return EventResult::Ignored;\n        }",
        "        if !self.easing() {\n            self.centre = self.target;\n            return EventResult::Consumed;\n        }",
        ["a_tick_moves_the_view_toward_its_target_and_then_stops_asking"],
    ),
    (
        # Without the landing the gap closes by a fraction each time and never
        # reaches zero, so `easing` answers true forever and the clock never
        # stops.  `settle` is bounded precisely so this reports rather than
        # hangs.
        "the view never quite lands on its target",
        "        if !self.easing() {\n            self.centre = self.target;\n        }\n        EventResult::Consumed",
        "        EventResult::Consumed",
        ["a_tick_moves_the_view_toward_its_target_and_then_stops_asking"],
    ),
    (
        "smoothing cannot be switched off",
        "        if !self.smooth {\n            self.centre = self.target;\n        }\n    }\n\n    fn set_preset",
        "    }\n\n    fn set_preset",
        ["switching_smoothing_off_makes_the_view_arrive_at_once"],
    ),
    # ── Pointer ───────────────────────────────────────────────────────────
    (
        # The whole difference between the three tracking settings is which
        # events re-aim the view.  Dropping the test makes all three the same.
        "every tracking setting follows the pointer",
        "                if self.paused || self.show_help || self.tracking != TrackingMode::FollowMouse {\n                    return EventResult::Ignored;\n                }",
        "",
        ["the_other_two_trackings_leave_the_pointer_as_just_a_pointer"],
    ),
    (
        "manual tracking follows a click anyway",
        "                if self.tracking == TrackingMode::Manual {\n                    return EventResult::Ignored;\n                }",
        "",
        ["a_click_in_the_picture_aims_at_it_unless_tracking_is_manual"],
    ),
    (
        "every mouse button aims the view",
        "            MouseEventKind::Press(MouseButton::Left) => {",
        "            MouseEventKind::Press(_) => {",
        ["a_mouse_button_the_program_does_not_use_does_nothing"],
    ),
    (
        # Fault two, in the form that survives the rewrite: a click read against
        # a size the window is not is a click that lands on whatever used to be
        # there.
        "a click is read against a fixed window size",
        "        self.frame(self.size_drawn.0, self.size_drawn.1)\n            .hit_test(x, y)",
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hit_test(x, y)",
        [
            "a_click_is_read_against_the_size_the_window_was_last_drawn_at",
            "rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against",
        ],
    ),
    (
        "rendering does not record the size it drew at",
        "        self.size_drawn = (width.max(1.0), height.max(1.0));",
        "",
        [
            "a_click_is_read_against_the_size_the_window_was_last_drawn_at",
            "rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against",
        ],
    ),
    # ── Layout ────────────────────────────────────────────────────────────
    (
        "the picture's share of the window is not reserved",
        "        let budget = (h - h * VIEWPORT_SHARE - pad * 2.0).max(0.0);",
        "        let budget = h;",
        ["the_viewport_keeps_its_share_of_every_window"],
    ),
    (
        "band drop order reversed",
        "const BAND_DROP_ORDER: [usize; 3] = [0, 2, 1];",
        "const BAND_DROP_ORDER: [usize; 3] = [1, 2, 0];",
        ["the_bands_are_dropped_whole_in_the_stated_order_when_the_window_shrinks"],
    ),
    (
        # Both read the same to `shows`, but only one reads the same to anything
        # asking "is this band gone, or merely thin?"
        "a dropped band is a full-width strip no pixels tall",
        "        let header = if hdr_h > 0.0 {\n            Rect::new(0.0, 0.0, w, hdr_h)\n        } else {\n            Rect::EMPTY\n        };",
        "        let header = Rect::new(0.0, 0.0, w, hdr_h);",
        ["a_dropped_band_is_nothing_at_all_rather_than_a_strip_no_pixels_tall"],
    ),
    (
        # The dropped-band form of this is the equivalent mutant noted below;
        # this one is the *live* fault -- reading the top from the band above
        # without adding the one above that.
        "the picture starts below the info line and over the header",
        "        let top = hdr_h + inf_h;",
        "        let top = inf_h;",
        ["the_viewport_never_sits_on_top_of_a_band_that_is_still_there"],
    ),
    # NOT a mutation: `let top = hdr_h + inf_h;` -> `let top = info.bottom();`.
    # It looks like one -- a dropped band is `Rect::EMPTY`, whose bottom is
    # zero, so reading the band back would put the picture over the header the
    # moment the info line went while the header stayed -- but `BAND_DROP_ORDER`
    # drops the header *first*, so the info line is only ever dropped once the
    # header already is and the two forms agree everywhere.  The mutant is
    # equivalent and no test can catch it.  The source is written the safe way
    # regardless, with the reasoning at the line, because the equivalence is a
    # property of a constant somebody will reorder.
    (
        "the chrome cannot be hidden",
        "        let mut wants = if chrome {",
        "        let mut wants = if true {",
        ["hiding_the_chrome_gives_the_whole_window_to_the_picture"],
    ),
    (
        "the two panes are cut from the window rather than from the viewport",
        "        let v = self.viewport;\n        if v.w <= 0.0 || v.h <= 0.0 {\n            return (Rect::EMPTY, Rect::EMPTY);\n        }",
        "        let v = self.window;\n        if v.w <= 0.0 || v.h <= 0.0 {\n            return (Rect::EMPTY, Rect::EMPTY);\n        }",
        ["the_two_panes_never_overlap_in_any_mode_or_at_any_dock_share"],
    ),
    (
        "docking at the bottom docks at the top",
        "                if mode.docks_top() {",
        "                if true {",
        ["docking_at_the_bottom_puts_the_strip_at_the_bottom"],
    ),
    (
        "the docked strip takes a share of the window rather than of the viewport",
        "                let strip = (v.h * dock.clamp(DOCK_MIN, DOCK_MAX)).max(0.0);",
        "                let strip = (v.h * 0.5).max(0.0);",
        ["the_docked_strip_takes_the_share_it_says_and_the_rest_is_life_size"],
    ),
    (
        # A lens half off the edge shows half a picture -- and the half that is
        # off the edge is still a hit box, so a click there is read against a
        # pane the window never drew.
        "the lens is allowed off the edge of the viewport",
        "        let x = (at.0 - w / 2.0).clamp(v.x, (v.right() - w).max(v.x));\n        let y = (at.1 - h / 2.0).clamp(v.y, (v.bottom() - h).max(v.y));",
        "        let x = at.0 - w / 2.0;\n        let y = at.1 - h / 2.0;",
        ["the_lens_is_kept_inside_the_viewport"],
    ),
    (
        # Fault eight, restored.  `render_lens` put the lens at `mouse_x/mouse_y`
        # and filled it from `center_x/center_y`.  Those are the same number only
        # while tracking follows the mouse; under manual tracking the lens sat
        # under your hand and showed somewhere else.
        "the lens is drawn at the pointer and filled from the centre",
        "        let at = window_point(life, src, self.centre.0, self.centre.1);\n        l.lens(at, self.lens_w, self.lens_h)",
        "        l.lens(self.pointer, self.lens_w, self.lens_h)",
        ["the_lens_sits_over_the_part_of_the_screen_it_is_showing"],
    ),
    # ── Drawing and hit boxes ─────────────────────────────────────────────
    (
        # Fault three, restored.  The old toolbar drew `[H]elp [M]ode [T]rack
        # [F]ilter` as text, recorded nothing, and had no mouse handler at all:
        # a picture of buttons.
        "the buttons record no hit box",
        "            f.hit(*target, r);",
        "",
        [
            "a_button_is_clickable_exactly_where_it_is_drawn",
            "every_control_the_frame_records_is_wired_to_something",
        ],
    ),
    (
        "the magnified pane records no hit box",
        "            f.hit(Target::Magnified, mag);",
        "",
        ["the_picture_is_still_usable_in_a_window_too_small_for_any_chrome"],
    ),
    (
        # The lens's box has to go on *after* the life-size pane's, because
        # `hit_test` reads the last box at a point: recorded before, a click
        # inside the lens would be read at life size against the pane behind it.
        # Deleting it does not change what a click *does* -- `screen_at` finds
        # the lens by geometry either way -- but it changes what the frame says
        # is under the pointer, from the magnified view to the life-size pane it
        # is drawn on top of.  A hit map that names the wrong pane is a wrong
        # answer today to anything that reads it and a wrong click tomorrow.
        "the lens is not clickable as the magnified view",
        "        f.hit(Target::Magnified, lens);",
        "",
        ["a_click_inside_the_lens_is_read_against_the_lens_and_not_the_pane_behind_it"],
    ),
    (
        # And the other half of the same fact: the lens's own box has to be read
        # at the zoom.  Reading it at life size makes a click in the lens aim at
        # whatever is behind the lens rather than at what the lens is showing.
        "a click in the lens is read at life size",
        "        if self.mode == MagnifyMode::Lens {\n            let lens = self.lens_rect();\n            if lens.contains(x, y) {",
        "        if false {\n            let lens = self.lens_rect();\n            if lens.contains(x, y) {",
        ["a_click_inside_the_lens_is_read_against_the_lens_and_not_the_pane_behind_it"],
    ),
    (
        # It reads sensibly -- cover the sheet you drew -- and it is wrong: the
        # sheet is opaque but smaller than the window, so every control it does
        # not physically cover goes on answering clicks the user cannot see the
        # targets of.
        "the open sheet covers only its own pixels, not the window",
        "        f.hit(Target::ToggleHelp, l.window);",
        "        f.hit(Target::ToggleHelp, h);",
        [
            "while_the_sheet_is_up_every_point_in_the_window_belongs_to_it",
            "a_click_anywhere_while_the_sheet_is_up_closes_it_and_reaches_nothing_behind",
        ],
    ),
    (
        # Fault six, restored.  `show_color_picker` was set true and assigned
        # false nowhere, so the readout could never be dismissed -- and it
        # overlapped the toolbar exactly.
        "the picked colour cannot be shown at all",
        "        if let Some((r, g, b)) = self.picked {\n            fill(f, swatch_rect, Color::rgb(r, g, b), swatch * 0.2);\n            stroke(f, swatch_rect, SURFACE1, 1.0, swatch * 0.2);\n        }",
        "",
        ["a_picked_colour_puts_a_swatch_in_the_info_band"],
    ),
    (
        "the info band does not say which filter is on",
        "            self.filter.label(),",
        '            "",',
        ["the_info_band_says_the_mode_the_tracking_the_filter_and_the_readings"],
    ),
    (
        "the info band is not drawn",
        "            &self.info_line(),",
        '            "",',
        ["the_info_band_says_the_mode_the_tracking_the_filter_and_the_readings"],
    ),
    (
        "the header says the zoom while the program is paused",
        '        let right = if self.paused {\n            "paused".to_string()\n        } else {\n            trim_zoom(self.zoom())\n        };',
        "        let right = trim_zoom(self.zoom());",
        ["the_header_says_the_zoom_and_says_paused_instead_when_it_is"],
    ),
    (
        "a paused window goes on drawing the picture",
        "        if self.paused {\n            self.draw_paused(f, l);\n            return;\n        }",
        "",
        ["a_paused_window_says_so_instead_of_drawing_a_stale_picture"],
    ),
    (
        "the crosshair cannot be switched off",
        "            if self.crosshair {\n                self.draw_crosshair(f, pane);\n            }",
        "            self.draw_crosshair(f, pane);",
        ["the_crosshair_can_be_switched_off_and_the_button_shows_which_it_is"],
    ),
    (
        # A button that merely names what it toggles leaves you to find out
        # which way it is set by pressing it.
        "the buttons name what they toggle rather than report it",
        "            Target::TogglePause => if self.paused { \"Resume\" } else { \"Pause\" }.to_string(),",
        '            Target::TogglePause => "Pause".to_string(),',
        ["the_pause_button_pauses_and_then_says_resume"],
    ),
    (
        "the filter button does not say which filter is on",
        "            Target::NextFilter => self.filter.short().to_string(),",
        '            Target::NextFilter => "Filter".to_string(),',
        ["the_filter_button_names_the_filter_it_is_showing"],
    ),
    (
        "the filter never reaches the pixels that are drawn",
        "                let (fr, fg, fb) = self.filter.apply(r, g, b);",
        "                let (fr, fg, fb) = (r, g, b);",
        ["the_filter_reaches_the_pixels_the_window_draws"],
    ),
    (
        "the sheet lists no shortcuts",
        "        for (i, (keys, what)) in HELP_ROWS.iter().enumerate() {",
        "        for (i, (keys, what)) in HELP_ROWS.iter().enumerate().take(0) {",
        ["the_sheet_lists_a_shortcut_for_each_thing_it_claims_to"],
    ),
    (
        "a clip is pushed and never popped",
        "        f.unclip();\n    }\n\n    /// The line that says where the magnified strip stops.",
        "    }\n\n    /// The line that says where the magnified strip stops.",
        ["the_frame_is_well_formed_at_every_size_mode_and_state"],
    ),
    # ── Window ────────────────────────────────────────────────────────────
    (
        "a close request is ignored",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "",
        ["a_close_request_ends_the_program"],
    ),
    (
        "every event asks for a repaint",
        "        match handle_event(self, event) {\n            EventResult::Consumed => Response::Redraw,\n            EventResult::Ignored => Response::Idle,\n        }",
        "        let _ = handle_event(self, event);\n        Response::Redraw",
        [
            "an_event_that_changed_something_asks_for_a_repaint_and_one_that_did_not_does_not"
        ],
    ),
    (
        "the window has no name",
        '        "Magnifier".to_string()',
        "        String::new()",
        ["the_window_names_itself"],
    ),
    (
        "the window is not resizable in a way the layout believes",
        "        self.resize(width, height);\n        self.frame(width, height).into_tree()",
        "        self.frame(width, height).into_tree()",
        ["rendering_a_frame_is_what_sets_the_size_the_next_click_is_read_against"],
    ),
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
            "magnifier",
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
    # A mutant can kill the *harness* rather than a test: lifting the ceiling
    # off the sample grid asks for two million rectangles per pane per frame,
    # which either hangs or dies on the allocation before any test can report.
    # The harness prints no `tests::<name>` line for that, so without this the
    # run would look exactly like a mutant no test noticed -- the opposite of
    # what happened.  Anything that ends non-zero having compiled and named no
    # failing test is a crash, and a crash is a catch.
    crashed = compiled and not timed_out and not failed and out.returncode != 0
    return compiled, failed, timed_out, crashed, out


def main():
    # The backup is written fresh from the source on every run, and removed
    # again when the run ends.  It exists only so that a Ctrl-C or a crash
    # mid-mutation leaves the real program on disk rather than a broken one.
    #
    # It must NOT be reused across runs, and the earlier version of this script
    # -- `if not BAK.exists()` -- did exactly that.  The consequence is not
    # theoretical: the second sweep of this app read a `.bak` left by the first,
    # wrote it over `main.rs`, and silently threw away every test fix made
    # between the two sweeps, then reported the same survivors as before.  A
    # mutation runner that can quietly revert the source it is measuring is
    # worse than no runner, because its output looks like evidence.
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
                print(out.stdout[-2000:])
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
