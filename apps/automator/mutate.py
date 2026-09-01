"""Mutation test for automator's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Automator is the forty-fourth application in this campaign.  It had 104 tests,
every one of them about macros -- what a script parses to, how long a playback
takes at double speed, whether a hotkey round-trips through its own string form
-- and not one about the window, because there was no window.  `main` was:

    fn main() {
        let mut app = AutomatorApp::new();
        app.new_macro("Login Sequence");
        ... eighteen `add_action` calls ...
        let _commands = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
    }

It built two demo macros, rendered one frame into a `Vec`, dropped it on the
next line and returned.  Nothing was ever shown, no click or key ever reached
the program, and no clock ever ran.

What that hid, in rough order of how badly it would have shown:

  * **The picture was drawn at one size in a window of any other.**  Every
    rectangle was a compile-time constant: `HEADER_HEIGHT = 44`,
    `SIDEBAR_WIDTH = 240`, `PROPERTIES_WIDTH = 260`, `TOOLBAR_HEIGHT = 40`.
    The centre panel was literally `width - 240.0 - 260.0`, which is a
    *negative* width in any window narrower than five hundred pixels, and the
    properties panel began at `width - 260.0`, which is off the left edge of
    anything narrower than that.  Nothing measured anything vertical with the
    height at all.
  * **Nothing was clickable.**  Not one control in the program recorded a hit
    box, because there was nowhere for a click to come from.  Fourteen
    commands, five speed buttons, three repeat buttons, two tabs and every row
    of both lists were painted and unreachable.
  * **The two scroll offsets were written once and never again.**
    `sidebar_scroll` and `action_list_scroll` were `f32` fields assigned zero
    in `new()`, read by the drawing pass, and assigned nowhere else in the
    program.  A library with more macros than fit was permanently truncated at
    whatever the window could show.
  * **`tick_playback` had no caller outside the tests.**  A macro started
    playing and then sat there: nothing in the program advanced it.
  * **`active_tab` was assigned in exactly one place, and that place was a
    test.**  The script editor could not be reached.
  * **The speed and repeat pads were laid out from a fixed offset.**  The
    speed section was at `content_y + content_h - 100.0` with the repeat row
    80 px below it and 24 px tall, so the pads ended four pixels *below* the
    properties pane, painted over the status bar.
  * **The panel footers were drawn at `content_y + content_h - 36.0`
    unconditionally.**  In a short window the New/Delete bar was not under the
    library, it was on top of it -- and the rows it covered were drawn all the
    same, under a bar that hid them.
  * **Every text was `max_width: None`.**  A macro name, an action summary,
    the status message and the header's playing read-out all ran straight off
    a narrow window, and the panel they were nominally in did not bound them.
  * **Thirteen crate-level `#![allow]`s hid 62 lints.**

A postscript, added when lane B filed
`requests/b-c-automator-never-receives-the-clock.md`: the fourth bullet above
was fixed *by half*.  `tick` was written, `tick_playback` acquired a caller,
and four rows of this table broke `tick` and were caught -- but `handle_event`
never grew an `Event::Tick` arm, so the compositor's ticks fell into
`_ => Ignored` and nothing in the running program ever called `tick` at all.
Every one of those four rows passed against a program whose clock was dead,
because every test behind them called `tick` directly.  The row "the tick never
reaches the app" now breaks the *door* rather than the worker behind it, and is
caught by four tests -- none of them the two that guarded `tick` before.  That
is lesson 102 in its purest form: test the entry point the platform calls.

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
    # -- the window itself ---------------------------------------------------
    (
        "a close request does not close the window",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Exit;\n        }",
        "        if matches!(event, Event::CloseRequested) {\n            return Response::Idle;\n        }",
        ["a_close_request_closes_the_window"],
    ),
    (
        "the app never asks for a clock, so playback never advances",
        "        // outside the tests.\n        Some(std::time::Duration::from_millis(TICK_MS))",
        "        // outside the tests.\n        None",
        ["the_app_asks_for_a_clock_and_a_running_macro_advances_on_it"],
    ),
    (
        "every tick asks for a repaint, sixty times a second for ever",
        "        fired || was_playing || self.recording_state.is_recording()",
        "        let _ = fired;\n        let _ = was_playing;\n        true",
        ["a_tick_asks_for_a_repaint_only_while_something_is_moving"],
    ),
    (
        "a running macro is not advanced by the clock",
        "        let fired = self.tick_playback(delta_ms).is_some();",
        "        let fired = false;",
        ["the_app_asks_for_a_clock_and_a_running_macro_advances_on_it"],
    ),
    # -- the clock the window delivers ---------------------------------------
    # Filed by lane B as `requests/b-c-automator-never-receives-the-clock.md`:
    # `handle_event` had no `Event::Tick` arm at all, so every tick the
    # compositor delivered fell into `_ => Ignored`.  The four rows above this
    # one all mutate `tick`, and `tick` was never reached from the window --
    # which is exactly why they all passed against a frozen program.  These
    # rows break the *door*, not the worker behind it.
    (
        "the tick never reaches the app, because the window's arm is missing",
        "            Event::Tick { elapsed_ms } => {\n                // `tick` reports whether anything moved; a tick that changed\n                // nothing must not ask for a frame, or an idle automator\n                // repaints at the tick rate for ever. That is the same\n                // distinction `on_event` maps onto `Redraw`/`Idle`, so the\n                // answer here needs nothing decided that `tick` has not\n                // already decided.\n                if self.tick(*elapsed_ms) {\n                    EventResult::Consumed\n                } else {\n                    EventResult::Ignored\n                }\n            }\n            _ => EventResult::Ignored,",
        "            _ => EventResult::Ignored,",
        ["the_clock_reaches_the_playback_through_the_door_the_window_knocks_on"],
    ),
    (
        "every tick delivered as an event is consumed, so an idle desktop repaints",
        "                if self.tick(*elapsed_ms) {\n                    EventResult::Consumed\n                } else {\n                    EventResult::Ignored\n                }",
        "                self.tick(*elapsed_ms);\n                EventResult::Consumed",
        ["a_tick_delivered_as_an_event_is_consumed_only_while_something_is_moving"],
    ),
    (
        "the elapsed clock is overwritten by each interval rather than summing them",
        "        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);",
        "        self.elapsed_ms = delta_ms;",
        ["the_elapsed_clock_advances_on_ticks_that_arrive_as_events"],
    ),
    (
        "a macro is stamped with the age of the window instead of the date",
        "        let id = self.library.create_macro(name, self.wall_ms);",
        "        let id = self.library.create_macro(name, self.elapsed_ms);",
        ["a_new_macro_is_stamped_with_the_date_not_the_age_of_the_window"],
    ),
    (
        "the wall clock is read once at startup and never again",
        "        if let Some(now) = now_ms() {\n            self.wall_ms = now;\n        }",
        "        if let Some(now) = now_ms() {\n            let _ = now;\n        }",
        ["the_date_is_read_again_on_the_clock_not_once_when_the_window_opened"],
    ),
    (
        "the window opens at a size the layout cannot afford both panels at",
        "    fn initial_size(&self) -> (u32, u32) {\n        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)",
        "    fn initial_size(&self) -> (u32, u32) {\n        (320, 240)",
        ["the_window_opens_at_the_size_the_layout_was_designed_for"],
    ),
    (
        "the picture is drawn at the size it remembers, not the size it is given",
        "        self.size = (width, height);\n        self.frame(width, height).into_tree()",
        "        let (w, h) = self.size;\n        self.size = (width, height);\n        self.frame(w, h).into_tree()",
        ["the_picture_is_drawn_at_the_size_it_is_given_not_the_size_it_remembers"],
    ),
    # -- the layout ----------------------------------------------------------
    (
        "the list is charged for a panel that was dropped for want of height",
        "        let list = Rect::new(\n            sidebar.w,\n            content_y,\n            (w - sidebar.w - props.w).max(0.0),\n            content_h,\n        );",
        "        let list = Rect::new(\n            sidebar_w,\n            content_y,\n            (w - sidebar_w - props_w).max(0.0),\n            content_h,\n        );",
        ["the_panels_and_the_list_fill_the_width_between_them"],
    ),
    (
        "a side panel is taken however little it leaves the list",
        "        if sidebar_w < MIN_PANEL_W || w - sidebar_w < MIN_LIST_W {\n            sidebar_w = 0.0;\n        }",
        "        if sidebar_w < 0.0 {\n            sidebar_w = 0.0;\n        }",
        ["the_list_is_never_given_up_for_a_side_panel"],
    ),
    (
        "the properties panel is squeezed onto a window that cannot hold it",
        "        if props_w < MIN_PANEL_W || w - sidebar_w - props_w < MIN_LIST_W {\n            props_w = 0.0;\n        }",
        "        if props_w < 0.0 {\n            props_w = 0.0;\n        }",
        ["the_list_is_never_given_up_for_a_side_panel"],
    ),
    (
        "the header may be taller than the window",
        "        let header = Rect::new(0.0, 0.0, w, (heading + pad * 2.2).min(h));",
        "        let header = Rect::new(0.0, 0.0, w, heading + pad * 2.2);",
        ["every_pane_stays_inside_the_window"],
    ),
    (
        "the toolbar may be taller than the window below the header",
        "        let toolbar_h = (button + pad * 1.4).min((h - header.h).max(0.0));",
        "        let toolbar_h = button + pad * 1.4;",
        ["every_pane_stays_inside_the_window"],
    ),
    (
        "the status bar may be taller than what the header and toolbar left",
        "        let status_h = (small + pad * 1.2).min((h - header.h - toolbar_h).max(0.0));",
        "        let status_h = small + pad * 1.2;",
        ["every_pane_stays_inside_the_window"],
    ),
    (
        "the properties panel runs off the right-hand edge",
        "            Rect::new(w - props_w, content_y, props_w, content_h)",
        "            Rect::new(w - props_w + 40.0, content_y, props_w, content_h)",
        ["every_pane_stays_inside_the_window"],
    ),
    (
        "a footer eats the body it is the foot of",
        "        let foot_h = foot_h.min(rest * 0.5).max(0.0);",
        "        let foot_h = foot_h.max(0.0);",
        ["a_footer_never_eats_the_body_it_is_the_foot_of"],
    ),
    (
        "a panel's heading may be taller than the panel",
        "        let head_h = head_h.min(panel.h);",
        "        let head_h = head_h;",
        ["a_footer_never_eats_the_body_it_is_the_foot_of"],
    ),
    # -- the drawing pass ----------------------------------------------------
    (
        "the toolbar marches past the right-hand edge of a narrow window",
        "            if bx + w > bar.right() - l.pad {\n                break;\n            }",
        "            if bx + w > f32::INFINITY {\n                break;\n            }",
        ["nothing_is_painted_outside_the_window"],
    ),
    (
        "an action row's number column takes the width it wants",
        '            let num_w =\n                text::measure("000", l.small, FontWeightHint::Regular).min((right - cx).max(0.0));',
        '            let num_w = text::measure("000", l.small, FontWeightHint::Regular);',
        ["no_text_runs_off_the_window_it_is_drawn_in"],
    ),
    (
        "an action row's badge takes the width it wants",
        "            )\n            .min((right - cx).max(0.0));\n            let badge_h = (rect.h - 4.0).max(0.0);",
        "            );\n            let badge_h = (rect.h - 4.0).max(0.0);",
        ["nothing_is_painted_outside_the_window"],
    ),
    (
        "the script gutter is as wide as three digits however narrow the window",
        '        let gutter = (text::measure("000", l.small, FontWeightHint::Regular) + l.pad)\n            .min((text_area.w - l.pad).max(0.0));',
        '        let gutter = text::measure("000", l.small, FontWeightHint::Regular) + l.pad;',
        ["no_text_runs_off_the_window_it_is_drawn_in"],
    ),
    (
        "a list row is drawn even when only half of it fits",
        "            if row_y + l.row > body.bottom() + 0.01 {\n                break;\n            }\n            let rect = Rect::new(body.x + 2.0, row_y, (body.w - 4.0).max(0.0), l.row - 2.0);\n            let selected = self.selected_action_idx == Some(i);",
        "            if row_y > body.bottom() + 0.01 {\n                break;\n            }\n            let rect = Rect::new(body.x + 2.0, row_y, (body.w - 4.0).max(0.0), l.row - 2.0);\n            let selected = self.selected_action_idx == Some(i);",
        ["every_hit_box_lies_in_the_band_of_the_pane_that_owns_it"],
    ),
    (
        "the help card is drawn bigger than the window that holds it",
        "        let card = Rect::new(0.0, 0.0, wanted_w.min(l.window.w), wanted_h.min(l.window.h));",
        "        let card = Rect::new(0.0, 0.0, wanted_w, wanted_h);",
        ["the_help_card_stays_inside_the_window_that_holds_it"],
    ),
    (
        "the help card lists only the buttons that happen to fit",
        "        let rows = Button::all();",
        "        let rows = &Button::all()[..4];",
        ["the_help_card_lists_every_button_and_the_key_it_shares"],
    ),
    (
        "the help card does not swallow the click that dismisses it",
        "        f.hit(Target::Help, card);",
        "        let _ = card;",
        ["the_help_card_swallows_the_click_that_dismisses_it"],
    ),
    (
        "an empty library shows a blank panel under a heading",
        '                "No macros yet -- press New",',
        '                "",',
        ["an_empty_library_says_so_rather_than_showing_nothing"],
    ),
    (
        "the status bar does not report the recorder and the player",
        '        let state = format!(\n            "Rec: {} | Play: {}",\n            self.recording_state.label(),\n            self.playback_state.label()\n        );',
        "        let state = String::new();",
        ["the_status_bar_says_what_is_recording_and_what_is_playing"],
    ),
    (
        "the status message is bounded by the whole bar rather than by what is left",
        "            right - state_w - l.pad - mx,\n            &self.status_message,",
        "            bar.w,\n            &self.status_message,",
        ["no_text_runs_off_the_window_it_is_drawn_in"],
    ),
    (
        "the read-out does not say what the selected action is",
        '                    if prop_row(f, l, body, &mut cy, "Action", timed.action.icon()) {',
        "                    if false {",
        ["the_read_out_says_what_the_selected_action_is"],
    ),
    (
        "the read-out does not name the macro it is reporting on",
        '            let mut more = prop_row(f, l, body, &mut cy, "Name", &mac.name);',
        '            let mut more = prop_row(f, l, body, &mut cy, "Name", "");',
        ["the_read_out_says_what_the_selected_macro_is"],
    ),
    (
        "a property row is written past the bottom of the panel it is in",
        "    if *cy + row_h > body.bottom() + 0.01 {\n        return false;\n    }",
        "    if *cy + row_h > f32::INFINITY {\n        return false;\n    }",
        ["the_property_rows_stay_clear_of_the_strip_the_pads_are_in"],
    ),
    (
        "the library does not name the macros it lists",
        "                    &mac.name,\n                    if selected { TEXT } else { SUBTEXT1 },",
        '                    "",\n                    if selected { TEXT } else { SUBTEXT1 },',
        ["the_sidebar_names_every_macro_it_has_room_for"],
    ),
    (
        "a script that does not parse says nothing about where it went wrong",
        "                    err,\n                    RED,",
        '                    "",\n                    RED,',
        ["a_script_that_does_not_parse_says_where_it_went_wrong"],
    ),
    (
        "a centred run too wide to centre hangs off both sides",
        "    let offset = ((w - measured) / 2.0).max(0.0);",
        "    let offset = (w - measured) / 2.0;",
        ["no_text_runs_off_the_window_it_is_drawn_in"],
    ),
    (
        "a run with no room left is drawn anyway, outside the box that has none",
        "    if width <= 0.0 || width.is_nan() {\n        return;\n    }",
        "    if width.is_nan() {\n        return;\n    }",
        ["no_text_runs_off_the_window_it_is_drawn_in"],
    ),
    # -- hit boxes -----------------------------------------------------------
    (
        "the library rows are hit-boxed where they are not drawn",
        "            f.hit(Target::Macro(i), rect);",
        "            f.hit(Target::Macro(i), rect.translated(0.0, -l.row));",
        ["every_hit_box_has_ink_painted_at_exactly_that_rectangle"],
    ),
    (
        "the action rows are hit-boxed where they are not drawn",
        "            f.hit(Target::Action(i), rect);",
        "            f.hit(Target::Action(i), rect.translated(0.0, -l.row));",
        ["every_hit_box_has_ink_painted_at_exactly_that_rectangle"],
    ),
    (
        "the toolbar buttons are hit-boxed where they are not drawn",
        "            f.hit(Target::Button(button), rect);",
        "            f.hit(Target::Button(button), rect.translated(-l.pad * 4.0, 0.0));",
        ["every_hit_box_has_ink_painted_at_exactly_that_rectangle"],
    ),
    (
        "the speed pads are hit-boxed where they are not drawn",
        "            f.hit(Target::Speed(s), rect);",
        "            f.hit(Target::Speed(s), rect.translated(0.0, -rect.h));",
        ["every_hit_box_has_ink_painted_at_exactly_that_rectangle"],
    ),
    (
        "the tab strip is not clickable at all",
        "            f.hit(Target::Tab(*tab), rect);",
        "            let _ = rect;",
        ["every_control_that_is_painted_can_be_clicked"],
    ),
    (
        "the repeat pads are not clickable at all",
        "            f.hit(Target::Repeat(m), rect);",
        "            let _ = rect;",
        ["every_control_that_is_painted_can_be_clicked"],
    ),
    # The `pads.is_empty()` and `each <= 0.0` guards below were mutated away in
    # the first sweep and nothing failed, which was honest: neither is
    # load-bearing. An empty rectangle refuses every fill and every run
    # downstream of it, and `Frame::hit` drops an empty hit box, so a panel
    # that was dropped cannot paint or answer anything whether the guard
    # returns early or not. What *is* load-bearing is where the pads and the
    # footer's buttons are put, so that is what these two rows mutate now.
    (
        "the pads are painted over the property rows rather than in their own strip",
        "        self.draw_pads(f, l, pads);",
        "        self.draw_pads(f, l, body);",
        ["every_hit_box_lies_in_the_band_of_the_pane_that_owns_it"],
    ),
    (
        "the footer buttons take a fixed width and march past the panel's edge",
        "        let each = (foot.w - l.pad * 2.0 - gaps) / usize_f32(buttons.len());",
        "        let each = 60.0_f32;",
        ["every_hit_box_lies_in_the_band_of_the_pane_that_owns_it"],
    ),
    # -- events --------------------------------------------------------------
    (
        "a click is answered by the last frame drawn rather than this window's",
        "                let frame = self.frame(w, h);",
        "                let frame = self.frame(WINDOW_WIDTH, WINDOW_HEIGHT);",
        ["a_click_lands_on_what_was_drawn_in_that_window_not_the_last_one"],
    ),
    (
        "a click on nothing is answered by the last thing that was drawn",
        "                let Some(target) = frame.hit_test(mouse.x, mouse.y) else {\n                    return EventResult::Ignored;\n                };",
        "                let Some(target) = frame\n                    .hit_test(mouse.x, mouse.y)\n                    .or_else(|| frame.hits().last().map(|(t, _)| *t))\n                else {\n                    return EventResult::Ignored;\n                };",
        ["a_click_on_nothing_changes_nothing"],
    ),
    (
        "a key with a modifier on it is taken from the window manager",
        "        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_key_with_a_modifier_is_left_to_the_window"],
    ),
    (
        "a key release is handled as though it were a press",
        "        if !key.pressed {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_key_release_is_not_a_key_press"],
    ),
    (
        "a button does not do what its key does",
        "                for button in Button::all() {\n                    if button.key() == k {\n                        self.press(*button);\n                        return EventResult::Consumed;\n                    }\n                }",
        "                for button in Button::all() {\n                    if button.key() == k {\n                        self.press(Button::Help);\n                        return EventResult::Consumed;\n                    }\n                }",
        ["every_button_does_what_its_key_does"],
    ),
    (
        "a button's face names a key that does something else",
        '    format!("{} ({})", button.action_label(), button.key_label())',
        '    format!("{} (Z)", button.action_label())',
        ["every_button_names_the_key_that_does_the_same_thing"],
    ),
    (
        "clicking a tab does not show that tab",
        "            Target::Tab(tab) => {\n                self.active_tab = tab;",
        "            Target::Tab(tab) => {\n                self.active_tab = ActiveTab::Editor;\n                let _ = tab;",
        ["clicking_a_tab_shows_that_tab"],
    ),
    (
        "clicking a speed pad does not set the speed",
        "            Target::Speed(speed) => {\n                self.set_speed(speed);",
        "            Target::Speed(speed) => {\n                let _ = speed;",
        ["clicking_a_speed_or_a_repeat_pad_sets_it"],
    ),
    (
        "clicking a repeat pad does not set the repeat mode",
        "            Target::Repeat(mode) => {\n                self.set_repeat_mode(mode);",
        "            Target::Repeat(mode) => {\n                let _ = mode;",
        ["clicking_a_speed_or_a_repeat_pad_sets_it"],
    ),
    (
        "clicking a macro selects a different one",
        "            Target::Macro(i) => {\n                self.select_macro_by_index(i);",
        "            Target::Macro(i) => {\n                self.select_macro_by_index(i.saturating_add(1));",
        ["clicking_a_macro_selects_that_macro"],
    ),
    # -- scrolling and selection --------------------------------------------
    (
        "the wheel over the library scrolls nothing",
        "                if l.sidebar.contains(mouse.x, mouse.y) {\n                    self.scroll_sidebar(rows)",
        "                if false {\n                    self.scroll_sidebar(rows)",
        ["a_wheel_over_the_library_scrolls_the_library"],
    ),
    (
        "the wheel over the action list scrolls nothing",
        "                } else if l.list.contains(mouse.x, mouse.y) {\n                    self.scroll_actions(rows)",
        "                } else if false {\n                    self.scroll_actions(rows)",
        ["a_wheel_over_the_action_list_scrolls_the_action_list"],
    ),
    (
        "the wheel scrolls whatever it is over, including the header",
        "                    self.scroll_actions(rows)\n                } else {\n                    EventResult::Ignored\n                }",
        "                    self.scroll_actions(rows)\n                } else {\n                    self.scroll_actions(rows)\n                }",
        ["a_wheel_over_nothing_scrolls_nothing"],
    ),
    (
        "the drawing pass ignores the offset the wheel set",
        "        let first = self.action_scroll.min(mac.actions.len());",
        "        let first = 0;",
        ["the_scrolled_list_shows_the_rows_the_offset_names"],
    ),
    (
        "a scroll offset is left past the end when the list shrinks under it",
        "        self.sidebar_scroll = self\n            .sidebar_scroll\n            .min(self.library.count().saturating_sub(1));",
        "        self.sidebar_scroll = self.sidebar_scroll;",
        ["a_list_that_shrinks_under_its_offset_pulls_the_offset_back"],
    ),
    (
        "the action offset is left past the end when the macro changes under it",
        "        self.action_scroll = self.action_scroll.min(actions.saturating_sub(1));",
        "        self.action_scroll = self.action_scroll;",
        ["an_action_offset_survives_the_macro_changing_under_it"],
    ),
    (
        "a click leaves the offsets pointing into the list it just replaced",
        "        let result = self.click_inner(target);\n        self.clamp_scrolls();",
        "        let result = self.click_inner(target);",
        ["an_action_offset_survives_the_macro_changing_under_it"],
    ),
    (
        "the list never follows the selection down, only back up",
        "            let first = first_offset_showing(i, list_rows);\n            if first > self.action_scroll {",
        "            let first = first_offset_showing(i, list_rows);\n            if false {",
        ["the_arrow_keys_move_the_selection_and_the_selection_stays_on_screen"],
    ),
    (
        "the list does not follow the selection back up",
        "        if let Some(i) = self.selected_action_idx\n            && i < self.action_scroll\n        {\n            self.action_scroll = i;\n        }",
        "        if let Some(i) = self.selected_action_idx\n            && i < 0_usize\n        {\n            self.action_scroll = i;\n        }",
        ["the_arrow_keys_move_the_selection_and_the_selection_stays_on_screen"],
    ),
    (
        "the arrow keys do not move the selection",
        "            Key::Up => self.move_action_selection(-1),\n            Key::Down => self.move_action_selection(1),",
        "            Key::Up => EventResult::Ignored,\n            Key::Down => EventResult::Ignored,",
        ["the_arrow_keys_move_the_selection_and_the_selection_stays_on_screen"],
    ),
    (
        "the selection may walk off the end of the list",
        "        let next = step(self.selected_action_idx, delta, n);",
        "        let next = self\n            .selected_action_idx\n            .map_or(0, |i| i.saturating_add_signed(delta));",
        ["the_selection_never_names_a_row_that_does_not_exist"],
    ),
    # --- The three faults the per-pass containment sweep turned up (lesson 108).
    # None of them is visible to the window-level tests: the window bounds a
    # run's left and right edges and never its top and bottom, and it bounds a
    # fill against the *window*, which every pass is well inside.
    (
        "the script error strip is hung off the text area instead of the body",
        "                body.bottom() - err_h,",
        "                text_area.bottom(),",
        # The two agree exactly until `text_area`'s height clamps at zero, which
        # is what a body shorter than its own four points of padding does.
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the help card's heading is inked whether or not the card has room",
        "        if y + l.font <= card.bottom() {",
        "        if true {",
        # The card is clamped to the window, so this one escapes the window too
        # -- and still no window test sees it, because it is a *run* and the
        # only vertical bound in the suite is on fills.
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "a line is centred in a band too short to hold it",
        "fn centre_line(band: Rect, size: f32) -> Option<f32> {\n    (band.h + 0.01 >= size).then(|| band.y + (band.h - size) / 2.0)\n}",
        "fn centre_line(band: Rect, size: f32) -> Option<f32> {\n    Some(band.y + (band.h - size) / 2.0)\n}",
        # Centring alone puts half the line above the band's top edge and the
        # rest below its bottom, in every heading strip, footer button and row
        # in the program at once.
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the recording dot is centred in the header at its nominal size",
        "            let dot = dot.min(head.h);",
        "",
        # A fill, and therefore the one fault of these six that a *window* test
        # could in principle have caught -- except that the header is nowhere
        # near the window's bottom edge, so it paints on the toolbar and the
        # window is none the wiser. It needed the sliver height in `GRID_H`:
        # between an empty header and one a whole line tall there is a band of
        # sizes the grid did not sample at all.
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the pads' headings are placed by offset instead of measured in a band",
        "        if let Some(ty) = centre_line(speed_head, l.small) {",
        "        if let Some(ty) = Some(pads.y + (quarter - l.small) / 2.0) {",
        # This is the form the two headings were written in. It reads as a
        # centring and is not one: a quarter of a squeezed properties panel is
        # shorter than a line, and the heading then sits above the strip.
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
    (
        "the playing marker is three points wide whatever the row measures",
        "                    Rect::new(rect.x, rect.y, 3.0_f32.min(rect.w), rect.h),",
        "                    Rect::new(rect.x, rect.y, 3.0, rect.h),",
        # A literal width, and the only thing in an action row that no other
        # measurement bounds.
        ["no_pass_paints_outside_the_region_it_owns"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "automator", timeout=300, only=only))
