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

2026-10-04 added two groups.  The toolbar's sidebar and strip toggles were
flags nothing read -- the panes stayed, drawn and answering clicks, whatever
the toolbar said -- and `Layout::hiding` now gives their room to the picture.
And the settings' bars became lane C's slider (`guitk::slider`): a press on a
bar sets the setting there, a drag carries it, Escape takes it back, the thumb
lights under the pointer, and a drag whose bar the window takes away is taken
back too, so the bar that returns does not follow a pointer whose button is
up.

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
        "        fill(&mut f, l.window, self.palette.crust, CornerRadii::ZERO);",
        "        fill(&mut f, l.viewfinder, self.palette.crust, CornerRadii::ZERO);",
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
        "            _ => return EventResult::Ignored,\n"
        "        }\n"
        "        let frame = self.frame(self.width, self.height);",
        "            _ => {}\n"
        "        }\n"
        "        let frame = self.frame(self.width, self.height);",
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
        "[(parts.down, \"-\", Nudge::Down), (parts.up, \"+\", Nudge::Up)]",
        "[(parts.down, \"-\", Nudge::Up), (parts.up, \"+\", Nudge::Up)]",
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
    # 2026-09-27: a real build lists no camera, and takes nothing without one.
    (
        "a real build invents its cameras again",
        "            cameras: Vec::new(),",
        "            cameras: default_cameras(),",
        ["a_real_build_lists_no_camera_and_says_why", "without_a_camera_nothing_is_taken"],
    ),
    (
        "a photo is taken with no camera",
        "    pub fn take_photo(&mut self) {\n        if !self.has_live_camera() {",
        "    pub fn take_photo(&mut self) {\n        if false {",
        ["without_a_camera_nothing_is_taken"],
    ),
    (
        "a recording starts with no camera",
        "    pub fn start_recording(&mut self) {\n        if !self.has_live_camera() {",
        "    pub fn start_recording(&mut self) {\n        if false {",
        ["without_a_camera_nothing_is_taken"],
    ),
    (
        "no camera is drawn as a camera in error",
        "        if self.active_camera().is_none() {",
        "        if false {",
        ["a_real_build_lists_no_camera_and_says_why"],
    ),
    # -- the shortcut card's hold on the pointer
    (
        "a press goes through the shortcut card",
        "        if self.show_help {\n"
        "            // The card is modal for the pointer as it is for the keys: a\n",
        "        if false {\n"
        "            // The card is modal for the pointer as it is for the keys: a\n",
        ["the_shortcut_card_takes_a_press_rather_than_passing_it_on"],
    ),
    (
        "only the left button puts the card away",
        "            if matches!(mouse.kind, MouseEventKind::Press(_)) {\n"
        "                self.show_help = false;\n",
        "            if matches!(mouse.kind, MouseEventKind::Press(MouseButton::Left)) {\n"
        "                self.show_help = false;\n",
        ["the_shortcut_card_takes_a_press_rather_than_passing_it_on"],
    ),
    # -- 2026-10-04: the toolbar's two pane toggles, read at last -------------
    (
        "the pane toggles are read by nothing",
        "        let l = Layout::solve(w, h).hiding(!self.sidebar_visible, !self.photo_strip_visible);",
        "        let l = Layout::solve(w, h).hiding(false, false);",
        [
            "hiding_the_sidebar_and_the_strip_gives_their_room_to_the_picture",
            "hiding_only_the_strip_keeps_the_sidebar_full_height",
        ],
    ),
    (
        "the sidebar's toggle hides the strip and the strip's the sidebar",
        "        let l = Layout::solve(w, h).hiding(!self.sidebar_visible, !self.photo_strip_visible);",
        "        let l = Layout::solve(w, h).hiding(!self.photo_strip_visible, !self.sidebar_visible);",
        [
            "hiding_the_sidebar_and_the_strip_gives_their_room_to_the_picture",
            "hiding_only_the_strip_keeps_the_sidebar_full_height",
        ],
    ),
    (
        "the hidden sidebar's room is not given to the picture",
        "            viewfinder.w = self.window.w;\n",
        "",
        ["hiding_the_sidebar_and_the_strip_gives_their_room_to_the_picture"],
    ),
    (
        "the hidden sidebar is still laid out",
        "            side = Rect::new(self.window.w, self.viewfinder.y, 0.0, 0.0);\n",
        "",
        ["hiding_the_sidebar_and_the_strip_gives_their_room_to_the_picture"],
    ),
    (
        "the hidden strip's room is not given to the picture",
        "            viewfinder.h =\n"
        "                (self.strip.bottom().max(self.viewfinder.bottom()) - viewfinder.y).max(0.0);\n",
        "",
        [
            "hiding_the_sidebar_and_the_strip_gives_their_room_to_the_picture",
            "hiding_only_the_strip_keeps_the_sidebar_full_height",
        ],
    ),
    (
        "the hidden strip is still laid out",
        "            Rect::new(0.0, viewfinder.bottom(), 0.0, 0.0)\n        } else {",
        "            self.strip\n        } else {",
        ["hiding_the_sidebar_and_the_strip_gives_their_room_to_the_picture"],
    ),
    (
        "a kept strip keeps its old width under a hidden sidebar",
        "            Rect::new(0.0, viewfinder.bottom(), viewfinder.w, self.strip.h)",
        "            self.strip",
        ["hiding_the_sidebar_and_the_strip_gives_their_room_to_the_picture"],
    ),
    # -- 2026-10-04: the settings' bars are the toolkit's slider --------------
    (
        "the settings rows are laid out under every panel",
        "        if self.sidebar_panel != SidebarPanel::Settings {\n"
        "            return Vec::new();\n"
        "        }\n"
        "        let Some(body) = sidebar_body(l) else {",
        "        let Some(body) = sidebar_body(l) else {",
        ["the_bars_answer_only_while_the_settings_panel_is_up"],
    ),
    (
        "a settings row that does not fit is laid out anyway",
        "            if y + l.row > body.bottom() {\n                break;\n            }\n",
        "",
        # Not `nothing_is_painted_outside_the_window`: the rows left over
        # land on the status line, inside the window.
        ["the_settings_rows_stay_inside_the_sidebar"],
    ),
    (
        "a bar runs under its ends",
        "    let inset = thumb / 2.0 + reach;",
        "    let inset = 0.0;",
        ["a_bars_thumb_and_its_light_stay_between_its_ends_at_every_size"],
    ),
    (
        "a bar's thumb is as tall as it likes",
        "    let thumb = BAR_THUMB.min(lower.h - 2.0 * reach).min(mid.w);",
        "    let thumb = BAR_THUMB.min(mid.w);",
        ["a_bars_thumb_and_its_light_stay_between_its_ends_at_every_size"],
    ),
    (
        "white balance moved on its bar stays automatic",
        "                self.settings.auto_white_balance = false;\n"
        "                self.settings.set_white_balance(whole as u32);",
        "                self.settings.set_white_balance(whole as u32);",
        ["white_balance_moved_on_its_bar_is_no_longer_automatic"],
    ),
    (
        "zoom is rounded to a whole number like the rest",
        "            Setting::Zoom => self.settings.set_zoom(v as f32),",
        "            Setting::Zoom => self.settings.set_zoom(whole as f32),",
        ["zoom_keeps_its_tenths_on_its_bar"],
    ),
    (
        "a value from a bar is not held to its setting's range",
        "            v.clamp(lo, hi)\n        } else {",
        "            v\n        } else {",
        ["set_setting_holds_every_setting_to_its_range"],
    ),
    (
        "a value that is not a number reaches the setters as the bottom",
        "        } else {\n            self.setting_number(s)\n        };",
        "        } else {\n            lo\n        };",
        ["set_setting_holds_every_setting_to_its_range"],
    ),
    (
        "a bar's move is not said on the status line",
        "            Setting::Zoom => self.settings.set_zoom(v as f32),\n"
        "        }\n"
        "        let label = self.setting_value(s);\n"
        "        self.set_status(&format!(\"{}: {label}\", s.label()));",
        "            Setting::Zoom => self.settings.set_zoom(v as f32),\n"
        "        }",
        ["a_setting_follows_a_press_and_a_drag_along_its_bar"],
    ),
    (
        "a bar is drawn where it was last dragged rather than at its setting",
        "        if !bar.is_dragging() {\n"
        "            bar.set_value(self.setting_number(s));\n"
        "        }",
        "",
        ["a_bar_is_drawn_where_its_setting_is_whatever_moved_it"],
    ),
    (
        "zoom's bar reads the brightness",
        "            Setting::Zoom => f64::from(self.settings.zoom),",
        "            Setting::Zoom => f64::from(self.settings.brightness),",
        ["a_bar_is_drawn_where_its_setting_is_whatever_moved_it"],
    ),
    (
        "a press on a bar sets nothing",
        "        if let Some(event) = response.event() {\n"
        "            self.set_setting(s, event.value());\n"
        "        }\n"
        "        response.is_taken()",
        "        response.is_taken()",
        ["a_setting_follows_a_press_and_a_drag_along_its_bar"],
    ),
    (
        "a press on a bar starts no drag",
        "        if dragging {\n            self.dragging = Some(s);\n        } else if",
        "        if false {\n            self.dragging = Some(s);\n        } else if",
        ["a_setting_follows_a_press_and_a_drag_along_its_bar"],
    ),
    (
        "a bar's drag is never let go",
        "        } else if self.dragging == Some(s) {\n"
        "            self.dragging = None;\n"
        "        }\n"
        "        if let Some(event) = response.event() {",
        "        }\n"
        "        if let Some(event) = response.event() {",
        ["a_setting_follows_a_press_and_a_drag_along_its_bar"],
    ),
    (
        "a drag whose bar goes is kept by the slider",
        "            let response = self.setting_bars.get_mut(s.index()).map(Slider::cancel);\n"
        "            if let Some(event) = response.and_then(guitk::slider::Response::event) {\n"
        "                self.set_setting(s, event.value());\n"
        "            }\n",
        "",
        ["a_drag_whose_bar_goes_is_taken_back_and_lets_the_pointer_go"],
    ),
    (
        "a drag whose bar goes keeps the pointer",
        "            self.dragging = None;\n"
        "            let response = self.setting_bars.get_mut(s.index()).map(Slider::cancel);",
        "            let response = self.setting_bars.get_mut(s.index()).map(Slider::cancel);",
        ["a_drag_whose_bar_goes_is_taken_back_and_lets_the_pointer_go"],
    ),
    (
        "no bar's thumb ever lights",
        "            lit |= self.bar_mouse(*s, mouse);",
        "            let _ = s;",
        ["a_bars_thumb_lights_under_the_pointer_and_goes_out_after"],
    ),
    (
        "a move is never offered to the bars",
        "                return if self.hover_bars(mouse) {",
        "                return if false {",
        ["a_bars_thumb_lights_under_the_pointer_and_goes_out_after"],
    ),
    (
        "a drag's pointer goes where any pointer goes",
        "        if let Some(setting) = self.dragging {\n"
        "            return if self.bar_mouse(setting, mouse) {",
        "        if let Some(setting) = None::<Setting> {\n"
        "            return if self.bar_mouse(setting, mouse) {",
        ["a_setting_follows_a_press_and_a_drag_along_its_bar"],
    ),
    (
        "a press on a bar is answered like a press on a button",
        "            Some(Target::SettingBar(setting)) => {\n"
        "                self.bar_mouse(setting, mouse);",
        "            Some(Target::SettingBar(setting)) => {\n"
        "                self.activate(Target::SettingBar(setting));",
        ["a_setting_follows_a_press_and_a_drag_along_its_bar"],
    ),
    (
        "every bar's hit box is brightness's",
        "                    f.hit(Target::SettingBar(setting), hit);",
        "                    f.hit(Target::SettingBar(Setting::Brightness), hit);",
        [
            "zoom_keeps_its_tenths_on_its_bar",
            "white_balance_moved_on_its_bar_is_no_longer_automatic",
        ],
    ),
    (
        "a key mid-drag is answered as if there were no drag",
        "        if let Some(result) = self.drag_key(key) {\n            return result;\n        }\n",
        "",
        ["escape_takes_a_bars_drag_back_and_other_keys_wait"],
    ),
    (
        "Escape takes the drag back and the setting keeps the drag's value",
        "        if let Some(event) = response.event() {\n"
        "            self.set_setting(s, event.value());\n"
        "        }\n"
        "        Some(if response.is_taken() {",
        "        Some(if response.is_taken() {",
        ["escape_takes_a_bars_drag_back_and_other_keys_wait"],
    ),
    (
        "a drag Escape took back still has the pointer",
        "        let response = stored.handle_key(key);\n"
        "        if !stored.is_dragging() {\n"
        "            self.dragging = None;\n"
        "        }\n",
        "        let response = stored.handle_key(key);\n",
        ["escape_takes_a_bars_drag_back_and_other_keys_wait"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "camera", timeout=300, only=only))
