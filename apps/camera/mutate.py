"""Mutation test for the camera's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The camera is the forty-sixth application in this campaign.  `main` was:

    fn main() {
        let mut app = CameraApp::new();
        for _ in 0..5 {
            app.tick(33);
        }
        app.take_photo();
        app.toggle_recording();
        app.tick(1000);
        app.toggle_recording();
        let _ = app.render();
    }

It ticked five simulated frames, took a photograph nobody saw, recorded a
second of nothing, rendered into a `Vec` and bound the result to `let _`.
Every pixel it produced was discarded, no click or key ever reached the
program, and it still exited zero.

Underneath that were four sidebar panels, eight image filters, a self-timer, a
recording session with pause and resume, and a gallery with favourites and
deletion -- none of which had anything to run in.

What the wiring exposed, in rough order of how badly it would have shown:

  * **The layout was compile-time furniture.**  A 260 px sidebar, a 100 px
    photo strip and a 48 px toolbar, subtracted from whatever window the
    program was given, so a 200 px window gave the viewfinder a *negative*
    width.  It never showed, because there was no window.
  * **The toolbar walked a cursor along the strip with literals** -- `tx += 80`
    after the word "Camera", `tx += 160` after the camera name -- so a longer
    camera name ran into the mode buttons and a shorter one left a hole.
  * **Every label was drawn with no width at all.**  A long camera name wrote
    over the mode buttons; a long status message wrote over the photo count.
  * **`tick` had no caller** outside `main`'s simulation and the tests.  The
    running program would have shown one frame for the life of the process,
    counted a recording that never got longer, and run a self-timer that never
    fired.
  * **Filters were chosen by matching each digit to a filter by name** in a
    second list that had to be kept in step with the first by hand.
  * **`#![allow(dead_code)]` covered the whole crate**, and two palette entries
    no picture used were dead behind it.
  * **Fourteen tests asserted `!cmds.is_empty()` and nothing else.**

One more fault was found by the size sweep while these tests were being
written, and it is the one the sweep leans on hardest: the toolbar's button
height was computed from the font and never bounded by the toolbar, so in a
window shorter than the toolbar wanted, every button was painted past the
bottom edge of the window -- and hit-boxed there too.

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
    # -- the panes as fractions of the window --------------------------------
    (
        "a negative window size is laid out rather than clamped away",
        "        let w = w.max(0.0);\n        let h = h.max(0.0);",
        "        let w = w;\n        let h = h;",
        ["a_nonsense_window_size_yields_no_layout_rather_than_a_wrong_one"],
    ),
    (
        "the sidebar is taken without checking what it leaves the viewfinder",
        "        if sidebar_w < MIN_SIDEBAR_W || w - sidebar_w < MIN_VIEWFINDER_W {\n"
        "            sidebar_w = 0.0;\n        }",
        "        if false {\n            sidebar_w = 0.0;\n        }",
        ["the_viewfinder_is_the_last_pane_given_up"],
    ),
    (
        "the strip is taken without checking what it leaves the viewfinder",
        "        if strip_h < MIN_STRIP_H || content_h - strip_h < MIN_VIEWFINDER_H {\n"
        "            strip_h = 0.0;\n        }",
        "        if false {\n            strip_h = 0.0;\n        }",
        ["the_viewfinder_is_the_last_pane_given_up"],
    ),
    (
        "the viewfinder is the whole window rather than what the sidebar left",
        "            (w - sidebar_w).max(0.0),\n            (content_h - strip_h).max(0.0),",
        "            w,\n            content_h,",
        # Not "escapes the window": at every width where the sidebar is taken
        # the window is wide enough that the viewfinder still fits inside it.
        # What it does is paint over the sidebar and the strip.
        ["no_two_panes_overlap_at_any_size"],
    ),
    (
        "the status line is hung past the bottom edge of the window",
        "        let status = Rect::new(0.0, h - status_h, w, status_h);",
        "        let status = Rect::new(0.0, h, w, status_h);",
        ["every_pane_stays_inside_the_window_at_every_size"],
    ),
    (
        "the button height comes from the font rather than from the toolbar",
        "        let button = (toolbar_h - pad * 1.6).clamp(0.0, wanted_button);",
        "        let button = wanted_button;",
        # The fault the sweep found. In a window shorter than the toolbar
        # wants, every toolbar control is painted -- and hit-boxed -- below the
        # bottom edge of the window.
        [
            "nothing_is_painted_outside_the_window",
            "every_hit_box_is_inside_the_window_and_has_an_area",
        ],
    ),
    (
        "a row that half fits is counted as a row",
        "        let n = (left / self.row).floor();",
        "        let n = (left / self.row).ceil();",
        ["rows_in_never_counts_a_row_that_does_not_fit"],
    ),
    # -- what is painted -----------------------------------------------------
    (
        "only the viewfinder is filled, leaving the rest of the window bare",
        "        fill(&mut f, l.window, CRUST, CornerRadii::ZERO);",
        "        fill(&mut f, l.viewfinder, CRUST, CornerRadii::ZERO);",
        ["the_window_is_filled_edge_to_edge_before_anything_else"],
    ),
    (
        "text is drawn unbounded, so a wide face walks over its neighbour",
        "        max_width: Some(r.w),",
        "        max_width: None,",
        ["no_run_of_text_is_drawn_unbounded_or_off_the_window"],
    ),
    (
        "the viewfinder's clip is opened and never closed",
        "            f.unclip();\n        }\n\n        // A disconnected camera is the one state",
        "        }\n\n        // A disconnected camera is the one state",
        ["the_frame_balances_its_clips_at_every_size_in_every_state"],
    ),
    # A row that widened the strip's clip from the strip to the whole window
    # was here, on the theory that the tiles can overrun the strip by a partial
    # one. They cannot: `fits` is a *floor* of whole steps, so the last tile's
    # right edge lands at `s.x + s.w - pad`, a whole pad inside the strip, at
    # every window size in the grid -- the clip is belt to the braces of the
    # count, not the thing holding the picture in. The mutation survived
    # because it cannot alter one pixel of any frame, which is a fact about
    # the code and not evidence about a test; naming an owner for it would
    # have taught this table to claim coverage that does not exist.
    (
        "the thirds grid is painted whether or not it is switched on",
        "            if self.show_grid_overlay {\n                draw_thirds(f, picture);\n            }",
        "            draw_thirds(f, picture);",
        ["the_overlays_are_painted_only_when_they_are_switched_on"],
    ),
    (
        "the flash is set and never painted",
        "        if self.flash_remaining_ms > 0 {\n            self.draw_flash(&mut f, &l);\n        }",
        "        if false {\n            self.draw_flash(&mut f, &l);\n        }",
        ["the_flash_ages_out_of_the_picture"],
    ),
    # -- what the pointer reaches --------------------------------------------
    (
        "any mouse event is a left click",
        "        if mouse.kind != MouseEventKind::Press(MouseButton::Left) {\n"
        "            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_right_click_is_not_a_left_one"],
    ),
    (
        "a click on bare background repeats the last control",
        "            None => EventResult::Ignored,",
        "            None => {\n                self.activate(Target::Shutter);\n                EventResult::Consumed\n            }",
        ["a_click_on_bare_background_changes_nothing"],
    ),
    (
        "the viewfinder is decoration rather than the shutter",
        "            Target::Shutter | Target::Viewfinder => match self.capture_mode {",
        "            Target::Viewfinder => {}\n            Target::Shutter => match self.capture_mode {",
        ["clicking_the_viewfinder_takes_the_picture_the_shutter_would"],
    ),
    (
        "the grid toggle moves the histogram switch",
        "            Target::Grid => self.toggle_grid_overlay(),",
        "            Target::Grid => self.toggle_histogram(),",
        ["each_toolbar_toggle_moves_its_own_switch_and_no_other"],
    ),
    (
        "every filter row records the same filter",
        "            f.hit(Target::Filter(*filter), r);",
        "            f.hit(Target::Filter(ImageFilter::None), r);",
        ["the_filter_panel_selects_the_filter_it_names"],
    ),
    (
        "every filter row records the filter named on the row below it",
        "            f.hit(Target::Filter(*filter), r);",
        "            let shifted = ImageFilter::all()\n"
        "                .iter()\n"
        "                .cycle()\n"
        "                .skip_while(|g| *g != filter)\n"
        "                .nth(1)\n"
        "                .copied()\n"
        "                .unwrap_or(*filter);\n"
        "            f.hit(Target::Filter(shifted), r);",
        # This is the mutation the obvious version of the test cannot see. It
        # relabels every row consistently, so a test that asks the frame where
        # `Filter(Sepia)` is and then clicks there gets the row reading
        # "Grayscale" and is told Sepia was chosen -- the mistake cancels
        # itself out. Only reading the words on the row breaks the symmetry.
        ["the_filter_panel_selects_the_filter_it_names"],
    ),
    (
        "every choice row records the row above it",
        "        f.hit(target(i), r);",
        "        f.hit(target(i.saturating_sub(1)), r);",
        ["the_device_panel_chooses_the_resolution_and_rate_it_names"],
    ),
    (
        "both ends of a slider nudge it the same way",
        "            for (r, label, nudge) in [(down, \"-\", Nudge::Down), (up, \"+\", Nudge::Up)] {",
        "            for (r, label, nudge) in [(down, \"-\", Nudge::Up), (up, \"+\", Nudge::Up)] {",
        ["each_slider_end_moves_its_own_setting_in_its_own_direction"],
    ),
    (
        "the star and the bin are hit-boxed over each other",
        "                f.hit(Target::Favorite, fav);",
        "                f.hit(Target::Delete, fav);",
        ["the_gallery_favourites_and_deletes_the_photograph_it_shows_as_selected"],
    ),
    (
        "the strip always starts at the first photograph",
        "            sel.saturating_sub(fits.saturating_sub(1))\n"
        "                .min(total.saturating_sub(fits))",
        "            0_usize",
        ["the_strip_keeps_the_selected_photograph_reachable"],
    ),
    # -- what the keyboard reaches -------------------------------------------
    (
        "a key coming back up is a second keystroke",
        "        if !key.pressed {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_key_coming_back_up_is_not_a_second_keystroke"],
    ),
    (
        "a shortcut reads the key position rather than the letter typed",
        "            let letter = key.text.chars().next().map(|c| c.to_ascii_lowercase());",
        "            let letter = match key.key {\n"
        "                Key::H => Some('h'),\n"
        "                Key::V => Some('v'),\n"
        "                Key::M => Some('m'),\n"
        "                Key::R => Some('r'),\n"
        "                Key::S => Some('s'),\n"
        "                _ => None,\n            };",
        ["a_shortcut_reads_the_letter_typed_and_not_the_key_position"],
    ),
    (
        "a digit picks the filter after the one it sits over",
        "            let nth = d.checked_sub(1).and_then(|i| usize::try_from(i).ok());",
        "            let nth = usize::try_from(d).ok();",
        ["a_digit_picks_the_filter_at_that_position_in_the_list"],
    ),
    (
        "the space bar and the shutter go different ways",
        "            Key::Enter | Key::Space => {\n                match self.capture_mode {",
        "            Key::Enter | Key::Space => {\n                self.set_status(\"\");\n                match CaptureMode::Video {",
        ["the_shutter_and_the_space_bar_take_the_same_photograph"],
    ),
    # -- the clock, and the entry points the platform calls -------------------
    (
        "the tick is dropped on the floor",
        "            Event::Tick { elapsed_ms } => {\n                if self.tick(*elapsed_ms) {",
        "            Event::Tick { elapsed_ms } => {\n                if self.tick(0) {",
        [
            "the_clock_reaches_the_program_through_the_entry_point_the_platform_calls",
            "the_recording_clock_and_the_self_timer_age_on_the_tick",
        ],
    ),
    (
        "every tick asks for a repaint, whether anything moved or not",
        "        live || was_recording || was_counting || was_flashing",
        "        true",
        ["a_tick_that_changes_nothing_asks_for_no_repaint"],
    ),
    (
        "no tick ever asks for a repaint",
        "        live || was_recording || was_counting || was_flashing",
        "        false",
        ["the_clock_reaches_the_program_through_the_entry_point_the_platform_calls"],
    ),
    (
        "the self-timer runs down and takes no photograph",
        "        if timer_expired {\n            self.do_capture();\n        }",
        "        if timer_expired {}",
        ["the_recording_clock_and_the_self_timer_age_on_the_tick"],
    ),
    (
        "a resize is not remembered, so the click is measured against the old picture",
        "                self.width = f32_from_u32(*width);\n                self.height = f32_from_u32(*height);",
        "                let _ = (width, height);",
        ["a_resize_is_the_size_the_next_click_is_measured_against"],
    ),
    (
        "the picture is laid out from the remembered size rather than the given one",
        "        self.width = width;\n        self.height = height;\n        self.frame(width, height).into_tree()",
        "        let _ = (width, height);\n        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()",
        ["the_picture_is_drawn_at_the_size_render_is_given"],
    ),
    (
        "every event closes the window",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        return Response::Exit;\n        #[allow(unreachable_code)]",
        ["the_close_button_closes_the_window"],
    ),
    (
        "the camera is handed no clock at all",
        "        Some(std::time::Duration::from_millis(TICK_MS))",
        "        None",
        ["the_clock_reaches_the_program_through_the_entry_point_the_platform_calls"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "camera", timeout=300, only=only))
