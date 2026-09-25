"""Mutation test for the media converter: its queue, its conversions, its
settings and its pointer layer.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Nothing could be added and nothing could start: the program converted nothing
(known-issues, TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED).
It converts WAV to WAV and PNG or JPEG to BMP now, through `src/engine.rs`,
which has its own table below.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"
ENGINE_SRC = Path(__file__).parent / "src" / "engine.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- the queue ----------------------------------------------------------------------------------------
    (
        "two jobs run at once",
        "        if self.worker.is_some() {\n"
        "            return false;\n"
        "        }",
        "        if false {\n"
        "            return false;\n"
        "        }",
        ["only_one_job_runs_at_a_time"],
    ),
    (
        "a finished job is never recorded",
        "                Ok(bytes) => {\n"
        "                    self.complete_job(id, bytes);",
        "                Ok(bytes) => {\n"
        "                    let _ = bytes;",
        ["a_wav_becomes_the_wav_the_profile_names"],
    ),
    (
        "a failed job looks cancelled",
        '                Err(why) if why == "cancelled" => {',
        "                Err(why) if !why.is_empty() => {",
        ["a_file_that_is_not_what_its_name_says_fails_and_says_why"],
    ),
    (
        "cancel does not reach the job",
        "            worker.cancel();",
        "",
        ["a_cancelled_job_writes_nothing"],
    ),
    (
        "another job's output is written over",
        "            p.exists() || planned.iter().any(|q| q == p)",
        "            p.exists()",
        ["two_files_of_one_name_get_two_outputs"],
    ),
    (
        "the next job never starts after the last",
        "            self.start_next_job();\n"
        "        }\n"
        "        EventResult::Consumed",
        "        }\n"
        "        EventResult::Consumed",
        ["only_one_job_runs_at_a_time"],
    ),
    # -- the sources -----------------------------------------------------------------------------------------
    (
        "a file is added without its length",
        "                        source.duration_secs = Some(info.seconds());",
        "",
        ["a_file_is_added_as_it_really_is"],
    ),
    (
        "the same file is added twice",
        "        if self.sources.iter().any(|s| s.path == path) {",
        "        if false {",
        ["a_file_is_added_as_it_really_is"],
    ),
    # -- the settings ------------------------------------------------------------------------------------------
    (
        "Up and Down do not walk the settings",
        "            Key::Up | Key::Down if self.active_panel == ActivePanel::Settings => {",
        "            Key::Up | Key::Down if false => {",
        ["the_settings_are_walked_and_changed_by_the_keys"],
    ),
    (
        "a new profile leaves the keys on a row it does not have",
        "            if !setting_rows(self.output_format().as_ref()).contains(&self.setting_row) {",
        "            if false {",
        ["the_settings_are_walked_and_changed_by_the_keys"],
    ),
    (
        "a pattern's date is the old constant",
        '                    .replace("{date}", &today_yyyymmdd());',
        '                    .replace("{date}", "20260518");',
        ["a_pattern_name_takes_todays_date_and_keeps_the_stem"],
    ),
    (
        "an unavailable profile does not say so",
        '                            text: format!("Not available: {why}"),',
        '                            text: format!("{why}"),',
        [
            "the_settings_panel_shows_what_applies_and_what_cannot_be_done",
            "what_cannot_be_converted_is_never_queued",
        ],
    ),
    # -- the pointer ---------------------------------------------------------------------------------------------
    (
        "Beside sources is offered when outputs go there already",
        "            self.output_dir.is_some(),",
        "            true,",
        ["every_control_answers_the_pointer"],
    ),
    (
        "a second press on a file does not queue it",
        "                if self.selected_source == Some(id) {\n"
        "                    self.queue_source(id);",
        "                if false {\n"
        "                    self.queue_source(id);",
        ["a_second_press_on_a_file_queues_it"],
    ),
    (
        "the source list does not scroll",
        "            self.source_scroll = next;",
        "            let _ = next;",
        ["the_lists_scroll"],
    ),
    (
        "a press goes through the list of keys",
        "            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));",
        "",
        ["the_list_of_keys_is_modal_to_the_pointer", "every_control_answers_the_pointer"],
    ),
    (
        "every frame scrolls back to the chosen row",
        "        self.clamp_scrolls();\n"
        "        let frame = self.frame(width, height);",
        "        self.follow_selection(true, true);\n"
        "        let frame = self.frame(width, height);",
        ["the_wheel_can_leave_the_chosen_row_behind"],
    ),
    (
        "an arrow at the end of the list leaves the row off screen",
        "                        || (walked && self.active_panel == ActivePanel::SourceList),",
        "                        || false,",
        ["the_wheel_can_leave_the_chosen_row_behind"],
    ),
    # -- the queue's own selection -------------------------------------------------------------------------
    (
        "Delete cancels the first job, not the chosen one",
        "                        Some((id, JobStatus::Queued | JobStatus::Running)) => {\n"
        "                            self.cancel_job(id);",
        "                        Some((_, JobStatus::Queued | JobStatus::Running)) => {\n"
        "                            let id = self.first_cancellable_job().unwrap_or_default();\n"
        "                            self.cancel_job(id);",
        ["a_job_is_chosen_and_delete_acts_on_it"],
    ),
    (
        "a job's row cannot be pressed",
        "            f.hit(\n"
        "                Target::QueueRow(job.id),\n"
        "                Rect::new(x + 4.0, cy, width - 8.0, QUEUE_ROW_H - 4.0),\n"
        "            );",
        "",
        ["a_job_is_chosen_and_delete_acts_on_it"],
    ),
    (
        "O opens the wrong picker",
        "            Key::O => {\n"
        "                self.open_picker(PickerFor::OutputFolder);",
        "            Key::O => {\n"
        "                self.open_picker(PickerFor::Folder);",
        ["where_outputs_go_is_chosen_by_key_too"],
    ),
]

ENGINE_MUTATIONS = [
    (
        "a file that is not a WAV is taken for one",
        '            if ext != "wav" {',
        "            if false {",
        ["what_cannot_be_done_is_refused_with_the_reason"],
    ),
    (
        "the source is written over",
        "    if first != source && !taken(&first) {",
        "    if !taken(&first) {",
        ["nothing_is_ever_written_over", "a_wav_becomes_the_wav_the_profile_names"],
    ),
    (
        "progress never moves",
        "                set(0.15 + 0.75 * f);",
        "                let _ = f;",
        ["a_running_job_moves_its_progress_bar"],
    ),
    (
        "a BMP's rows are stored top-down",
        "    for y in (0..h).rev() {",
        "    for y in 0..h {",
        ["a_picture_becomes_a_bmp_of_the_same_pixels"],
    ),
    (
        "a BMP's rows are not padded",
        "        .map(|r| r & !3)",
        "        .map(|r| r - 3)",
        ["a_picture_becomes_a_bmp_of_the_same_pixels"],
    ),
    (
        "an opaque picture is written with alpha",
        "    let alpha = pixels.iter().any(|p| p >> 24 != 0xFF);",
        "    let alpha = !pixels.is_empty();",
        ["a_picture_becomes_a_bmp_of_the_same_pixels"],
    ),
    (
        "a picture is not fitted",
        "                    w.unwrap_or(u32::MAX),",
        "                    u32::MAX,",
        ["a_picture_is_fitted_inside_the_size_asked"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    first = sweep(SRC, MUTATIONS, "mediaconvert", timeout=900, only=only)
    second = sweep(ENGINE_SRC, ENGINE_MUTATIONS, "mediaconvert", timeout=900, only=only)
    raise SystemExit(max(first, second))
