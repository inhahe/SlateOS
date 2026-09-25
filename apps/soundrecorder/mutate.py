"""Mutation test for the sound recorder: its take, its recordings folder, the
recording it opens, and its pointer layer.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It could not be clicked, and nothing after a take could be reached: Save wrote
a history entry naming a file nobody wrote, and there was no way to open a
recording at all (known-issues,
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
    # -- the take ------------------------------------------------------------------------------------
    (
        "a take starts with nothing to record from",
        "        if target == RecordingState::Recording && self.current_device().is_none() {",
        "        if false {",
        ["a_take_cannot_be_started_without_an_input_and_no_clock_runs"],
    ),
    (
        "a take is saved over a file that has its name",
        "            match safeio::write_new_atomically(&path, &bytes) {",
        "            match safeio::write_atomically(&path, &bytes) {",
        ["stop_saves_the_take_and_opens_it"],
    ),
    (
        "a take is saved as mono",
        "        let bytes = wavpcm::encode_pcm16(self.sample_rate, self.channels, &self.samples)?;",
        "        let bytes = wavpcm::encode_pcm16(self.sample_rate, 1, &self.samples)?;",
        ["stop_saves_the_take_and_opens_it"],
    ),
    (
        "a take's markers are not saved",
        "        wavpcm::with_cues(&bytes, &cues)\n"
        "    }\n"
        "}",
        "        wavpcm::with_cues(&bytes, &[])\n"
        "    }\n"
        "}",
        ["the_take_is_stored_as_it_was_captured", "stop_saves_the_take_and_opens_it"],
    ),
    (
        "Play does not say why",
        "        self.blocked_reason = Some(String::from(CANNOT_PLAY));",
        "",
        ["play_says_why_it_cannot"],
    ),
    (
        "the notice waits for a press",
        "        if self.input_devices.is_empty() {\n"
        "            for (i, line) in NO_AUDIO_LINES.iter().enumerate() {",
        "        if false {\n"
        "            for (i, line) in NO_AUDIO_LINES.iter().enumerate() {",
        ["the_window_says_it_cannot_record_before_record_is_pressed"],
    ),
    (
        "a take starts through the list of keys",
        "        if self.show_help {\n"
        "            // Modal: Space would start a take from behind the card.",
        "        if false {\n"
        "            // Modal: Space would start a take from behind the card.",
        ["the_shortcut_list_reaches_the_window"],
    ),
    # -- the folder -----------------------------------------------------------------------------------
    (
        "a long recording's length is its first megabyte's",
        "            detail: wavpcm::parse_header_prefix(&head.bytes, meta.len()).map_err(|e| e.to_string()),",
        "            detail: wavpcm::parse_header(&head.bytes).map_err(|e| e.to_string()),",
        ["the_folder_lists_its_recordings_as_they_are"],
    ),
    (
        "files that are not WAV are listed",
        "            .is_some_and(|e| e.eq_ignore_ascii_case(\"wav\"));",
        "            .is_some();",
        ["the_folder_lists_its_recordings_as_they_are"],
    ),
    (
        "a folder not made yet reads as unreadable",
        "            Err(_) if !dir.exists() => {",
        "            Err(_) if false => {",
        ["the_folder_lists_its_recordings_as_they_are"],
    ),
    (
        "unsaved markers are left by one press",
        "            && open.markers_changed\n",
        "            && false\n",
        ["unsaved_markers_are_not_left_by_a_single_press"],
    ),
    # -- the open recording -----------------------------------------------------------------------------
    (
        "markers are saved over a file changed on disk",
        "        if Stamp::of(&self.path)? != self.stamp {",
        "        if false {",
        ["markers_are_not_saved_over_a_file_changed_on_disk"],
    ),
    (
        "the kept part is saved over the recording",
        "        if same_file(to, &self.path) {",
        "        if false {",
        ["the_kept_part_is_saved_as_a_file_of_its_own"],
    ),
    (
        "the kept part leaves out markers not yet saved",
        "        let current = wavpcm::with_cues(&self.bytes, &self.markers).map_err(fail)?;",
        "        let current = self.bytes.clone();",
        ["the_kept_part_is_saved_as_a_file_of_its_own"],
    ),
    (
        "Right moves nothing",
        "                    let step = if shift { 1.0 } else { 0.1 };",
        "                    let step = if shift { 1.0 } else { 0.0 };",
        ["the_cursor_moves_by_the_keys"],
    ),
    (
        "Ctrl+Left looks forward",
        "            self.markers.iter().rposition(|m| u64::from(m.frame) < at)",
        "            self.markers.iter().position(|m| u64::from(m.frame) > at)",
        ["the_cursor_moves_by_the_keys"],
    ),
    (
        "a new marker goes to the end of the list",
        "        let at = self.markers.partition_point(|m| m.frame < frame);",
        "        let at = self.markers.len();",
        ["markers_are_named_removed_and_saved_into_the_file"],
    ),
    (
        "a marker's new name is not kept",
        "            let name = input.text().trim().to_owned();",
        "            let name = String::from(\"Marker 1\");",
        ["markers_are_named_removed_and_saved_into_the_file"],
    ),
    (
        "unsaved markers are not flagged",
        "        if open.markers_changed {\n"
        "            let note = \"Markers not saved\";",
        "        if false {\n"
        "            let note = \"Markers not saved\";",
        ["markers_are_named_removed_and_saved_into_the_file"],
    ),
    (
        "a name that is not UTF-8 is replaced",
        "            out.push_str(&format!(\"\\\\x{b:02X}\"));",
        "            out.push('\\u{FFFD}');",
        ["a_marker_name_that_is_not_utf8_is_shown_and_kept"],
    ),
    # -- the pointer --------------------------------------------------------------------------------------
    (
        "a press on the waveform does not move the cursor",
        "            open.cursor = frame_at(r, x, frames);",
        "",
        ["every_control_answers_the_pointer"],
    ),
    (
        "a shaking hand moves the marker it chose",
        "                if slop {\n"
        "                    return EventResult::Ignored;\n"
        "                }",
        "",
        ["the_waveform_is_dragged"],
    ),
    (
        "a drag outlives its release",
        "                if self.drag.take().is_some() {",
        "                if self.drag.is_some() {",
        ["the_waveform_is_dragged"],
    ),
    (
        "the start handle cannot be taken",
        "        } else if near(open.kept.start_frame) {",
        "        } else if false {",
        ["the_waveform_is_dragged"],
    ),
    (
        "Save markers is offered with nothing to save",
        "                    Target::SaveMarkers,\n"
        "                    open.markers_changed,",
        "                    Target::SaveMarkers,\n"
        "                    true,",
        ["every_control_answers_the_pointer"],
    ),
    (
        "every frame scrolls back to the choice",
        "        self.clamp_scrolls();\n"
        "        let frame = self.frame();",
        "        self.follow_choice(true, true);\n"
        "        let frame = self.frame();",
        ["the_list_scrolls_and_follows_the_keys"],
    ),
    (
        "the list does not follow the keys",
        "                    self.chosen_entry != before.0 || matches!(key.key, Key::Up | Key::Down),",
        "                    false,",
        ["the_list_scrolls_and_follows_the_keys"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "soundrecorder", timeout=900, only=only))
