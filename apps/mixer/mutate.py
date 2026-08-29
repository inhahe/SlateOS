"""Mutation test for the sound mixer's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The mixer is the ninth application in this campaign and the one where the
question "is this tested?" had the bluntest answer: the program it replaces had
no `Event::Mouse` arm anywhere in the file, so *none* of its faders could be
moved with the pointer, and its one slider handler --

    pub fn handle_slider_click(state: &mut MixerState,
                               column_index: Option<usize>,
                               y_fraction: f32)

-- was a public function with no caller and no way for a caller to obtain
either argument, because there were no column rectangles anywhere to compute
them from.  Its two tests passed hand-written numbers straight in.  That is the
shape this script exists to detect: an argument the program cannot produce is
an argument no test result means anything about.

Usage:  python apps/mixer/mutate.py [substring ...]
"""

import re
import subprocess
import sys
from pathlib import Path

SRC = Path(__file__).parent / "src" / "main.rs"
BAK = Path(__file__).parent / "src" / "main.rs.bak"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The volume arithmetic ─────────────────────────────────────────────
    (
        "a level is rounded down to a percentage instead of to the nearest",
        "    (level.clamp(0.0, 1.0) * 100.0).round() as u8",
        "    (level.clamp(0.0, 1.0) * 100.0).floor() as u8",
        ["a_level_is_a_percentage_of_itself_and_never_out_of_range"],
    ),
    (
        # The clamp is what makes the cast exact.  This one SURVIVED the first
        # sweep, and the reason was a fault in the program rather than in the
        # suite: a second, redundant `.clamp(0.0, 100.0)` sat after the round
        # and saturated whatever the first clamp let through, so removing the
        # first clamp changed no output at all.  Two guards of one invariant
        # are one guard and one hiding place; the second is gone.
        "a level outside the range is taken at its word",
        "    (level.clamp(0.0, 1.0) * 100.0).round() as u8",
        "    (level * 100.0).round() as u8",
        ["a_level_is_a_percentage_of_itself_and_never_out_of_range"],
    ),
    (
        "silence is zero decibels rather than none at all",
        "    if linear <= 0.0 {\n        f32::NEG_INFINITY",
        "    if linear <= 0.0 {\n        0.0",
        ["decibels_and_linear_volume_are_inverses_where_both_are_defined"],
    ),
    (
        "the decibel scale is the power one rather than the amplitude one",
        "        20.0 * linear.log10()",
        "        10.0 * linear.log10()",
        ["decibels_and_linear_volume_are_inverses_where_both_are_defined"],
    ),
    (
        "the decibel floor is above the range instead of below it",
        "    if db <= -80.0 {\n        0.0",
        "    if db <= 80.0 {\n        0.0",
        ["decibels_and_linear_volume_are_inverses_where_both_are_defined"],
    ),
    (
        "a positive decibel gives a volume louder than full scale",
        "        (10.0_f32).powf(db / 20.0).clamp(0.0, 1.0)",
        "        (10.0_f32).powf(db / 20.0)",
        ["decibels_and_linear_volume_are_inverses_where_both_are_defined"],
    ),
    (
        "a stream's volume runs through the master twice",
        "    (app_volume * master_volume).clamp(0.0, 1.0)",
        "    (app_volume * master_volume * master_volume).clamp(0.0, 1.0)",
        ["a_stream_is_heard_at_its_own_fader_through_the_master"],
    ),
    (
        "a muted stream is heard at its fader anyway",
        "        if self.muted { 0.0 } else { self.volume }",
        "        self.volume",
        ["a_stream_is_heard_at_its_own_fader_through_the_master"],
    ),
    (
        "muting a stream also winds its fader down, so unmuting cannot restore it",
        "    pub fn toggle_mute(&mut self) {\n        self.muted = !self.muted;",
        "    pub fn toggle_mute(&mut self) {\n        self.muted = !self.muted;\n        self.volume = 0.0;",
        ["a_stream_is_heard_at_its_own_fader_through_the_master"],
    ),
    (
        "a volume set out of range is taken at its word",
        "    pub fn set_volume(&mut self, vol: f32) {\n        self.volume = vol.clamp(0.0, 1.0);",
        "    pub fn set_volume(&mut self, vol: f32) {\n        self.volume = vol;",
        ["a_stream_is_heard_at_its_own_fader_through_the_master"],
    ),
    (
        "a stream is created at a volume outside the range",
        "            volume: volume.clamp(0.0, 1.0),",
        "            volume,",
        ["a_stream_is_heard_at_its_own_fader_through_the_master"],
    ),
    (
        "a device says its properties in the wrong order",
        '            "{}Hz / {}bit / {}ch",\n            self.sample_rate, self.bit_depth, self.channels',
        '            "{}Hz / {}bit / {}ch",\n            self.sample_rate, self.channels, self.bit_depth',
        ["a_device_says_what_it_is"],
    ),

    # ── The order the columns are in ──────────────────────────────────────
    (
        "the columns are sorted with the silent streams first",
        "            b.playing\n                .cmp(&a.playing)",
        "            a.playing\n                .cmp(&b.playing)",
        ["the_columns_are_playing_streams_first_and_then_by_name"],
    ),
    (
        "the columns are sorted by name backwards",
        "                .then_with(|| a.app_name.cmp(&b.app_name))",
        "                .then_with(|| b.app_name.cmp(&a.app_name))",
        ["the_columns_are_playing_streams_first_and_then_by_name"],
    ),
    (
        # This is the mutation that says a column index and a storage index are
        # not the same number.  A suite that reached the streams by `streams[i]`
        # would go on agreeing with itself right through it.
        # This was expected to be caught by
        # `a_column_index_means_the_same_stream_to_the_screen_and_to_the_keyboard`,
        # and it was not -- which is the whole lesson about agreement tests in
        # one line.  That test asks whether the name drawn on column `i` is the
        # name of `stream_at(i)`; both sides go through `stream_at`, so
        # replacing it replaces both and they agree as loudly as ever.  What
        # does catch it is every test phrased against the *order* the columns
        # are drawn in, because that is the thing the mapping is for.
        "a column index is used as a storage index",
        "    pub fn stream_at(&self, index: usize) -> Option<&AudioStream> {\n        let id = *self.order().get(index)?;\n        self.streams.iter().find(|s| s.id == id)",
        "    pub fn stream_at(&self, index: usize) -> Option<&AudioStream> {\n        self.streams.get(index)",
        ["a_click_on_a_mute_button_mutes_that_column_and_no_other"],
    ),
    (
        "the keyboard reaches a different stream from the one the column draws",
        "    fn stream_at_mut(&mut self, index: usize) -> Option<&mut AudioStream> {\n        let id = *self.order().get(index)?;\n        self.streams.iter_mut().find(|s| s.id == id)",
        "    fn stream_at_mut(&mut self, index: usize) -> Option<&mut AudioStream> {\n        self.streams.get_mut(index)",
        ["a_click_on_a_mute_button_mutes_that_column_and_no_other"],
    ),
    (
        "a column past the last stream is the last stream",
        "        let id = *self.order().get(index)?;\n        self.streams.iter().find(|s| s.id == id)",
        "        let id = *self.order().last()?;\n        self.streams.iter().find(|s| s.id == id)",
        # Same reason as above: the agreement test cannot see it, and the tests
        # that name a particular column can.
        ["up_and_down_move_the_selected_column_by_one_step_and_no_other"],
    ),

    # ── Moving between columns ────────────────────────────────────────────
    (
        # The old program's `move_right` from the last stream returned itself
        # while `move_left` from Master wrapped, so the two directions disagreed
        # about what the ends of the row mean.  This puts the wall back.
        "moving forward off the end stops instead of wrapping",
        "        let next = here.saturating_add(delta).rem_euclid(slots_i.max(1));",
        "        let next = here.saturating_add(delta).clamp(0, slots_i.saturating_sub(1));",
        ["the_selection_moves_one_column_per_keystroke_and_wraps_at_both_ends"],
    ),
    (
        "the master column is not one of the slots to move between",
        "        let slots = stream_count.saturating_add(1);",
        "        let slots = stream_count;",
        ["the_selection_moves_one_column_per_keystroke_and_wraps_at_both_ends"],
    ),
    (
        "the selection moves two columns per keystroke",
        "        let next = here.saturating_add(delta).rem_euclid(slots_i.max(1));",
        "        let next = here.saturating_add(delta * 2).rem_euclid(slots_i.max(1));",
        ["the_selection_moves_one_column_per_keystroke_and_wraps_at_both_ends"],
    ),
    (
        "index 0 is a stream rather than the master",
        "    pub fn at(index: usize) -> Self {",
        "    pub fn at(index: usize) -> Self {\n        if index == 0 {\n            return Self::Stream(0);\n        }",
        ["the_selection_moves_one_column_per_keystroke_and_wraps_at_both_ends"],
    ),
    (
        # Expected to be caught by `left_and_right_undo_each_other_from_every
        # _column`, and it was not: two keys that have been swapped undo each
        # other exactly as well as two keys that have not.  A symmetry test is
        # blind to a symmetric fault, in the same way an agreement test is
        # blind to a fault in the mapping both its sides use.  What catches it
        # is the test that says which *way* a keystroke moves.
        "left and right are swapped",
        "            Key::Left => Some(Action::MoveSelection(-1)),\n            Key::Right | Key::Tab => Some(Action::MoveSelection(1)),",
        "            Key::Left => Some(Action::MoveSelection(1)),\n            Key::Right | Key::Tab => Some(Action::MoveSelection(-1)),",
        ["the_selection_moves_one_column_per_keystroke_and_wraps_at_both_ends"],
    ),
    (
        # The old file made Tab a byte-for-byte duplicate of Right and then had
        # no handler that looked at a modifier, so Shift-Tab went forwards.
        "shift-tab goes forwards like tab",
        "        if ev.key == Key::Tab && ev.modifiers.shift && self.picker == Picker::None {",
        "        if false {",
        ["tab_moves_forward_and_shift_tab_moves_back"],
    ),
    (
        "tab does nothing at all",
        "            Key::Right | Key::Tab => Some(Action::MoveSelection(1)),",
        "            Key::Right => Some(Action::MoveSelection(1)),",
        ["tab_moves_forward_and_shift_tab_moves_back"],
    ),
    (
        # SURVIVED the first sweep: nothing ever asked to select a column that
        # is not there, because the keyboard's own movement wraps within the
        # columns that are.  But `Action::Select` is public and a click raises
        # it, so an out-of-range index is a thing a caller can really produce.
        "a column that does not exist can be selected",
        "            Selection::Stream(i) => i < self.streams.len(),",
        "            Selection::Stream(_) => true,",
        ["a_column_that_does_not_exist_cannot_be_selected"],
    ),

    # ── The volume keys ───────────────────────────────────────────────────
    (
        "up and down are swapped",
        "            Key::Up => Some(Action::NudgeVolume(VOLUME_STEP)),\n            Key::Down => Some(Action::NudgeVolume(-VOLUME_STEP)),",
        "            Key::Up => Some(Action::NudgeVolume(-VOLUME_STEP)),\n            Key::Down => Some(Action::NudgeVolume(VOLUME_STEP)),",
        ["up_and_down_move_the_selected_column_by_one_step_and_no_other"],
    ),
    (
        "a nudge moves the volume by twice the step it names",
        "                if let Some(here) = self.volume_of(self.selection) {\n                    self.apply(Action::SetVolume(self.selection, here + delta));",
        "                if let Some(here) = self.volume_of(self.selection) {\n                    self.apply(Action::SetVolume(self.selection, here + delta * 2.0));",
        ["up_and_down_move_the_selected_column_by_one_step_and_no_other"],
    ),
    (
        "a nudge moves a column other than the selected one",
        "                if let Some(here) = self.volume_of(self.selection) {\n                    self.apply(Action::SetVolume(self.selection, here + delta));",
        "                if let Some(here) = self.volume_of(Selection::Master) {\n                    self.apply(Action::SetVolume(Selection::Master, here + delta));",
        ["up_and_down_move_the_selected_column_by_one_step_and_no_other"],
    ),
    (
        "the volume runs off the top of its range",
        "            Action::SetVolume(sel, v) => {\n                let v = v.clamp(0.0, 1.0);",
        "            Action::SetVolume(sel, v) => {\n                let v = v.max(0.0);",
        ["the_volume_stops_at_both_ends_of_its_range"],
    ),
    (
        "the volume runs off the bottom of its range",
        "            Action::SetVolume(sel, v) => {\n                let v = v.clamp(0.0, 1.0);",
        "            Action::SetVolume(sel, v) => {\n                let v = v.min(1.0);",
        ["the_volume_stops_at_both_ends_of_its_range"],
    ),

    # ── Mute ──────────────────────────────────────────────────────────────
    (
        "M mutes the master whatever column is selected",
        "            Key::M => Some(Action::ToggleMute(self.selection)),",
        "            Key::M => Some(Action::ToggleMute(Selection::Master)),",
        ["m_mutes_the_column_the_keyboard_is_pointed_at_and_no_other"],
    ),
    (
        "mute is a one-way switch",
        "            Action::ToggleMute(sel) => match sel {\n                Selection::Master => self.master_muted = !self.master_muted,",
        "            Action::ToggleMute(sel) => match sel {\n                Selection::Master => self.master_muted = true,",
        ["a_muted_master_silences_everything_without_moving_a_fader"],
    ),
    (
        "muting a stream is a one-way switch",
        "                Selection::Stream(i) => {\n                    if let Some(s) = self.stream_at_mut(i) {\n                        s.muted = !s.muted;",
        "                Selection::Stream(i) => {\n                    if let Some(s) = self.stream_at_mut(i) {\n                        s.muted = true;",
        ["muting_a_column_silences_it_without_forgetting_where_its_fader_was"],
    ),
    (
        "muting the master winds its fader down as well",
        "                Selection::Master => self.master_muted = !self.master_muted,",
        "                Selection::Master => {\n                    self.master_muted = !self.master_muted;\n                    self.master_volume = 0.0;\n                }",
        ["a_muted_master_silences_everything_without_moving_a_fader"],
    ),

    # ── The key handler ───────────────────────────────────────────────────
    (
        # `apps/life` shipped this fault for real: destructuring `KeyEvent`
        # without `pressed` runs every key twice, once down and once up.
        "the key handler swallows `pressed` and runs everything twice",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        ["a_key_release_on_its_own_changes_nothing"],
    ),
    (
        "the key handler answers releases and ignores presses",
        "        if !ev.pressed {\n            return EventResult::Ignored;\n        }",
        "        if ev.pressed {\n            return EventResult::Ignored;\n        }",
        ["a_key_release_on_its_own_changes_nothing"],
    ),
    (
        # The old file filtered no modifier at all, so Ctrl-M muted.
        "a held Ctrl no longer refuses the key",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {",
        "        if ev.modifiers.alt || ev.modifiers.super_key {",
        ["a_modifier_held_down_refuses_the_key"],
    ),
    (
        "a held Alt no longer refuses the key",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {",
        "        if ev.modifiers.ctrl || ev.modifiers.super_key {",
        ["a_modifier_held_down_refuses_the_key"],
    ),
    (
        "a held Shift refuses the key, so Shift-Tab cannot be typed",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {",
        "        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key || ev.modifiers.shift {",
        ["shift_is_not_a_modifier_that_refuses_a_key"],
    ),
    (
        # The old file consumed Escape to do nothing, over the comment "Could
        # close the app in a real implementation".
        "a key the program does not answer is consumed anyway",
        "            None => EventResult::Ignored,\n        }",
        "            None => EventResult::Consumed,\n        }",
        ["a_key_the_program_does_not_answer_is_left_alone"],
    ),
    (
        # SURVIVED the first sweep.  The two sheets look alike, so "a picker
        # opened" is true of both and separates neither; what separates them is
        # the list of names written on the rows.
        "O and I open each other's picker",
        "            Key::O => Some(Action::OpenOutput),\n            Key::I => Some(Action::OpenInput),",
        "            Key::O => Some(Action::OpenInput),\n            Key::I => Some(Action::OpenOutput),",
        ["o_opens_the_output_picker_and_i_opens_the_input_one"],
    ),

    # ── The shortcut bar ──────────────────────────────────────────────────
    (
        "the bar names a key the program does not answer",
        '    ("M", "mute"),',
        '    ("M", "mute"),\n    ("Q", "quit"),',
        ["the_shortcut_bar_names_every_key_the_program_answers_and_no_others"],
    ),
    (
        "the bar leaves out a key the program does answer",
        '    ("M", "mute"),\n',
        "",
        ["the_shortcut_bar_names_every_key_the_program_answers_and_no_others"],
    ),
    (
        "the bar keeps showing the mixer's keys while a sheet is up",
        "        let rows: &[(&str, &str)] = if self.picker == Picker::None {",
        "        let rows: &[(&str, &str)] = if true {",
        ["the_shortcut_bar_says_what_view_it_is_in"],
    ),

    # ── The pointer ───────────────────────────────────────────────────────
    (
        "a right press does what a left one does",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if !matches!(ev.kind, MouseEventKind::Press(_)) {",
        ["only_a_left_press_does_anything"],
    ),
    (
        "a release does what a press does",
        "        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {",
        "        if matches!(ev.kind, MouseEventKind::Move) {",
        ["only_a_left_press_does_anything"],
    ),
    (
        "a click on nothing is consumed anyway",
        "        let Some((target, rect)) = hit_with_rect(&f, ev.x, ev.y) else {\n            return EventResult::Ignored;\n        };",
        "        let Some((target, rect)) = hit_with_rect(&f, ev.x, ev.y) else {\n            return EventResult::Consumed;\n        };",
        ["a_click_on_nothing_is_left_alone"],
    ),
    (
        # `hit_test` takes the *last* box at a point.  Taking the first makes the
        # whole-column box win over the fader and the mute button drawn after
        # it, and makes the picker's backdrop win over its own rows.
        "the first box recorded at a point wins instead of the last",
        "    f.hits()\n        .iter()\n        .rev()\n        .find(|(_, r)| r.contains(x, y))",
        "    f.hits()\n        .iter()\n        .find(|(_, r)| r.contains(x, y))",
        ["the_pixels_a_control_is_drawn_on_are_the_pixels_that_reach_it"],
    ),
    (
        "a fader is loud at the bottom and quiet at the top",
        "    (1.0 - (y - r.y) / r.h).clamp(0.0, 1.0)",
        "    ((y - r.y) / r.h).clamp(0.0, 1.0)",
        ["a_click_on_a_fader_sets_the_volume_to_the_height_it_landed_at"],
    ),
    (
        "a fader reads the click against the window rather than its own track",
        "    (1.0 - (y - r.y) / r.h).clamp(0.0, 1.0)",
        "    (1.0 - y / r.h).clamp(0.0, 1.0)",
        ["a_click_on_a_fader_sets_the_volume_to_the_height_it_landed_at"],
    ),
    (
        # SURVIVED the first sweep, because through the pointer the clamp is
        # unreachable: a click is measured in the very box it was tested
        # against, so the fraction is already in range before the clamp sees
        # it.  But `value_at` is public, and a caller handing it a `y` off the
        # track is a thing that can happen -- so unlike the two guards deleted
        # elsewhere in this file, this one is worth keeping, and therefore
        # worth a test that reaches it.
        "a fader click sets a volume outside the range",
        "    (1.0 - (y - r.y) / r.h).clamp(0.0, 1.0)",
        "    1.0 - (y - r.y) / r.h",
        ["a_click_past_either_end_of_a_track_is_full_volume_or_none"],
    ),
    (
        "clicking a fader moves it but does not point the keyboard at it",
        "            Target::StreamFader(i) => {\n                self.apply(Action::Select(Selection::Stream(i)));",
        "            Target::StreamFader(i) => {",
        ["a_click_on_a_fader_sets_the_volume_to_the_height_it_landed_at"],
    ),
    (
        "clicking a mute button mutes the next column along",
        "            Target::StreamMute(i) => {\n                self.apply(Action::Select(Selection::Stream(i)));\n                self.apply(Action::ToggleMute(Selection::Stream(i)));",
        "            Target::StreamMute(i) => {\n                self.apply(Action::Select(Selection::Stream(i)));\n                self.apply(Action::ToggleMute(Selection::Stream(i.saturating_add(1))));",
        ["a_click_on_a_mute_button_mutes_that_column_and_no_other"],
    ),
    (
        "clicking a mute button moves the fader instead",
        "            Target::StreamMute(i) => {\n                self.apply(Action::Select(Selection::Stream(i)));\n                self.apply(Action::ToggleMute(Selection::Stream(i)));",
        "            Target::StreamMute(i) => {\n                self.apply(Action::Select(Selection::Stream(i)));\n                self.apply(Action::SetVolume(Selection::Stream(i), 0.0));",
        ["a_click_on_a_mute_button_mutes_that_column_and_no_other"],
    ),
    (
        "clicking the master mute mutes a stream",
        "            Target::MasterMute => {\n                self.apply(Action::Select(Selection::Master));\n                self.apply(Action::ToggleMute(Selection::Master));",
        "            Target::MasterMute => {\n                self.apply(Action::Select(Selection::Master));\n                self.apply(Action::ToggleMute(Selection::Stream(0)));",
        ["a_click_on_a_mute_button_mutes_that_column_and_no_other"],
    ),
    (
        "clicking a column selects it and moves its fader as well",
        "            Target::StreamColumn(i) => self.apply(Action::Select(Selection::Stream(i))),",
        "            Target::StreamColumn(i) => {\n                self.apply(Action::Select(Selection::Stream(i)));\n                self.apply(Action::SetVolume(Selection::Stream(i), 0.0));\n            }",
        ["a_click_on_a_column_selects_it_and_changes_nothing_else"],
    ),
    (
        "the device bars open each other's picker",
        "            Target::OutputDevice => self.apply(Action::OpenOutput),\n            Target::InputDevice => self.apply(Action::OpenInput),",
        "            Target::OutputDevice => self.apply(Action::OpenInput),\n            Target::InputDevice => self.apply(Action::OpenOutput),",
        ["the_device_bars_open_the_picker_they_name"],
    ),
    (
        "no hit box is recorded for a fader",
        "        f.hit(\n            match sel {\n                Selection::Master => Target::MasterFader,\n                Selection::Stream(i) => Target::StreamFader(i),\n            },\n            track,\n        );",
        "",
        ["every_column_can_be_muted_and_faded_with_the_pointer"],
    ),
    (
        "no hit box is recorded for a mute button",
        "        f.hit(\n            match sel {\n                Selection::Master => Target::MasterMute,\n                Selection::Stream(i) => Target::StreamMute(i),\n            },\n            mute,\n        );",
        "",
        ["every_column_can_be_muted_and_faded_with_the_pointer"],
    ),
    (
        "no hit box is recorded for a device bar",
        "            fill(f, r, if open { SURFACE1 } else { SURFACE0 }, 5.0);\n            f.hit(target, r);",
        "            fill(f, r, if open { SURFACE1 } else { SURFACE0 }, 5.0);",
        ["the_device_bars_open_the_picker_they_name"],
    ),
    (
        # The whole column is recorded *before* the fader and the mute button so
        # that they win where they are.  Recording it after makes the column
        # swallow both of them and nothing inside a column can be clicked.
        "the whole column is recorded over the controls inside it",
        "        f.hit(\n            match sel {\n                Selection::Master => Target::MasterColumn,\n                Selection::Stream(i) => Target::StreamColumn(i),\n            },\n            col,\n        );\n\n        centred_in(",
        "        centred_in(",
        ["every_column_can_be_muted_and_faded_with_the_pointer"],
    ),
    (
        # SURVIVED the first sweep, and it is the sharpest of the thirteen.
        # The grid walk compares the frame's hit boxes with each other, so a
        # box of the wrong *shape* is invisible to it -- shrink every fader to
        # its top half and the walk still agrees with itself, because both
        # sides of the comparison shrank together.  Only a test that ties the
        # box back to the geometry the fader was *drawn* from can see it.
        "a fader's hit box is not the track it is drawn on",
        "        f.hit(\n            match sel {\n                Selection::Master => Target::MasterFader,\n                Selection::Stream(i) => Target::StreamFader(i),\n            },\n            track,\n        );",
        "        f.hit(\n            match sel {\n                Selection::Master => Target::MasterFader,\n                Selection::Stream(i) => Target::StreamFader(i),\n            },\n            Rect::new(track.x, track.y, track.w, track.h * 0.5),\n        );",
        ["a_faders_hit_box_is_the_track_it_is_drawn_on"],
    ),

    # ── The clock ─────────────────────────────────────────────────────────
    (
        # Lesson 47 itself: the old `main` called `update_peak_meters()` ten
        # times in a loop and exited, because no window was ever going to call
        # it again.  An app that asks for no clock is that same app.
        "the window is never told to run a clock",
        "        self.meters_moving()\n            .then(|| Duration::from_millis(METER_STEP_MS))",
        "        None",
        ["the_window_is_asked_for_a_clock_exactly_while_the_meters_are_moving"],
    ),
    (
        "the clock keeps running after everything has fallen silent",
        "        self.meters_moving()\n            .then(|| Duration::from_millis(METER_STEP_MS))",
        "        Some(Duration::from_millis(METER_STEP_MS))",
        ["the_window_is_asked_for_a_clock_exactly_while_the_meters_are_moving"],
    ),
    (
        "a decayed meter counts as still moving, so the clock never stops",
        "            .any(|s| (s.playing && !s.muted) || s.peak_level > 0.0)",
        "            .any(|s| (s.playing && !s.muted) || s.peak_level >= 0.0)",
        ["the_window_is_asked_for_a_clock_exactly_while_the_meters_are_moving"],
    ),
    (
        "a muted stream still counts as moving",
        "            .any(|s| (s.playing && !s.muted) || s.peak_level > 0.0)",
        "            .any(|s| s.playing || s.peak_level > 0.0)",
        ["the_window_is_asked_for_a_clock_exactly_while_the_meters_are_moving"],
    ),
    (
        "the interval asked for is not the interval a step takes",
        "        self.meters_moving()\n            .then(|| Duration::from_millis(METER_STEP_MS))",
        "        self.meters_moving()\n            .then(|| Duration::from_millis(METER_STEP_MS * 4))",
        ["the_window_is_asked_for_a_clock_exactly_while_the_meters_are_moving"],
    ),
    (
        # The old `update_peak_meters` took no elapsed time at all: one step per
        # call, so the ballistics were whatever the frame rate happened to be.
        "a tick advances by one step however long really passed",
        "        while self.meter_accum >= METER_STEP_MS {",
        "        if self.meter_accum >= METER_STEP_MS {\n            self.meter_accum = 0;\n            self.step_meters();\n            return EventResult::Consumed;\n        }\n        while false {",
        ["a_tick_advances_by_the_time_that_passed_not_by_the_interval"],
    ),
    (
        "time shorter than a step is thrown away rather than banked",
        "        self.meter_accum = self.meter_accum.saturating_add(elapsed_ms);",
        "        self.meter_accum = elapsed_ms;",
        ["a_tick_shorter_than_a_step_banks_the_time_rather_than_losing_it"],
    ),
    (
        "a tick that ran nothing claims the frame changed",
        "        if taken == 0 {\n            EventResult::Ignored\n        } else {\n            EventResult::Consumed\n        }",
        "        EventResult::Consumed",
        ["a_tick_shorter_than_a_step_banks_the_time_rather_than_losing_it"],
    ),
    (
        "the catch-up loop is unbounded",
        "            if taken >= MAX_CATCHUP {",
        "            if false {",
        ["catching_up_is_capped_and_the_backlog_is_dropped_not_banked"],
    ),
    (
        "the dropped backlog is banked and paid out on the next tick",
        "                self.meter_accum = self.meter_accum.checked_rem(METER_STEP_MS).unwrap_or(0);\n                break;",
        "                break;",
        ["catching_up_is_capped_and_the_backlog_is_dropped_not_banked"],
    ),
    (
        "a step is not taken out of the bank, so one tick runs forever",
        "            self.meter_accum = self.meter_accum.saturating_sub(METER_STEP_MS);\n            self.step_meters();",
        "            self.step_meters();",
        ["a_tick_advances_by_the_time_that_passed_not_by_the_interval"],
    ),

    # ── The meters ────────────────────────────────────────────────────────
    (
        # SURVIVED the first sweep: every test knew a meter rises while a
        # stream plays and falls when it does not, and none knew a rise is
        # *steeper* than a fall.  `a_meter_rises_faster_than_it_falls` says so
        # as a bound rather than a golden number -- one step closes at most
        # `rate` of the gap that is left, so a run that ever closes more than
        # `DECAY_RATE` of a gap proves the two rates differ.
        "a meter attacks as slowly as it decays",
        "                let rate = if target > stream.peak_level {\n                    ATTACK_RATE\n                } else {\n                    DECAY_RATE\n                };",
        "                let rate = DECAY_RATE;",
        ["a_meter_rises_faster_than_it_falls"],
    ),
    (
        "a silent meter falls towards a floor it never reaches",
        "                stream.peak_level *= SILENCE_RATE;\n                if stream.peak_level < SILENCE_FLOOR {\n                    stream.peak_level = 0.0;\n                }",
        "                stream.peak_level *= SILENCE_RATE;",
        ["a_meter_rises_towards_a_playing_stream_and_falls_to_silence_on_a_muted_one"],
    ),
    (
        "a muted stream's meter keeps reading its level",
        "            if stream.playing && !stream.muted {",
        "            if stream.playing {",
        ["a_meter_rises_towards_a_playing_stream_and_falls_to_silence_on_a_muted_one"],
    ),
    (
        # The levels used to come from `(id * 7 + tick * 13) % 100`, which is a
        # sawtooth: every meter ran the same ramp offset by its id, so they
        # climbed in lockstep and reset together.  A constant draw is the same
        # fault with the ramp taken out.
        "every meter reads the same constant",
        "            .map(|_| self.rng.unit_f32())",
        "            .map(|_| 0.5_f32)",
        ["two_mixers_draw_two_different_meter_runs_and_one_seed_repeats_itself"],
    ),
    (
        "every seed draws the same meter run",
        "            rng: SeededRng::new(seed),",
        "            rng: SeededRng::new(FALLBACK_SEED),",
        ["two_mixers_draw_two_different_meter_runs_and_one_seed_repeats_itself"],
    ),
    (
        "the meter step count is never advanced",
        "        self.steps = self.steps.saturating_add(1);",
        "",
        ["a_tick_advances_by_the_time_that_passed_not_by_the_interval"],
    ),

    # ── The picker ────────────────────────────────────────────────────────
    (
        "the picker opens on the first row rather than the chosen one",
        "            Action::OpenOutput => {\n                self.picker = Picker::Output;\n                self.picker_row = self.selected_output;",
        "            Action::OpenOutput => {\n                self.picker = Picker::Output;\n                self.picker_row = 0;",
        ["the_picker_opens_on_the_device_that_is_already_chosen"],
    ),
    (
        "the picker's selection runs off the end of the list",
        "                    self.picker_row = here.saturating_add(delta).clamp(0, last) as usize;",
        "                    self.picker_row = here.saturating_add(delta).max(0) as usize;",
        ["the_picker_selection_moves_one_row_and_stops_at_the_ends"],
    ),
    (
        "the picker's up and down are swapped",
        "                Key::Up => Some(Action::MovePickerRow(-1)),\n                Key::Down => Some(Action::MovePickerRow(1)),",
        "                Key::Up => Some(Action::MovePickerRow(1)),\n                Key::Down => Some(Action::MovePickerRow(-1)),",
        ["the_picker_selection_moves_one_row_and_stops_at_the_ends"],
    ),
    (
        "Enter closes the sheet without using the row it is on",
        "                match self.picker {\n                    Picker::None => {}\n                    Picker::Output => self.selected_output = self.picker_row,\n                    Picker::Input => self.selected_input = self.picker_row,\n                }",
        "                match self.picker {\n                    Picker::None | Picker::Output | Picker::Input => {}\n                }",
        ["enter_uses_the_row_the_picker_is_on_and_escape_leaves_it_alone"],
    ),
    (
        "Escape chooses the row it is leaving",
        "                Key::Escape => Some(Action::ClosePicker),",
        "                Key::Escape => Some(Action::ChooseDevice),",
        ["enter_uses_the_row_the_picker_is_on_and_escape_leaves_it_alone"],
    ),
    (
        "choosing a device leaves the sheet up",
        "                self.apply(Action::ClosePicker);\n            }\n        }\n    }",
        "            }\n        }\n    }",
        ["enter_uses_the_row_the_picker_is_on_and_escape_leaves_it_alone"],
    ),
    # There was a mutation here called "a row past the end of the list can be
    # chosen", which disabled a `picker_row >= picker_len()` guard at the top
    # of `Action::ChooseDevice`.  It SURVIVED, and rightly: the guard was
    # unreachable.  Every way of moving the row already bounds it, so no input
    # could put the model in the state the guard existed to catch, and no test
    # could tell the guard from its absence.  The guard is gone rather than the
    # mutation being excused -- an unreachable third copy of a bound its two
    # holders already keep is a hiding place, not defence in depth.  Same
    # lesson as the second clamp in `percent_of`.
    (
        "a click on a picker row selects it without using it",
        "            Target::OutputRow(i) | Target::InputRow(i) => {\n                self.apply(Action::SelectPickerRow(i));\n                self.apply(Action::ChooseDevice);",
        "            Target::OutputRow(i) | Target::InputRow(i) => {\n                self.apply(Action::SelectPickerRow(i));",
        ["every_row_the_picker_lists_can_be_chosen_with_the_pointer"],
    ),
    (
        "a click on a picker row uses whichever row was already selected",
        "            Target::OutputRow(i) | Target::InputRow(i) => {\n                self.apply(Action::SelectPickerRow(i));\n                self.apply(Action::ChooseDevice);",
        "            Target::OutputRow(_) | Target::InputRow(_) => {\n                self.apply(Action::ChooseDevice);",
        ["every_row_the_picker_lists_can_be_chosen_with_the_pointer"],
    ),
    (
        "the sheet offers one fewer device than there are",
        "        for (i, d) in devices.iter().enumerate() {",
        "        for (i, d) in devices.iter().enumerate().take(devices.len().saturating_sub(1)) {",
        ["every_row_the_picker_lists_can_be_chosen_with_the_pointer"],
    ),
    (
        # SURVIVED the first sweep.  Every picker test asked whether the sheet
        # had rows and whether each row could be used -- questions the outputs
        # answer exactly as well as the inputs do.  What separates the two
        # lists is what is written on them, and no test read that.
        "the picker's rows are drawn for the other kind of device",
        "                Picker::Input => (\"Input device\", &self.input_devices, Target::InputRow),",
        "                Picker::Input => (\"Input device\", &self.output_devices, Target::InputRow),",
        ["the_sheet_lists_the_devices_of_the_kind_it_is_the_picker_for"],
    ),
    (
        # The backdrop is recorded over the whole window *before* the rows, so
        # it loses to them where they are and wins everywhere else.  Recording
        # it after makes it swallow the sheet it is behind.
        #
        # The first sweep's version of this moved the backdrop's box only as
        # far as the line after the sheet's fill -- which is still before the
        # rows are recorded, so the rows still won and nothing changed.  A
        # mutation has to reach past everything it is meant to swallow, so
        # this one records the backdrop again after the last row.
        "the modal backdrop is recorded over the sheet it is behind",
        "            f.hit(row_target(i), r);\n        }\n    }\n}",
        "            f.hit(row_target(i), r);\n        }\n        f.hit(Target::ClosePicker, l.window);\n    }\n}",
        ["every_row_the_picker_lists_can_be_chosen_with_the_pointer"],
    ),
    (
        "the sheet does not block the controls underneath it",
        "        f.hit(Target::ClosePicker, l.window);\n\n        fill(f, l.sheet, MANTLE, 8.0);",
        "        fill(f, l.sheet, MANTLE, 8.0);",
        ["while_the_sheet_is_up_no_click_reaches_the_controls_beneath_it"],
    ),
    (
        # SURVIVED the first sweep, because the test that clicked away from the
        # sheet never moved the sheet's cursor first -- so the row it was on
        # *was* the device already chosen, and cancelling and choosing came to
        # exactly the same thing.  A cancel is only distinguishable from a
        # choice once the two differ.
        "a click off the sheet chooses a device instead of cancelling",
        "            Target::ClosePicker => self.apply(Action::ClosePicker),",
        "            Target::ClosePicker => self.apply(Action::ChooseDevice),",
        ["a_click_off_the_sheet_cancels_it_without_choosing_the_row_it_was_on"],
    ),
    (
        # The keyboard's block on the mixer is a separate piece of code from the
        # pointer's, and a suite that only clicks cannot see it.
        "the sheet lets the mixer's keys through underneath it",
        "        if self.picker != Picker::None {\n            return match key {",
        "        if false {\n            return match key {",
        ["while_the_sheet_is_up_the_mixer_keys_do_nothing"],
    ),
    (
        "Up and Down move a fader as well as the picker's selection",
        "                Key::Up => Some(Action::MovePickerRow(-1)),",
        "                Key::Up => Some(Action::NudgeVolume(VOLUME_STEP)),",
        ["up_and_down_move_the_picker_and_not_a_fader_while_the_sheet_is_up"],
    ),
    (
        # SURVIVED the first sweep: opening a sheet always sets the row from
        # the chosen device, so a stale row is overwritten before anything
        # reads it -- as long as nothing asks while the sheet is down.  But
        # `picker_row()` is public and *is* asked, so the claim is a real one
        # and worth stating rather than a guard that cannot be observed.
        "closing the sheet leaves the row it was on behind",
        "            Action::ClosePicker => {\n                self.picker = Picker::None;\n                self.picker_row = 0;",
        "            Action::ClosePicker => {\n                self.picker = Picker::None;",
        ["closing_the_sheet_forgets_the_row_it_was_on"],
    ),

    # ── The layout ────────────────────────────────────────────────────────
    (
        # The lower clamp is a floor, and a floor can be wider than the room the
        # gaps have to share: at 1x1 five two-pixel gaps wanted ten pixels of a
        # half-pixel band.  This is the fault the 1x1 invariant actually found.
        "the gaps may be wider than the band they are inside",
        "        let gap = (band.w * 0.012)\n            .clamp(2.0, 14.0)\n            .min(band.w / (2.0 * cols.max(1) as f32));",
        "        let gap = (band.w * 0.012).clamp(2.0, 14.0);",
        ["the_columns_are_side_by_side_and_do_not_overlap"],
    ),
    (
        "the columns overlap one another",
        "    let col_w = ((band.w - gaps) / slots as f32).max(0.0);",
        "    let col_w = ((band.w - gaps) / slots as f32).max(0.0) * 1.4;",
        ["the_columns_are_side_by_side_and_do_not_overlap"],
    ),
    (
        # The first sweep's version of this inserted `if false { return None; }`,
        # which is not a mutation at all -- it changed nothing, so of course
        # nothing failed.  A bound that is one too generous is the fault this
        # is meant to be.
        "there is a column for a stream that does not exist",
        "    pub fn column(&self, i: usize) -> Option<Rect> {\n        if i >= self.cols {",
        "    pub fn column(&self, i: usize) -> Option<Rect> {\n        if i >= self.cols.saturating_add(1) {",
        ["there_is_no_column_for_a_stream_that_does_not_exist"],
    ),
    (
        "the layout is taken from the window it was built at, not the one asked for",
        "        Layout::new(self.size.0, self.size.1, self.streams.len())",
        "        Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT, self.streams.len())",
        ["the_layout_is_read_from_the_window_and_not_remembered"],
    ),
    (
        "a window of no size is taken at its word",
        "        self.size = (width.max(1.0), height.max(1.0));",
        "        self.size = (width, height);",
        ["a_resize_is_the_only_thing_that_changes_the_size_the_layout_is_read_at"],
    ),
    (
        "a resize is ignored, so the layout is stuck at the size it opened",
        "        Event::Resize { width, height } => {\n            app.resize(*width as f32, *height as f32);",
        "        Event::Resize { .. } => {",
        ["a_resize_is_the_only_thing_that_changes_the_size_the_layout_is_read_at"],
    ),
    (
        "render draws at a size of its own rather than the window's",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {\n        let (width, height) = (WINDOW_WIDTH, WINDOW_HEIGHT);\n        let _ = (width, height);",
        ["the_window_gets_a_tree_at_the_size_it_asked_for_and_keeps_it"],
    ),
    (
        "a close request is swallowed, so the window can never be closed",
        "        _ => EventResult::Ignored,\n    }\n}",
        "        _ => EventResult::Consumed,\n    }\n}",
        ["an_event_the_program_does_not_answer_is_left_alone"],
    ),
    (
        "the window opens at a size the layout was never designed for",
        "    fn initial_size(&self) -> (u32, u32) {\n        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)\n    }",
        "    fn initial_size(&self) -> (u32, u32) {\n        (320, 240)\n    }",
        ["the_window_is_told_who_it_is"],
    ),
    (
        "the app tells the desktop it is some other program",
        '        "mixer".to_string()',
        '        "audio".to_string()',
        ["the_window_is_told_who_it_is"],
    ),
    (
        "a centred string is given the whole box to run into from where it starts",
        "        Some((r.right() - x).max(0.0)),",
        "        Some(r.w),",
        ["a_centred_string_never_overflows_the_box_it_is_centred_in"],
    ),
    (
        "a string too long to centre starts left of its box",
        "    let x = (r.x + (r.w - w) / 2.0).max(r.x);",
        "    let x = r.x + (r.w - w) / 2.0;",
        ["a_centred_string_never_overflows_the_box_it_is_centred_in"],
    ),
    (
        # SURVIVED the first sweep, and the reason is worth keeping: *both*
        # orders leave every band inside the window, so a test that asks
        # whether everything fits cannot see the order at all.  What the order
        # decides is which band is the one to go at a height where only one can
        # stay -- and that is a question only a test about the choice can ask.
        "a band is dropped in the wrong order, so the columns go before the bar",
        "const BAND_DROP_ORDER: [usize; 2] = [1, 0];",
        "const BAND_DROP_ORDER: [usize; 2] = [0, 1];",
        ["the_shortcut_bar_is_the_band_given_up_first_when_the_window_is_short"],
    ),
    (
        "the fader and the meter overlap one another",
        "    pub fn fader_of(&self, col: Rect) -> Rect {",
        "    pub fn fader_of(&self, col: Rect) -> Rect {\n        #[allow(clippy::needless_return)]\n        if true {\n            return self.middle_of(col);\n        }",
        ["the_fader_and_the_meter_are_side_by_side_and_do_not_overlap"],
    ),
    (
        "the parts of a column are stacked on top of one another",
        "    pub fn mute_of(&self, col: Rect) -> Rect {",
        "    pub fn mute_of(&self, col: Rect) -> Rect {\n        #[allow(clippy::needless_return)]\n        if true {\n            return self.name_of(col);\n        }",
        ["the_parts_of_a_column_stack_up_without_overlapping"],
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
            "mixer",
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
    # An unbounded catch-up loop does not fail a test; it runs meter sweeps
    # until the runner kills it.  A harness that only counted named failures
    # would score that as a mutant nobody noticed, which is the opposite of the
    # truth: a hang IS the symptom.  Same for a mutant that aborts the process
    # before any test can report.
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
