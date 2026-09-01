"""Mutation test for the alarm clock's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The alarm clock was wired to a window early in this campaign, before the three
geometry sweeps existed and before any app in it carried a mutation table.  It
came out of that with 175 tests and not one of them asking *where the paint
landed* -- which is exactly the blind spot Lesson 107 in `known-issues.md` was
written about, and this app turned out to be the worst case of it in the tree:

  * It rolls its own `text(..)` and `fill(..)` helpers straight onto
    `RenderCommand`, where `guitk::put_text` would have refused to emit a run
    the clip in force cannot show.  So an overrunning pass here reached the
    *picture*, not merely the pixels: a label four hundred points below its
    pane still entered the frame claiming to be on screen, and anything that
    reads the frame to find out what is displayed was told a lie.
  * All three of its scrolling panes -- the lap table, the alarm list, the
    timer list -- walked their whole collection and drew every item, with the
    clip as the only thing standing between the overrun and the screen.
  * `text_centred` declared its `max_width` from where the run *starts* rather
    than from the box it was told to centre in, so every centred run was
    declared to overhang its box by half the slack.  "Stopwatch" in a
    120-point tab was declared to run to 385 in a 360-point window.
  * The alarm editor laid its stack out at a fixed 308 points.  A window at
    this app's own minimum size has a 248-point content area, so Save and
    Cancel were painted below the panel, hidden by the clip and with their hit
    boxes dropped by `Frame::hit` for having nothing visible.  The editor
    covers the alarm tab: with neither button reachable it was a trap the
    pointer could not get out of.

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
    # ---- where the paint lands ----
    # The lap table's matching guard has deliberately no row here.  It was one,
    # and it survived: a lap row is three runs of text and nothing else, and
    # once `text` began refusing runs the clip cannot show, a walk of the whole
    # lap list produces byte-for-byte the frame the guarded walk does.  The
    # guard is real and stays -- it is what stops the pass formatting three
    # strings per lap on every frame, over a list nothing bounds -- but it is a
    # work bound, and no assertion over the picture can witness a bound on work.
    # Weakening some other test until it noticed would have been a lie about
    # what that test checks, so the row is recorded here instead.
    (
        "the alarm list draws every card it holds, pane or no pane",
        "            if y - self.alarm_scroll >= list.bottom() {\n"
        "                break;\n"
        "            }\n"
        "            if y - self.alarm_scroll + card_h > list.y {\n"
        "                alarm.draw(f, list.x, y, list.w, self.time_format);\n"
        "            }\n",
        "            alarm.draw(f, list.x, y, list.w, self.time_format);\n",
        ["nothing_is_painted_entirely_outside_the_clip_in_force"],
    ),
    (
        "the timer list draws every card it holds, pane or no pane",
        "            if y - self.timer_scroll >= list.bottom() {\n"
        "                break;\n"
        "            }\n"
        "            if y - self.timer_scroll + TIMER_ROW_H <= list.y {\n"
        "                continue;\n"
        "            }\n",
        "",
        ["nothing_is_painted_entirely_outside_the_clip_in_force"],
    ),
    (
        "a run of text is emitted wherever the caller asks, clip or no clip",
        "    if !f.is_visible(Rect::new(x, y, bound, size)) {\n        return;\n    }\n",
        "",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "a centred run declares its bound from where it starts",
        "    text(f, left, y, body, color, size, weight, measured);",
        "    text(f, left, y, body, color, size, weight, width);",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "the editor is laid out at its natural height whatever it was given",
        "        let s = if ratio.is_finite() {\n"
        "            ratio.clamp(0.0, 1.0)\n"
        "        } else {\n"
        "            1.0\n"
        "        };",
        "        let s = 1.0;",
        [
            "the_editor_can_always_be_left_by_pointer",
            "the_editor_is_laid_out_inside_the_panel_it_was_given",
        ],
    ),
    # ---- the window itself ----
    (
        "the window asks never to be ticked",
        "    fn tick_interval(&self) -> Option<Duration> {\n"
        "        Some(if self.stopwatch.state == StopwatchState::Running {\n"
        "            TICK_FAST\n"
        "        } else {\n"
        "            TICK_SLOW\n"
        "        })\n"
        "    }",
        "    fn tick_interval(&self) -> Option<Duration> {\n        None\n    }",
        ["the_tick_interval_is_never_none"],
    ),
    (
        "a tick advances the clock by a fixed step rather than by what elapsed",
        "            Event::Tick { elapsed_ms } => self.tick(*elapsed_ms),",
        "            Event::Tick { elapsed_ms: _ } => self.tick(500),",
        ["a_late_tick_advances_by_what_actually_elapsed"],
    ),
    # ---- the tab strip and the clock band ----
    (
        "every tab in the strip selects the alarm tab",
        "            f.hit(Target::Tab(tab), rect);",
        "            f.hit(Target::Tab(ActiveTab::Alarm), rect);",
        ["clicking_a_tab_selects_it"],
    ),
    (
        "the clock band adds an alarm rather than swapping the format",
        "        f.hit(Target::ClockFormat, clock);",
        "        f.hit(Target::AddAlarm, clock);",
        ["clicking_the_clock_swaps_the_time_format"],
    ),
    # ---- the alarm list ----
    (
        "add creates an alarm blind rather than opening the editor",
        "            Target::AddAlarm => self.open_new_alarm(),",
        "            Target::AddAlarm => {\n                let _ = self.create_alarm(9, 0);\n            }",
        ["add_alarm_opens_the_editor_rather_than_creating_one_blind"],
    ),
    (
        "the cross on a card toggles the alarm instead of deleting it",
        "        f.hit(Target::AlarmDelete(self.id), delete);",
        "        f.hit(Target::AlarmToggle(self.id), delete);",
        ["the_alarm_pill_and_cross_do_what_they_say"],
    ),
    (
        "the body of a card toggles the alarm instead of opening it",
        "        f.hit(Target::AlarmRow(self.id), card);",
        "        f.hit(Target::AlarmToggle(self.id), card);",
        ["clicking_an_alarm_card_opens_it_for_editing"],
    ),
    (
        "deleting the alarm under an open editor leaves the editor open",
        "                if self.editor.as_ref().and_then(|e| e.editing) == Some(id) {\n"
        "                    self.cancel_editor();\n"
        "                }\n",
        "",
        ["deleting_the_alarm_under_an_open_editor_closes_it"],
    ),
    # ---- the editor ----
    (
        "cancel saves the edit it was asked to throw away",
        "            Target::EditCancel => {\n"
        "                if self.editor.is_none() {\n"
        "                    return Action::None;\n"
        "                }\n"
        "                self.cancel_editor();\n"
        "            }",
        "            Target::EditCancel => {\n"
        "                if self.editor.is_none() {\n"
        "                    return Action::None;\n"
        "                }\n"
        "                self.save_editor();\n"
        "            }",
        ["cancelling_an_edit_leaves_the_alarm_untouched"],
    ),
    (
        "every repeat-day chip toggles Monday",
        "                    if let Some(slot) = editor.repeat_days.get_mut(day.index()) {",
        "                    if let Some(slot) = editor.repeat_days.get_mut(0) {",
        ["saving_the_editor_creates_the_alarm_it_shows"],
    ),
    (
        "the label field takes as many characters as are typed into it",
        "                    if editor.label.chars().count() >= MAX_LABEL_LEN {",
        "                    if false {",
        ["the_label_field_is_bounded"],
    ),
    # ---- the timer tab ----
    (
        "a preset creates a timer and leaves it stopped",
        "                let id = self.create_timer_preset(minutes);\n"
        "                self.start_timer(id);",
        "                let _ = self.create_timer_preset(minutes);",
        ["every_preset_starts_a_running_timer"],
    ),
    (
        "start with empty fields is answered as if something happened",
        "            Target::CustomStart => {\n"
        "                if self.start_custom_timer().is_none() {\n"
        "                    return Action::None;\n"
        "                }\n"
        "            }",
        "            Target::CustomStart => {\n                let _ = self.start_custom_timer();\n            }",
        ["start_with_empty_fields_does_nothing_at_all"],
    ),
    (
        "every custom duration field is the hours field",
        "            f.hit(Target::CustomField(hms), rect);",
        "            f.hit(Target::CustomField(HmsField::Hours), rect);",
        ["the_custom_row_starts_the_duration_it_spells"],
    ),
    (
        "a duration field takes letters, and as many as are typed",
        "                    if !ch.is_ascii_digit() || entry.len() >= 2 {",
        "                    if false {",
        ["the_custom_fields_take_digits_only_and_two_of_them"],
    ),
    (
        "the reset button on a timer card starts and stops it instead",
        "            Target::TimerReset(id) => match self.find_timer_mut(id) {\n"
        "                Some(timer) => timer.reset(),",
        "            Target::TimerReset(id) => match self.find_timer_mut(id) {\n"
        "                Some(timer) => timer.toggle(),",
        ["timer_card_buttons_route_to_that_timer"],
    ),
    # ---- the stopwatch ----
    (
        "the lap button on the stopwatch resets it",
        "            Target::SwLap,",
        "            Target::SwReset,",
        ["the_stopwatch_buttons_drive_the_stopwatch"],
    ),
    (
        "lap is answered as a redraw even when the stopwatch is stopped",
        "                if self.stopwatch.state != StopwatchState::Running {\n"
        "                    return Action::None;\n"
        "                }\n"
        "                self.stopwatch.lap();",
        "                self.stopwatch.lap();",
        ["the_stopwatch_buttons_drive_the_stopwatch"],
    ),
    # ---- the keyboard ----
    (
        "ctrl-q is looked at only after the focused field has had it",
        "        if m.ctrl && event.key == Key::Q {\n            return Action::Quit;\n        }\n",
        "",
        ["ctrl_q_quits_even_with_a_field_focused"],
    ),
    (
        "escape does not reach the editor",
        "            if self.editor.is_some() {\n"
        "                self.cancel_editor();\n"
        "                return Action::Redraw;\n"
        "            }\n",
        "",
        ["escape_closes_the_editor_then_stops_doing_anything"],
    ),
    (
        "a focused field does not own the keyboard",
        "        if let Some(focus) = self.focus {\n"
        "            return self.type_into(focus, event);\n"
        "        }\n",
        "",
        ["typing_a_label_is_not_a_pile_of_shortcuts"],
    ),
    (
        "a shortcut fires whatever modifiers are held with it",
        "        if m.ctrl || m.alt || m.super_key {\n            return Action::None;\n        }\n",
        "",
        ["alt_tab_is_the_window_managers_and_not_ours"],
    ),
    (
        "space drives the stopwatch from every tab",
        "            Key::Space if self.active_tab == ActiveTab::Stopwatch => {",
        "            Key::Space => {",
        ["space_drives_the_stopwatch_only_on_its_own_tab"],
    ),
    (
        "shift-tab cycles the tabs forwards like tab",
        "                let step = if m.shift { -1 } else { 1 };",
        "                let step = 1;",
        ["digits_select_tabs_and_tab_cycles_them"],
    ),
    (
        "a click on nothing leaves the keyboard where it was",
        "            return if self.focus.take().is_some() {\n"
        "                Action::Redraw\n"
        "            } else {\n"
        "                Action::None\n"
        "            };\n"
        "        };",
        "            return Action::None;\n        };",
        ["clicking_away_from_a_field_drops_the_keyboard"],
    ),
    # ---- scrolling ----
    (
        "the wheel scrolls the pane under the tab rather than under the pointer",
        "        if !rect.contains(x, y) {\n            return Action::None;\n        }\n",
        "",
        ["the_wheel_over_a_pane_that_is_not_there_does_nothing"],
    ),
    (
        "a scrolled pane runs past both of its ends",
        "        *offset = clamp_scroll(\n"
        "            before + guitk::wheel::pixels(dy, LAP_ROW_H),\n"
        "            content_h,\n"
        "            rect.h,\n"
        "        );",
        "        *offset = before + guitk::wheel::pixels(dy, LAP_ROW_H);",
        ["the_wheel_scrolls_the_alarm_list_and_stops_at_both_ends"],
    ),
    (
        "deleting the content under a scrolled pane leaves the offset alone",
        "                self.clamp_scrolls(size.0, size.1);\n"
        "            }\n"
        "            Target::AlarmSnooze(id) => self.snooze_alarm(id),",
        "            }\n            Target::AlarmSnooze(id) => self.snooze_alarm(id),",
        ["deleting_the_content_under_a_scrolled_pane_pulls_it_back"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "alarmclock", timeout=300, only=only))
