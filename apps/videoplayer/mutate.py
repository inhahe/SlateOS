"""Mutation test for the video player's language preferences.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: the preferred audio and subtitle languages
were drawn -- "Subtitle: eng | Audio: Any" -- with no row to change them and
nothing that read them; a file opened with its own default tracks whatever
they said.  They are rows now, and a file opens with the tracks in them.

And, the same day, every setting is kept in `videoplayer.yaml`: none was, so
each lasted exactly as long as the window it was chosen in.  The message of
the moment is drawn on every tab, whole.  And every row does what it says or
says it cannot: Auto-load Subtitles loads the `.srt` beside a film, Remember
Volume keeps the volume, the four rows that act on playback say they are not
applied, and the seek steps have one source.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROWS = "the_language_rows_step_through_the_languages_and_back_to_the_files_own"
SPELLINGS = "a_track_is_in_a_language_by_any_of_its_three_spellings"
OPENS = "a_file_opens_with_the_preferred_languages_tracks"
SETTINGS_TAB = "a_language_chosen_on_the_settings_tab_chooses_the_track_of_the_next_file"
KEPT = "a_setting_changed_is_the_next_windows"
QUIET = "a_player_a_test_builds_keeps_nothing"
LEAVES = "a_language_back_to_the_files_own_leaves_the_file"
UNKNOWN = "what_the_file_holds_that_is_not_a_setting_is_said_and_the_setting_kept"
UNSAVED = "a_setting_that_cannot_be_saved_says_so"
EVERY_TAB = "the_message_is_drawn_on_every_tab_and_whole"
SAID_ON_SETTINGS = "a_setting_changed_is_said_on_the_settings_tab"
SUBS_BESIDE = "a_films_own_subtitles_are_loaded_from_beside_it_and_drawn_at_their_time"
SUBS_LANGUAGE = "the_preferred_languages_subtitles_come_first_and_the_switch_turns_them_off"
SUBS_UNICODE = "a_subtitle_file_is_read_as_unicode_and_one_in_a_code_page_is_refused_and_said"
SUBS_ODD = "a_subtitle_file_too_large_or_a_folder_by_that_name_is_not_read"
CANDIDATES = "the_candidates_are_the_languages_codes_then_the_films_own_name"
VOLUME = "a_remembered_volume_is_the_next_windows_and_a_forgotten_one_is_not"
VOLUME_BAD = "a_kept_volume_that_is_not_one_is_said_and_the_normal_level_kept"
NOT_APPLIED = "a_row_that_acts_on_playback_says_it_is_not_applied"
SEEK = "the_seek_keys_move_by_the_steps_their_labels_and_the_settings_tab_name"

MUTATIONS = [
    (
        "a BCP 47 tag's region is not cut off",
        "        let primary = tag.split(['-', '_']).next().unwrap_or(tag);",
        "        let primary = tag;",
        [SPELLINGS, OPENS],
    ),
    (
        "the bibliographic code is not matched",
        "        [self.code, self.bibliographic, self.two_letter]",
        "        [self.code, self.code, self.two_letter]",
        [SPELLINGS],
    ),
    (
        "the two-letter code is not matched",
        "        [self.code, self.bibliographic, self.two_letter]",
        "        [self.code, self.bibliographic, self.code]",
        [SPELLINGS, OPENS],
    ),
    (
        "the last language does not come back to the file's own",
        "        LANGUAGES.get(at.saturating_add(1)).copied()",
        "        LANGUAGES.get(at.saturating_add(1)).or(LANGUAGES.last()).copied()",
        [ROWS],
    ),
    (
        "the audio row changes nothing",
        "                prefs.audio_preferred_lang = Language::after(prefs.audio_preferred_lang);",
        "",
        [ROWS, SETTINGS_TAB],
    ),
    (
        "the subtitle row changes nothing",
        "                prefs.subtitle_preferred_lang = Language::after(prefs.subtitle_preferred_lang);",
        "",
        [ROWS],
    ),
    (
        "a file opens with its own sound whatever the preference",
        "        self.selected_audio_track = opening_audio(&file, self.preferences.audio_preferred_lang);",
        "        self.selected_audio_track = opening_audio(&file, None);",
        [OPENS, SETTINGS_TAB],
    ),
    (
        "a file opens with its own subtitles whatever the preference",
        "            opening_subtitle(&file, self.preferences.subtitle_preferred_lang);",
        "            opening_subtitle(&file, None);",
        [OPENS],
    ),
    (
        "a commentary in the language is taken before its main track",
        "        .find(|a| a.is_default)\n        .or_else(|| file.audio_streams.iter().find(wanted))",
        "        .find(|_| true)\n        .or_else(|| file.audio_streams.iter().find(wanted))",
        [OPENS],
    ),
    (
        "forced subtitles are taken before full ones",
        "        .find(|s| !s.is_forced)",
        "        .find(|_| true)",
        [OPENS],
    ),
    (
        "a change on the Settings tab is not saved",
        "                        self.save_settings();",
        "",
        [KEPT, UNSAVED],
    ),
    (
        "a player a test builds writes the settings file",
        "        if !self.keeps_settings {\n            return;\n        }\n        let mut doc",
        "        let mut doc",
        [QUIET],
    ),
    (
        "the kept settings are not read",
        "        let mut problems = read_preferences(&doc, &mut self.preferences);",
        "        let mut problems: Vec<String> = Vec::new();",
        [KEPT],
    ),
    (
        "a kept switch is not taken",
        "            (Some(on), _) => *flag = on,",
        "            (Some(_), _) => {}",
        [KEPT, UNKNOWN],
    ),
    (
        "a switch that is not true or false is not said",
        "            (None, Some(value)) => {\n                unknown(",
        "            (None, Some(value)) => {\n                (|_: &str, _: &str, _: &str, _: &str| {})(",
        [UNKNOWN],
    ),
    (
        "a kept language is not taken",
        "                Some(found) => *language = Some(*found),",
        "                Some(_) => {}",
        [KEPT, UNKNOWN],
    ),
    (
        "a kept on_finish is not taken",
        "            Some(action) => prefs.on_finish = action,",
        "            Some(_) => {}",
        [KEPT],
    ),
    (
        "a kept deinterlace mode is not taken",
        "            Some(mode) => prefs.deinterlace = mode,",
        "            Some(_) => {}",
        [UNKNOWN],
    ),
    (
        "a language back to the file's own stays in the file",
        "            None => {\n                doc.remove(&[key]);\n            }",
        "            None => {}",
        [LEAVES],
    ),
    (
        "on_finish is not written",
        '    doc.set_str(&["on_finish"], prefs.on_finish.yaml_name());',
        "",
        [KEPT],
    ),
    (
        "the message is drawn on the player tab alone",
        "        self.render_osd(&mut cmds);",
        "        if self.active_tab == PlayerTab::Player {\n            self.render_osd(&mut cmds);\n        }",
        [EVERY_TAB, SAID_ON_SETTINGS, UNSAVED],
    ),
    (
        "the message box is 200 wide whatever it says",
        "        let width = (text::measure(msg, SIZE, FontWeightHint::Bold) + 2.0 * PAD).min(room);",
        "        let width = 200.0_f32.min(room);",
        [EVERY_TAB],
    ),
    (
        "the message box runs past a narrow window",
        "        let width = (text::measure(msg, SIZE, FontWeightHint::Bold) + 2.0 * PAD).min(room);",
        "        let width = (text::measure(msg, SIZE, FontWeightHint::Bold) + 2.0 * PAD).max(room);",
        [EVERY_TAB],
    ),
    (
        "a save that failed is not said",
        '                "Changed until the window closes -- it was not saved: {e}"',
        '                "{e}"',
        [UNSAVED],
    ),
    # -- Auto-load Subtitles ---------------------------------------------------
    (
        "an opened film's subtitles are not looked for",
        "                match self.load_sibling_subtitles() {",
        "                match None::<String> {",
        [SUBS_BESIDE],
    ),
    (
        "the switch is ignored",
        "        if !self.preferences.subtitle_auto_load {\n            return None;\n        }",
        "",
        [SUBS_LANGUAGE],
    ),
    (
        "a missing file stops the search",
        "                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,",
        "                Err(e) if e.kind() == std::io::ErrorKind::NotFound && false => continue,",
        [SUBS_LANGUAGE],
    ),
    (
        "a folder is taken for a file",
        "            if !meta.is_file() {\n                continue;\n            }",
        "",
        [SUBS_ODD],
    ),
    (
        "a file of any size is read",
        "            if meta.len() > MAX_SUBTITLE_FILE {",
        "            if meta.len() > MAX_SUBTITLE_FILE && false {",
        [SUBS_ODD],
    ),
    (
        "the cues read are not kept",
        "            self.external_subtitles = cues;",
        "            drop(cues);",
        [SUBS_BESIDE, SUBS_LANGUAGE, SUBS_UNICODE],
    ),
    (
        "one code is tried twice",
        "            if !out.contains(&path) {\n                out.push(path);\n            }",
        "            out.push(path);",
        [CANDIDATES],
    ),
    (
        "the two-letter name is not tried",
        "        for code in [language.two_letter, language.code, language.bibliographic] {",
        "        for code in [language.code, language.bibliographic] {",
        [CANDIDATES, SUBS_LANGUAGE],
    ),
    (
        "a UTF-8 mark is read as a character",
        "    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {",
        "    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF, 0xEF]) {",
        [SUBS_UNICODE],
    ),
    (
        "little-endian UTF-16 is read big-endian",
        "        return utf16(rest, true);",
        "        return utf16(rest, false);",
        [SUBS_UNICODE],
    ),
    (
        "a byte left over from UTF-16 is dropped",
        "        if !pairs.remainder().is_empty() {",
        "        if false {",
        [SUBS_UNICODE],
    ),
    # -- Remember Volume ------------------------------------------------------
    (
        "a changed volume is not kept",
        "        self.volume.decrease(5);\n        self.show_osd(&format!(\"Volume: {}\", self.volume.label()));\n        self.keep_volume();",
        "        self.volume.decrease(5);\n        self.show_osd(&format!(\"Volume: {}\", self.volume.label()));",
        [VOLUME],
    ),
    (
        "a kept volume is not taken",
        "                (Some(level), _) => self.volume.set_level(level),",
        "                (Some(_), _) => {}",
        [VOLUME],
    ),
    (
        "a kept volume past the loudest is taken",
        "                .filter(|level| *level <= Volume::MAX);",
        ";",
        [VOLUME_BAD],
    ),
    (
        "the volume is not written",
        "            doc.set_i64(&[VOLUME_KEY], i64::from(self.volume.level()));",
        "",
        [VOLUME],
    ),
    (
        "a forgotten volume stays in the file",
        "            doc.remove(&[VOLUME_KEY]);",
        "",
        [VOLUME],
    ),
    # -- Not applied ----------------------------------------------------------
    (
        "no row acts on playback",
        "            Self::ResumePlayback | Self::HardwareDecode | Self::OnFinish | Self::Deinterlace",
        "            Self::ResumePlayback | Self::HardwareDecode | Self::OnFinish",
        [NOT_APPLIED],
    ),
    (
        "the panel does not say a row is not applied",
        "            if not_applied {",
        "            if not_applied && false {",
        [NOT_APPLIED],
    ),
    (
        "a player that decodes says rows are not applied",
        "            let not_applied = row.needs_decoding() && !self.decodes;",
        "            let not_applied = row.needs_decoding();",
        [NOT_APPLIED],
    ),
    (
        "a change does not say it is not applied",
        "                        if row.needs_decoding() && !self.decodes {",
        "                        if row.needs_decoding() && !self.decodes && false {",
        [NOT_APPLIED],
    ),
    # -- The seek steps -------------------------------------------------------
    (
        "a key seeks by other than its label says",
        "                Command::SeekBy(SEEK_SMALL_MS),",
        "                Command::SeekBy(5_000),",
        [SEEK],
    ),
    (
        "the settings tab names another step",
        "                SEEK_SMALL_MS / 1000,",
        "                5,",
        [SEEK],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "videoplayer", timeout=900, only=only))
