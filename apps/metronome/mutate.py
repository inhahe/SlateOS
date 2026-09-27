"""Mutation test for the metronome: its silence, its practice settings, its
key card and its pointer layer.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It drew a tempo, a beat per circle and a panel of settings, and nothing
answered the pointer; it never said it makes no sound; and practice mode
always started at 80 BPM, a value nothing could change (known-issues,
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
    (
        "the window does not say it is silent",
        "            text: String::from(SILENT_LINE),",
        "            text: String::new(),",
        ["the_window_says_the_beat_is_not_heard"],
    ),
    (
        "the start tempo cannot be set",
        "                self.practice_start_bpm = step(self.practice_start_bpm, 5, MIN_BPM, MAX_BPM);",
        "",
        ["practice_starts_at_the_start_tempo_that_was_set"],
    ),
    (
        "a start above the target leaves the target below it",
        "                self.practice_target_bpm = self.practice_target_bpm.max(self.practice_start_bpm);",
        "",
        ["the_start_and_the_target_stay_in_order"],
    ),
    (
        "a target below the start leaves the start above it",
        "                self.practice_start_bpm = self.practice_start_bpm.min(self.practice_target_bpm);",
        "",
        ["the_start_and_the_target_stay_in_order"],
    ),
    (
        "Down does not walk the settings",
        "                    at.saturating_add(1)\n"
        "                        .min(SETTING_ROWS.len().saturating_sub(1))",
        "                    at",
        ["right_raises_the_practice_increment", "practice_starts_at_the_start_tempo_that_was_set"],
    ),
    (
        "the first row does not turn practice on",
        "            SettingRow::Practice => self.practice_mode = !self.practice_mode,",
        "            SettingRow::Practice => {}",
        ["the_settings_can_be_set_before_practice_is_on"],
    ),
    (
        "a digit past the measure claims to act",
        "                if beat >= self.accents.len() {\n"
        "                    return EventResult::Ignored;\n"
        "                }",
        "",
        ["a_press_accents_the_twelfth_beat"],
    ),
    (
        "a press on a beat does not accent it",
        "            Target::Beat(i) => self.toggle_accent(i),",
        "            Target::Beat(_) => {}",
        ["a_press_accents_the_twelfth_beat", "every_control_answers_the_pointer"],
    ),
    (
        "the beats cannot be pressed",
        "            f.hit(\n"
        "                Target::Beat(i as usize),\n"
        "                Rect::new(cx, beat_y, circle_size, circle_size),\n"
        "            );",
        "",
        ["a_press_accents_the_twelfth_beat"],
    ),
    (
        "the wheel turns the tempo the wrong way",
        "                if notches < 0 {\n"
        "                    self.increase_bpm(by);",
        "                if notches > 0 {\n"
        "                    self.increase_bpm(by);",
        ["the_wheel_turns_the_tempo"],
    ),
    (
        "the wheel turns the tempo from anywhere",
        "                if self.target_at(event.x, event.y) != Some(Target::Bpm) {",
        "                if false {",
        ["the_wheel_turns_the_tempo"],
    ),
    (
        "a notch of the wheel is three beats a minute",
        "                let notches = self.wheel.rows_at(dy, 1.0);",
        "                let notches = self.wheel.rows(dy);",
        ["the_wheel_turns_the_tempo"],
    ),
    (
        "Space starts the beat through the list of keys",
        "        if self.show_help {\n"
        "            // Modal: Space would start the beat from behind the card.",
        "        if false {\n"
        "            // Modal: Space would start the beat from behind the card.",
        ["the_shortcut_list_reaches_the_window"],
    ),
    (
        "Forget taps is offered with nothing to forget",
        "            !self.tap_times_ms.is_empty(),",
        "            true,",
        ["every_control_answers_the_pointer"],
    ),
    (
        "Reset leaves it running",
        "    fn reset(&mut self) {\n"
        "        self.playing = false;",
        "    fn reset(&mut self) {",
        ["every_control_answers_the_pointer", "key_r_resets"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "metronome", timeout=600, only=only))
