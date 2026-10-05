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

And on 2026-10-04 the player began to play: a film's pictures are decoded
on a thread of their own (`src/pictures.rs`, swept by `PICTURES_MUTATIONS`)
and taken as the player's clock reaches them; every jump of the clock moves
the picture with it; the film ends when its last picture's time is up; and
each settings row that changes nothing says its own reason, the one they
shared -- "nothing here decodes video" -- having stopped being true.  The
same day the seek bar, the volume and the Adjustments tab became the
toolkit's slider, and the adjustments are applied to the picture
(`src/grade.rs`, swept by `GRADE_MUTATIONS`).

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
NOT_APPLIED = "a_row_that_changes_nothing_says_why"
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
    # -- Not applied: each row's own reason (2026-10-04) ------------------------
    (
        "the deinterlace row is said to act",
        '                Some("Not applied: no film played here is interlaced (VP8, VP9 and AV1 never are)")\n',
        "                None\n",
        [NOT_APPLIED],
    ),
    (
        "the panel does not say why a row changes nothing",
        "            if let Some(note) = not_applied {",
        "            if let Some(note) = not_applied.filter(|_| false) {",
        [NOT_APPLIED],
    ),
    (
        "a change does not say why it changes nothing",
        "                        if let Some(why) = row.not_applied() {",
        "                        if let Some(why) = row.not_applied().filter(|_| false) {",
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
    (
        'a plain binding answers the Windows key',
        '            Self::Plain(key) => event.key == key && plain && !mods.shift,',
        '            Self::Plain(key) => event.key == key && !mods.shift && !mods.ctrl && !mods.alt,',
        ['a_key_held_with_the_windows_key_is_not_the_players'],
    ),
    (
        'a shifted binding answers the Windows key',
        '            Self::Shift(key) => event.key == key && plain && mods.shift,',
        '            Self::Shift(key) => event.key == key && mods.shift && !mods.ctrl && !mods.alt,',
        ['a_key_held_with_the_windows_key_is_not_the_players'],
    ),
    (
        'a Ctrl binding answers the Windows key',
        '            Self::Ctrl(key) => event.key == key && textline::is_ctrl_chord(mods),',
        '            Self::Ctrl(key) => event.key == key && mods.ctrl && !mods.alt,',
        ['a_key_held_with_the_windows_key_is_not_the_players'],
    ),
    (
        'a Ctrl binding answers AltGr',
        '            Self::Ctrl(key) => event.key == key && textline::is_ctrl_chord(mods),',
        '            Self::Ctrl(key) => event.key == key && mods.ctrl && !mods.super_key,',
        ['a_key_held_with_the_windows_key_is_not_the_players'],
    ),
    (
        'the digits answer the Windows key',
        '            Self::Digit => Self::digit_of(event.key).is_some() && plain,',
        '            Self::Digit => Self::digit_of(event.key).is_some() && !mods.ctrl && !mods.alt,',
        ['a_key_held_with_the_windows_key_is_not_the_players'],
    ),
]

# F1 (2026-10-04): it did nothing, though the whole table was a tab away.
F1 = "f1_shows_the_keys_and_goes_back"

MUTATIONS += [
    (
        "F1 shows nothing",
        "                    self.tab_before_keys = Some(self.active_tab);\n"
        "                    self.active_tab = PlayerTab::Shortcuts;\n",
        "                    self.tab_before_keys = Some(self.active_tab);\n",
        [F1],
    ),
    (
        "F1 does not go back where it came from",
        "                    self.active_tab = self.tab_before_keys.take().unwrap_or(PlayerTab::Player);\n",
        "                    self.active_tab = PlayerTab::Player;\n",
        [F1],
    ),
    (
        "? shows nothing",
        "                Press::Shift(Key::Slash),\n",
        "                Press::Shift(Key::F12),\n",
        [F1],
    ),
]

# The pictures (2026-10-04): a film's pictures are decoded on a thread of
# their own (src/pictures.rs) and taken as the clock reaches them.
FIRST = "a_film_opened_shows_its_first_picture"
CLOCK = "the_picture_follows_the_clock"
PAUSED = "a_paused_film_keeps_its_picture"
SEEK_TO = "a_seek_shows_the_picture_at_the_time_sought"
STOP = "stop_shows_the_first_picture"
END = "the_film_ends_when_its_last_picture_has_been_shown"
REPEAT = "repeat_one_plays_the_pictures_again"
DRAG = "dragging_the_seek_bar_shows_the_key_frames_on_the_way"
TICK_SEEK = "a_paused_film_shows_a_seeks_picture_on_its_next_tick"
STOPPED = "a_film_whose_pictures_stop_says_so_once"
LENGTH = "a_film_whose_header_gives_no_length_takes_its_videos"
AWAY = "opening_a_file_that_does_not_decode_takes_the_picture_away"
PLACED = "the_picture_is_placed_as_the_aspect_mode_says"
SHAPE = "a_picture_is_drawn_at_its_display_shape"
INTERVAL = "the_clock_asks_for_the_next_pictures_time"
WAKER = "a_film_opened_before_the_waker_is_given_wakes_the_window"

MUTATIONS += [
    (
        "opening a film does not start its pictures",
        "        self.external_subtitles.clear();\n        self.open_pictures();\n    }\n",
        "        self.external_subtitles.clear();\n    }\n",
        [FIRST],
    ),
    (
        "a jump leaves the picture behind",
        "            pictures.seek(nanos(position), SeekMode::Exact);\n",
        "            let _ = (nanos(position), SeekMode::Exact);\n",
        [SEEK_TO, STOP, REPEAT],
    ),
    (
        "a seek lands on the key frame before its time",
        "            pictures.seek(nanos(position), SeekMode::Exact);\n",
        "            pictures.seek(nanos(position), SeekMode::KeyFrame);\n",
        # Not SEEK_TO: with the clock at the time sought, a key-frame seek
        # ends on the same picture, the thread passing over what is behind
        # the clock -- the two part only for a decoder slower than
        # LONGEST_UNSHOWN over a key frame's distance, which no film here is.
        ["a_jump_of_the_clock_seeks_exactly_and_a_drag_to_a_key_frame"],
    ),
    (
        "the clock does not move the picture",
        "            self.refresh_picture();\n            if self.run_out() {\n",
        "            if self.run_out() {\n",
        [CLOCK],
    ),
    (
        "the pictures' end is not the film's",
        "                (pictures.ended() || pictures.failed().is_some())\n",
        "                pictures.failed().is_some()\n",
        [END],
    ),
    (
        "a film ends while its last picture still shows",
        "                        .is_none_or(|shown| nanos(self.position) >= shown.until)\n",
        "                        .is_none_or(|_| true)\n",
        [END],
    ),
    (
        "play at the end ends again at once",
        "        if self.run_out() {\n            self.jump_to(Duration::ZERO);\n        }\n",
        "",
        [END],
    ),
    (
        "repeat one leaves the picture at the end",
        "            RepeatMode::One => self.jump_to(Duration::ZERO),\n",
        "            RepeatMode::One => self.position = Duration::ZERO,\n",
        [REPEAT],
    ),
    (
        "a picture is uploaded under another number",
        "            id: PICTURE_IMAGE,\n            width: frame.width,\n",
        "            id: PICTURE_IMAGE + 1,\n            width: frame.width,\n",
        [FIRST],
    ),
    (
        "a picture superseded before it was sent is sent",
        "        self.pending_images.retain(|change| !is_picture(change));\n"
        "        self.pending_images.push(ImageChange::Upload {\n",
        "        self.pending_images.push(ImageChange::Upload {\n",
        [CLOCK],
    ),
    (
        "closing a film leaves its picture stored",
        "        if self.shown.take().is_some() {\n",
        "        if self.shown.take().is_some() && false {\n",
        [AWAY],
    ),
    (
        "the picture is not drawn",
        "        } else if let Some(at) = self.picture_rect(area) {\n",
        "        } else if let Some(at) = None::<Rect> {\n",
        [FIRST],
    ),
    (
        "a picture is drawn at its pixels' shape",
        "            Some(info) if info.width > 0 && info.height > 0 => (\n",
        "            Some(info) if false => (\n",
        [SHAPE],
    ),
    (
        "Fit fills the area",
        "            let scale = (area.w / width).min(area.h / height);\n",
        "            let scale = (area.w / width).max(area.h / height);\n",
        [FIRST, PLACED],
    ),
    (
        "Fill fits in the area",
        "            let scale = (area.w / width).max(area.h / height);\n",
        "            let scale = (area.w / width).min(area.h / height);\n",
        [PLACED],
    ),
    (
        "Original is fitted to the area",
        "        AspectMode::Original => (width, height),\n",
        "        AspectMode::Original => (area.w, area.h),\n",
        [PLACED],
    ),
    (
        "a custom shape is the picture's own",
        "                aspect_w as f32 / aspect_h as f32\n",
        "                width / height\n",
        [PLACED],
    ),
    (
        "the picture is not centred",
        "        x: area.x + (area.w - w) / 2.0,\n",
        "        x: area.x,\n",
        [FIRST, PLACED],
    ),
    (
        "the next picture's time is not waited for",
        "        let ahead = due.saturating_sub(nanos(self.position)).max(0);\n",
        "        let ahead = i64::MAX;\n",
        [INTERVAL],
    ),
    (
        "the wait is the same at any speed",
        "            / self.speed.value()) as u64;\n",
        "            / 1.0) as u64;\n",
        [INTERVAL],
    ),
    (
        "the window's waker is not kept",
        "        self.waker.get_or_init(|| waker);\n",
        "        drop(waker);\n",
        [WAKER],
    ),
    (
        "a wake shows no picture",
        "        if self.refresh_picture() {\n            Response::Redraw\n",
        "        if false {\n            Response::Redraw\n",
        [WAKER],
    ),
    (
        "no picture is ever uploaded",
        "        std::mem::take(&mut self.pending_images)\n",
        "        Vec::new()\n",
        [FIRST],
    ),
    (
        "dragging the seek bar moves no picture",
        "            pictures.seek(nanos(at), SeekMode::KeyFrame);\n",
        "            let _ = (nanos(at), SeekMode::KeyFrame);\n",
        [DRAG],
    ),
    (
        "a drag's key frame runs on to the clock",
        "        let now = if self.seeking {\n",
        "        let now = if false {\n",
        [DRAG],
    ),
    (
        "a paused film's tick shows nothing",
        "        } else if self.refresh_picture() {\n",
        "        } else if false {\n",
        [TICK_SEEK],
    ),
    (
        "the pictures stopping is not said",
        "            Some(why) if !self.failure_said => {\n",
        "            Some(why) if false => {\n",
        [STOPPED],
    ),
    (
        "the pictures stopping is said on every look",
        "            Some(why) if !self.failure_said => {\n",
        "            Some(why) => {\n",
        [STOPPED],
    ),
    (
        "a film with no length in its header has none",
        "            .and_then(|info| info.duration)\n",
        "            .and_then(|_| None::<u64>)\n",
        [LENGTH],
    ),
]

# The thread itself (src/pictures.rs).
PICTURES_SRC = Path(__file__).parent / "src" / "pictures.rs"
LEFT_BEHIND = "a_picture_the_clock_has_left_behind_is_not_converted"
LATE_START = "a_film_that_starts_late_shows_its_first_picture_at_once"
SEEK_STALE = "a_seek_shows_the_picture_at_its_time_and_nothing_from_before_it"
DUE = "the_first_picture_shows_at_once_and_the_rest_when_due"
FAILURE = "a_failure_is_said_after_the_pictures_before_it"
FULL_QUEUE = "a_seek_reaches_a_thread_waiting_on_a_full_queue"
TWO_SEEKS = "two_seeks_in_a_row_land_on_the_second"
DROPPED = "dropping_the_pictures_ends_the_thread"
SLOW = "a_decoder_slower_than_the_film_still_moves_the_picture"

PICTURES_MUTATIONS = [
    (
        "a picture behind the clock is converted",
        "                    if S::time(&after) <= clock.load(Ordering::Relaxed)\n",
        "                    if false\n",
        [LEFT_BEHIND],
    ),
    (
        "a decoder slower than the film freezes the picture",
        "                        && passing_since.elapsed() < LONGEST_UNSHOWN =>\n",
        "                        && true =>\n",
        [SLOW],
    ),
    (
        "the clock is not told to the thread",
        "    pub fn show_at(&mut self, now: i64) -> Option<Ready> {\n"
        "        self.clock.store(now, Ordering::Relaxed);\n",
        "    pub fn show_at(&mut self, now: i64) -> Option<Ready> {\n",
        [LEFT_BEHIND],
    ),
    (
        "a frame from before a seek is shown after it",
        "            Delivery::Frame { seek, frame } if seek == self.seek => Some(frame),\n",
        "            Delivery::Frame { frame, .. } => Some(frame),\n",
        [SEEK_STALE],
    ),
    (
        "the first picture waits for its time",
        "                Some(frame) if self.fresh || frame.time <= now => {\n",
        "                Some(frame) if frame.time <= now => {\n",
        [LATE_START],
    ),
    (
        "a later picture is shown before its time",
        "                Some(frame) if self.fresh || frame.time <= now => {\n",
        "                Some(frame) if true => {\n",
        [DUE],
    ),
    (
        "the film's end is a failure",
        "                    _ => Delivery::End { seek },\n",
        "                    _ => Delivery::Failed {\n                        seek,\n                        why: String::new(),\n                    },\n",
        [DUE],
    ),
    (
        "a failure is the end",
        "                self.failed = Some(why);\n                None\n",
        "                self.failed = Some(why);\n                self.end_taken = true;\n                None\n",
        [FAILURE],
    ),
    (
        "a seek waits for the window to take from a full queue",
        "        while self.delivered.try_recv().is_ok() {}\n",
        "",
        [FULL_QUEUE],
    ),
    (
        "the first of two seeks is the one taken",
        "                Ok(ask) => asked = Some(ask),\n                Err(TryRecvError::Empty) => break,\n",
        "                Ok(ask) => {\n                    if asked.is_none() {\n                        asked = Some(ask);\n                    }\n                }\n                Err(TryRecvError::Empty) => break,\n",
        [TWO_SEEKS],
    ),
    # No row for `hand` saying the window is there when its send failed: the
    # thread's next look at its asks finds the window gone too, and ends --
    # the two guards are one, and dropping_the_pictures_ends_the_thread holds
    # them together.
]

# The sliders (2026-10-04, c-e-the-toolkit-has-a-slider-now.md): the seek
# bar, the volume and the Adjustments tab's six are the toolkit's; the
# adjustments are applied to the picture (src/grade.rs), on the thread.
SEEK_THUMB_T = "a_press_on_the_seek_bars_thumb_holds_it_where_it_is"
SEEK_ESCAPE = "escape_takes_a_seek_bar_drag_back"
SEEK_TABS = "the_seek_bar_answers_only_on_the_player_tab"
VOLUME_DRAG = "the_volume_bar_drags"
ADJUST_KEYS = "an_adjustment_moved_with_the_keys_changes_the_picture"
ADJUST_DRAG = "an_adjustment_drags_and_reset_all_puts_the_picture_back"
ADJUST_ESCAPE = "escape_takes_an_adjustment_drag_back"

MUTATIONS += [
    (
        "the seek bar answers on every tab",
        "                    PlayerTab::Adjustments => self.adjustments_press(event),\n                    _ => false,\n",
        "                    PlayerTab::Adjustments => self.adjustments_press(event),\n                    _ => self.seek_bar_mouse(event),\n",
        [SEEK_TABS],
    ),
    (
        "a key during a drag is the player's",
        "        if let Some(taken) = self.drag_key(event) {\n            return taken;\n        }\n",
        "",
        [SEEK_ESCAPE, ADJUST_ESCAPE],
    ),
    (
        "Escape leaves the seek bar's drag on",
        "            let response = self.seek_slider.handle_key(event);\n",
        "            let response = guitk::slider::Response::Taken;\n",
        [SEEK_ESCAPE],
    ),
    (
        "a drag taken back leaves the picture at the drag",
        "            Some(SliderEvent::Cancelled(_)) => {\n                self.seek_preview_position = None;\n                let here = self.position;\n                self.jump_to(here);\n",
        "            Some(SliderEvent::Cancelled(_)) => {\n                self.seek_preview_position = None;\n",
        [SEEK_ESCAPE],
    ),
    (
        "the volume bar moves no volume",
        "        self.volume.set_level(level);\n",
        "        let _ = level;\n",
        [VOLUME_DRAG],
    ),
    (
        "an adjustment changes no picture",
        "            Ok(mut slot) => *slot = grade,\n",
        "            Ok(_) => drop(grade),\n",
        [ADJUST_KEYS, ADJUST_DRAG],
    ),
    (
        "a paused picture keeps the old adjustments",
        "        if self.state != PlaybackState::Playing && self.pictures.is_some() {\n",
        "        if false {\n",
        [ADJUST_KEYS, ADJUST_DRAG],
    ),
    (
        "the Adjustments tab's keys are the player's",
        "        if self.active_tab == PlayerTab::Adjustments && self.adjustments_key(event) {\n",
        "        if false && self.adjustments_key(event) {\n",
        [ADJUST_KEYS],
    ),
    (
        "Down goes past the last slider",
        "                    .min(ADJUSTMENTS.len().saturating_sub(1));\n",
        ";\n",
        [ADJUST_KEYS],
    ),
    (
        "a key moves the first slider whichever has the keyboard",
        "                let response = stored.handle_key(event);\n                if let Some(moved) = response.event() {\n                    self.adjusted(row, moved.value());\n",
        "                let response = stored.handle_key(event);\n                if let Some(moved) = response.event() {\n                    self.adjusted(0, moved.value());\n",
        [ADJUST_KEYS],
    ),
    (
        "Reset All resets nothing",
        "            self.video_adjustments.reset();\n",
        "",
        [ADJUST_DRAG],
    ),
    (
        "a press on a slider leaves the keyboard where it was",
        "            if self.adjust_placement(row).hit().contains(event.x, event.y) {\n                self.adjust_row = row;\n",
        "            if self.adjust_placement(row).hit().contains(event.x, event.y) {\n",
        # A press on a track sets the row through the slider's event as well;
        # only a press on the thumb, which moves nothing, leans on this line.
        ["a_press_on_an_adjustments_thumb_gives_it_the_keys"],
    ),
    (
        "an adjustment's drag goes on after the release",
        "        self.adjust_dragging = dragging.then_some(row);\n",
        "        if dragging {\n            self.adjust_dragging = Some(row);\n        }\n",
        [ADJUST_DRAG],
    ),
]

# The grade (src/grade.rs).
GRADE_SRC = Path(__file__).parent / "src" / "grade.rs"
NEUTRAL = "neutral_adjustments_are_no_grade"
BRIGHT = "brightness_lifts_and_lowers_every_channel"
CONTRAST = "contrast_turns_about_the_middle"
GAMMA = "gamma_above_one_lifts_the_middle_and_keeps_the_ends"
COLOUR = "no_saturation_is_grey_and_a_half_turn_of_hue_is_the_complement"
SHARP = "sharpness_raises_an_edge_and_leaves_flat_colour_and_the_border"

GRADE_MUTATIONS = [
    (
        "neutral adjustments still grade every picture",
        "        if !(tonal || colour || sharp) {\n            return None;\n        }\n",
        "",
        [NEUTRAL],
    ),
    (
        "brightness adds nothing",
        "        let v = ((v - 0.5) * contrast + 0.5 + brightness).clamp(0.0, 1.0);\n",
        "        let v = ((v - 0.5) * contrast + 0.5).clamp(0.0, 1.0);\n",
        [BRIGHT],
    ),
    (
        "contrast turns about black",
        "        let v = ((v - 0.5) * contrast + 0.5 + brightness).clamp(0.0, 1.0);\n",
        "        let v = (v * contrast + brightness).clamp(0.0, 1.0);\n",
        [CONTRAST],
    ),
    (
        "gamma bends the wrong way",
        "        let v = v.powf(1.0 / gamma);\n",
        "        let v = v.powf(gamma);\n",
        [GAMMA],
    ),
    (
        "saturation and hue are not applied",
        "            matrix: colour.then(|| colour_matrix(saturation, hue)),\n",
        "            matrix: None,\n",
        [COLOUR],
    ),
    (
        "hue turns the other way",
        "    let (sin, cos) = hue_degrees.to_radians().sin_cos();\n",
        "    let (sin, cos) = (-hue_degrees).to_radians().sin_cos();\n",
        [COLOUR],
    ),
    (
        "sharpness is not applied",
        "        if self.sharpen > 0 {\n",
        "        if false {\n",
        [SHARP],
    ),
    (
        "an edge is smoothed rather than sharpened",
        "                let lift = (9 * c - sum).saturating_mul(amount) / (9 * ONE);\n",
        "                let lift = (sum - 9 * c).saturating_mul(amount) / (9 * ONE);\n",
        [SHARP],
    ),
]

PICTURES_MUTATIONS += [
    # 2026-10-04: the thread passed the first picture after a seek over
    # against a clock the window had not moved yet, so a seek-bar drag
    # sometimes showed the frame after the key frame it asked for.
    (
        "the picture a seek asks for is passed over like any other",
        "            first_since_seek = true;\n            ahead = match source.seek(time, mode) {\n",
        "            ahead = match source.seek(time, mode) {\n",
        ["the_picture_a_seek_asks_for_is_not_passed_over_for_a_clock_left_behind"],
    ),
    (
        "a picture is not put through the grade",
        "        if let Some(grade) = grade {\n            grade.apply(&mut frame);\n        }\n",
        "",
        ["each_picture_goes_through_the_grade_in_the_slot"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:]
    worst = 0
    for src, rows in (
        (SRC, MUTATIONS),
        (PICTURES_SRC, PICTURES_MUTATIONS),
        (GRADE_SRC, GRADE_MUTATIONS),
    ):
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {src.name} ########")
        worst = max(worst, sweep(src, rows, "videoplayer", timeout=900, only=mine or None))
    raise SystemExit(worst)
