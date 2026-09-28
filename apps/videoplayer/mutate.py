"""Mutation test for the video player's language preferences.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: the preferred audio and subtitle languages
were drawn -- "Subtitle: eng | Audio: Any" -- with no row to change them and
nothing that read them; a file opened with its own default tracks whatever
they said.  They are rows now, and a file opens with the tracks in them.

And, the same day, every setting is kept in `videoplayer.yaml`: none was, so
each lasted exactly as long as the window it was chosen in.

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
        "        let problems = read_preferences(&settingsfile::load(CONFIG_NAME), &mut self.preferences);",
        "        let problems: Vec<String> = Vec::new();",
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
        "            None => drop(doc.remove(&[key])),",
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
        "a save that failed is not said",
        '                "Changed until the window closes -- it was not saved: {e}"',
        '                "{e}"',
        [UNSAVED],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "videoplayer", timeout=900, only=only))
