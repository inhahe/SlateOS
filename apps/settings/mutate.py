"""Mutation test for the Settings app's pages that write the desktop's files.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what lane C asked this app for on 2026-09-27 and 2026-09-28: the
taskbar's window-titles switch, the automatic mode's hours, the pointer's size
and colours, the colour and icon themes, and the Date & Time page -- each a
setting the desktop obeyed that nothing here could change.  The rest of the
suite predates this table.  The last rows, added 2026-10-04, cover the list of
keys' hold on the pointer: a press with it up puts it away and flips nothing
under it, and the wheel scrolls no dropdown it covers.

And, from 2026-10-09, the Recycle Bin page (design-decisions §1238, §1240):
every drive's bin, the default limits, and each drive's own, in `main.rs`;
what each limit offers and how it reads, in `recyclebins.rs`.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

BINS_LISTED = "the_recycle_bin_page_lists_every_drive_and_what_it_holds"
BINS_DEFAULT = "the_default_limits_are_chosen_and_kept"
BINS_OWN = "a_drive_given_limits_of_its_own_keeps_them_in_its_bin"
BINS_UNREAD = "a_drive_whose_limits_cannot_be_read_says_so"
BINS_BY_HAND = "a_limit_written_by_hand_is_offered_as_it_is"

LABELS = "test_the_taskbar_labels_toggle_is_on_the_themes_page_and_reaches_the_file"
AUTO = "the_automatic_modes_hours_are_chosen_beside_it_and_reach_the_file"
CURSOR = "the_cursor_size_and_colours_reach_the_file_the_compositor_reads"
LISTED = "the_installed_themes_are_listed_and_one_that_cannot_be_used_says_why"
CHOSEN = "a_chosen_theme_is_the_settings_and_one_mode_themes_say_so"
DATETIME = "the_date_and_time_page_sets_the_zone_the_clock_and_the_world_clocks"
CLICKED = "a_click_on_the_date_and_time_page_reaches_the_file"
FOUR = "four_world_clocks_are_the_most_and_the_page_says_so"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "the look pill carries the accent from one look to the other",
        "                    self.appearance.settings.set_surface_style(*style);",
        "                    self.appearance.settings.surface_style = *style;",
        ["each_look_keeps_its_own_accent"],
    ),
    (
        "the window-titles switch is another setting",
        "            ToggleId::TaskbarLabels => &mut self.appearance.settings.taskbar_labels,",
        "            ToggleId::TaskbarLabels => &mut self.appearance.settings.taskbar_autohide,",
        [LABELS],
    ),
    (
        "the hours are shown for every mode",
        "        if selected == ThemeMode::System {\n            let hours = self.appearance.settings.auto_light_hours;",
        "        if true {\n            let hours = self.appearance.settings.auto_light_hours;",
        [AUTO],
    ),
    (
        "the other end's hour is offered",
        "            .filter(|t| *t != other)",
        "            .filter(|t| *t != other || true)",
        [AUTO],
    ),
    (
        "the two ends are read the wrong way round",
        "            (hours.start(), hours.end())\n        } else {",
        "            (hours.end(), hours.start())\n        } else {",
        [AUTO],
    ),
    (
        "choosing the light hour moves the dark one",
        "                            notifsettings::DailyWindow::new(*chosen, hours.end())",
        "                            notifsettings::DailyWindow::new(*chosen, *chosen)",
        [AUTO],
    ),
    (
        "the pointer's size is not kept",
        "                    self.appearance.settings.cursor_size = *size;",
        "                    let _ = size;",
        [CURSOR],
    ),
    (
        "the pointer's colours are not kept",
        "                    self.appearance.settings.cursor_scheme = *scheme;",
        "                    let _ = scheme;",
        [CURSOR],
    ),
    (
        "a dark-only theme does not say so in the list",
        '                (true, false) => format!("{} (dark only)", info.name),',
        "                (true, false) => info.name.clone(),",
        [LISTED],
    ),
    (
        "a theme that cannot be used can be chosen",
        "                if let Some(info) = self.themes.get(index)\n                    && info.provides_colors()\n"
        "                {\n                    self.appearance.settings.color_theme =",
        "                if let Some(info) = self.themes.get(index)\n"
        "                {\n                    self.appearance.settings.color_theme =",
        [LISTED],
    ),
    (
        "choosing a colour theme changes nothing",
        "                    self.appearance.settings.color_theme =\n"
        "                        appearance::themes::ColorTheme::load_from(&self.theme_dirs, &info.id);",
        "                    let _ = &info.id;",
        [CHOSEN],
    ),
    (
        "choosing an icon theme changes nothing",
        "                    self.appearance.settings.icon_theme =\n"
        "                        if info.origin == appearance::themes::Origin::BuiltIn {",
        "                    let _unused =\n"
        "                        if info.origin == appearance::themes::Origin::BuiltIn {",
        [CHOSEN],
    ),
    (
        "a dark-only theme does not say so on the page",
        '                    "Dark only: this theme is shown dark whichever mode is chosen above."',
        '                    "This theme is shown dark whichever mode is chosen above."',
        [CHOSEN],
    ),
    (
        "the machine's own zone is not offered",
        "                let items = std::iter::once(AUTOMATIC_ZONE.to_string())",
        "                let items = std::iter::empty::<String>()",
        [DATETIME],
    ),
    (
        "a chosen zone is not kept",
        "                    self.datetime.settings.set_zone(Some(&zone.tz_id));",
        "                    let _ = zone;",
        [DATETIME],
    ),
    (
        "a zone already on a clock is offered again",
        "            .filter(|z| !clocks.iter().any(|c| c.label == z.city))",
        "            .filter(|z| !clocks.iter().any(|c| c.label == z.city) || true)",
        [DATETIME],
    ),
    (
        "a clock is not added",
        "                    self.datetime.settings.add_clock(&tz_id, &city);",
        "                    let _ = (tz_id, city);",
        [DATETIME],
    ),
    (
        "a fifth clock is offered",
        "        if settings.additional_clocks.len() < datetimesettings::MAX_CLOCKS {",
        "        if true {",
        [FOUR],
    ),
    (
        "Remove removes nothing",
        "                self.datetime.settings.remove_clock(index);",
        "                let _ = index;",
        [FOUR],
    ),
    (
        "a change on the page is not saved",
        "        if changed.datetime {\n            self.save_datetime();",
        "        if false {\n            self.save_datetime();",
        [CLICKED],
    ),
    (
        "the seconds switch is another setting",
        "            ToggleId::ClockSeconds => &mut self.datetime.settings.show_seconds,",
        "            ToggleId::ClockSeconds => &mut self.datetime.settings.show_date,",
        [CLICKED],
    ),
    (
        "a world clock's switch changes nothing",
        "                    .additional_clocks\n                    .get_mut(index)?\n                    .visible",
        "                    .show_day_of_week",
        [CLICKED],
    ),
    (
        "the Colors page does not read the calendar",
        "        if page == SettingsPage::Colors {\n            self.refresh_calendar_events();\n        }",
        "",
        ["an_accent_that_hides_an_event_names_it_with_a_way_to_its_colour"],
    ),
    (
        "every event is named, hidden or not",
        "        self.calendar_events\n            .iter()\n            .filter(|e| appearance::hard_to_tell_apart(e.effective_color(&pal), pal.accent))",
        "        self.calendar_events\n            .iter()\n            .filter(|_| true)",
        ["an_accent_that_hides_an_event_names_it_with_a_way_to_its_colour", "an_accent_apart_from_every_event_warns_of_none"],
    ),
    (
        "the warning is not the calendar's own test",
        "        self.calendar_events\n            .iter()\n            .filter(|e| appearance::hard_to_tell_apart(e.effective_color(&pal), pal.accent))",
        "        self.calendar_events\n            .iter()\n            .filter(|e| e.effective_color(&pal) == pal.accent)",
        ["the_events_named_are_the_ones_the_calendars_test_hides"],
    ),
    (
        "a hidden event has no button",
        "                    Some(RowHit::Press(ButtonId::EventColour(event.id))),",
        "                    None,",
        ["an_accent_that_hides_an_event_names_it_with_a_way_to_its_colour"],
    ),
    (
        "the button opens the calendar at no event",
        "                self.start_calendar(&[\"--event-colour\", &id], &what);",
        "                self.start_calendar(&[], &what);",
        ["an_accent_that_hides_an_event_names_it_with_a_way_to_its_colour"],
    ),
    (
        "every hidden event is listed",
        "            for event in hidden.iter().take(CLASHES_LISTED) {",
        "            for event in hidden.iter() {",
        ["more_hidden_events_than_are_listed_open_the_calendar"],
    ),
    (
        "more hidden events than listed are not offered the calendar",
        "            if hidden.len() > CLASHES_LISTED {",
        "            if false {",
        ["more_hidden_events_than_are_listed_open_the_calendar"],
    ),
    (
        "Look again does not look",
        "            RowHit::Press(ButtonId::LookAgain) => self.refresh_calendar_events(),",
        "            RowHit::Press(ButtonId::LookAgain) => {}",
        ["look_again_reads_the_calendar_again"],
    ),
    (
        "a calendar that cannot be read is not said",
        "                self.calendar_note = Some(format!(\n                    \"The calendar's events could not be read",
        "                let _ = Some(format!(\n                    \"The calendar's events could not be read",
        ["a_calendar_that_cannot_be_read_or_started_is_said"],
    ),
    (
        "a start that failed is said as one that worked",
        "            Err(e) => format!(\"Could not start the calendar ({CALENDAR}): {e}\"),",
        "            Err(_) => format!(\"Opening {what} in the calendar.\"),",
        ["a_calendar_that_cannot_be_read_or_started_is_said"],
    ),
    (
        "a long title is not cut",
        "    if text.chars().count() <= max {",
        "    if true {",
        ["a_long_title_is_cut_with_an_ellipsis"],
    ),
    (
        'an announced settings file is not read again',
        '            return if self.reread(group.file_name()) {',
        '            return if false && self.reread(group.file_name()) {',
        ['a_file_changed_elsewhere_is_read_again_and_not_written_back', 'every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'a settings file read again is written back',
        '            self.load_appearance();\n            before != self.appearance.settings',
        '            self.load_appearance();\n            self.save_appearance();\n            before != self.appearance.settings',
        ['a_file_changed_elsewhere_is_read_again_and_not_written_back'],
    ),
    (
        "one file's announcement reads another too",
        '            self.load_input();\n            before != self.input.settings',
        '            self.load_input();\n            self.load_appearance();\n            before != self.input.settings',
        ['a_file_changed_elsewhere_is_read_again_and_not_written_back'],
    ),
    (
        'appearance.yaml is not read again when announced',
        '        if name == appearance::CONFIG_NAME {',
        '        if name == "" {',
        ['a_file_changed_elsewhere_is_read_again_and_not_written_back'],
    ),
    (
        'input.yaml is not read again when announced',
        '        } else if name == inputsettings::CONFIG_NAME {',
        '        } else if name == "" {',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'notifications.yaml is not read again when announced',
        '        } else if name == notifsettings::CONFIG_NAME {',
        '        } else if name == "" {',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'datetime.yaml is not read again when announced',
        '        } else if name == datetimesettings::CONFIG_NAME {',
        '        } else if name == "" {',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'session.yaml is not read again when announced',
        '        } else if name == lockscreen::CONFIG_NAME {',
        '        } else if name == "" {',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'lockscreen.yaml is not read again when announced',
        '        } else if name == lockscreen::CLOCK_CONFIG {',
        '        } else if name == "" {',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'fileassoc.yaml is not read again when announced',
        '        } else if name == associations::CONFIG_NAME {',
        '        } else if name == "" {',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'appearance.yaml read again says it changed nothing',
        '            before != self.appearance.settings\n',
        '            false\n',
        ['a_file_changed_elsewhere_is_read_again_and_not_written_back'],
    ),
    (
        'input.yaml read again says it changed nothing',
        '            before != self.input.settings\n',
        '            false\n',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'notifications.yaml read again says it changed nothing',
        '            before != self.notif.settings\n',
        '            false\n',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'datetime.yaml read again says it changed nothing',
        '            before != self.datetime.settings\n',
        '            false\n',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'session.yaml read again says it changed nothing',
        '            before != self.lock_after_minutes\n',
        '            false\n',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'lockscreen.yaml read again says it changed nothing',
        '            before != (self.lock_clock_seconds, self.lock_clock_date)\n',
        '            false\n',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'fileassoc.yaml read again says it changed nothing',
        '            before.0 != self.default_apps || before.1 != self.default_app_categories\n',
        '            false\n',
        ['every_file_settings_shows_is_read_again_when_it_changes'],
    ),
    (
        'the Default Apps page lists no jobs',
        '        for (role, program) in &self.default_roles {',
        '        for (role, program) in self.default_roles.iter().take(0) {',
        ['the_default_apps_page_names_the_program_for_every_job'],
    ),
    (
        'a job nothing does is not said',
        '                None => s.value_row(role.label(), "None installed", pal.subtext0),',
        '                None => {}',
        ['the_default_apps_page_names_the_program_for_every_job'],
    ),
    (
        'no job is done by anything',
        '            .map(|&role| (role, role.filled_by(&known).map(|app| app.name.clone())))',
        '            .map(|&role| (role, None::<String>))',
        ['the_default_apps_page_names_the_program_for_every_job', 'an_installed_program_does_the_job_its_entry_claims'],
    ),
    (
        'the programs installed here are not read',
        '    let installed: Vec<String> = list.iter().map(|app| app.id.clone()).collect();',
        '    list.clear();\n    let installed: Vec<String> = list.iter().map(|app| app.id.clone()).collect();',
        ['an_installed_program_does_the_job_its_entry_claims'],
    ),
    (
        "SlateOS's own programs are not behind the installed ones",
        '            .filter(|own| !installed.contains(&own.id)),',
        '            .filter(|_| false),',
        ['the_default_apps_page_names_the_program_for_every_job'],
    ),
    (
        'the About page is listed nowhere',
        '                SettingsPage::RecycleBin,\n                SettingsPage::About,\n            ],',
        '                SettingsPage::RecycleBin,\n            ],',
        ['the_about_page_is_named_about_and_listed_under_system'],
    ),
    (
        'entering the About page reads no notices',
        '        if page == SettingsPage::About {\n            self.refresh_notices();\n        }',
        '',
        ['the_about_page_lists_the_notices_and_opens_one'],
    ),
    (
        "a notice's licence is not said",
        '                    s.value_row("Licence", &notice.licence, pal.subtext0);\n',
        '',
        ['the_about_page_lists_the_notices_and_opens_one'],
    ),
    (
        "a notice's attribution is not shown",
        '                    if let Some(attribution) = &notice.attribution {\n                        s.note(attribution, 20.0);\n                    }\n',
        '',
        ['the_about_page_lists_the_notices_and_opens_one'],
    ),
    (
        'an opened notice shows none of its text',
        '                                    for line in body.lines() {',
        '                                    for line in body.lines().take(0) {',
        ['the_about_page_lists_the_notices_and_opens_one'],
    ),
    (
        'a notice opened is never put away',
        '        if self.open_notice.as_ref().is_some_and(|(i, _)| *i == index) {\n            self.open_notice = None;\n            return;\n        }\n',
        '',
        ['the_about_page_lists_the_notices_and_opens_one'],
    ),
    (
        'a byte that is not UTF-8 is dropped',
        '            let _ = write!(out, "\\\\x{byte:02X}");',
        '            let _ = byte;',
        ['a_licence_byte_that_is_not_utf8_is_shown_escaped'],
    ),
    (
        'notices not installed are shown as none',
        '            Err(notices::NoticesError::NotInstalled { .. }) => NoticesShown::NotInstalled,',
        '            Err(notices::NoticesError::NotInstalled { .. }) => NoticesShown::Loaded(Vec::new()),',
        ['the_about_page_says_when_the_notices_are_not_installed'],
    ),
    (
        'the pictures by time of day are not on the page',
        '        self.build_wallpaper_schedule(s);\n',
        '',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page', 'a_schedule_of_more_than_two_pictures_is_listed_as_it_is'],
    ),
    (
        'a hand-written schedule is taken for a morning and an evening',
        '        [day, night] => Some((Some(day), Some(night))),\n        _ => None,',
        '        [day, night] => Some((Some(day), Some(night))),\n        _ => Some((schedule.first(), schedule.last())),',
        ['a_schedule_of_more_than_two_pictures_is_listed_as_it_is'],
    ),
    (
        'a lone picture is placed by the wrong half of the day',
        '[one] if one.from.hour() < NOON_HOUR',
        '[one] if one.from.hour() >= NOON_HOUR',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        "a new picture goes up at the evening's time",
        'let from = if day { DAY_FROM } else { NIGHT_FROM };',
        'let from = NIGHT_FROM;',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        'choosing a picture again keeps the old one',
        '            Some(entry) => entry.image = image,',
        '            Some(_) => {}',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        'a hand-written schedule is edited piecemeal',
        "            // A hand-written schedule is not this page's to edit piecemeal.\n            None => return,",
        "            // A hand-written schedule is not this page's to edit piecemeal.\n            None => None,",
        ['a_schedule_of_more_than_two_pictures_is_listed_as_it_is'],
    ),
    (
        'a new picture is left where it was added',
        '                schedule.sort_by_key(|entry| entry.from);',
        '',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        'the morning is offered times past the evening',
        'Some(other) if day => *t < other.from,',
        'Some(other) if day => *t != other.from,',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        'the evening is offered times before the morning',
        'Some(other) => *t > other.from,',
        'Some(other) => *t != other.from,',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        'a lone picture is offered the other half of the day',
        'None => (t.hour() < NOON_HOUR) == day,',
        'None => true,',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        'a time chosen for a picture is not applied',
        '                    entry.from = chosen;',
        '                    let _ = (entry, chosen);',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        "the morning's picker sets the evening's picture",
        'PickerPurpose::DayWallpaper => self.set_scheduled_picture(true, path),',
        'PickerPurpose::DayWallpaper => self.set_scheduled_picture(false, path),',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        "the morning's row shows the evening's picture",
        '                "Daytime picture",\n                day,',
        '                "Daytime picture",\n                night,',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        'Clear leaves the pictures',
        '                self.appearance.settings.wallpaper_schedule.clear();',
        '',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page', 'a_schedule_of_more_than_two_pictures_is_listed_as_it_is'],
    ),
    (
        'the picture up now is not said',
        'now.map_or_else(String::new, |up| format!(" Up now: {up}."))',
        'now.map_or_else(String::new, |_| String::new())',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        "the picture up now goes by UTC, not this machine's zone",
        '.scheduled_wallpaper_at(utc_secs, self.datetime.settings.rule(self.system_zone))',
        '.scheduled_wallpaper_at(utc_secs, datetimesettings::Tz::utc())',
        ['the_picture_up_now_is_the_one_for_this_machines_time_of_day'],
    ),
    (
        'the picture does not say a schedule hides it',
        '                if scheduled {\n                    s.note(\n                        "Not shown while there are pictures by time of day, below.",',
        '                if false {\n                    s.note(\n                        "Not shown while there are pictures by time of day, below.",',
        ['the_picture_and_the_rotation_say_when_a_schedule_hides_them'],
    ),
    (
        'the rotation does not say a schedule hides it',
        '                if scheduled {\n                    s.note(\n                        "Not shown while there are pictures by time of day, above.",',
        '                if false {\n                    s.note(\n                        "Not shown while there are pictures by time of day, above.",',
        ['the_picture_and_the_rotation_say_when_a_schedule_hides_them'],
    ),
    (
        'the rotation says the single picture is shown under a schedule',
        '            None if scheduled => s.note("No folder.", 28.0),\n',
        '',
        ['the_picture_and_the_rotation_say_when_a_schedule_hides_them'],
    ),
    (
        'no picture reads as the plain background under a schedule',
        '            None if scheduled => s.note("No picture.", 28.0),\n',
        '',
        ['the_picture_and_the_rotation_say_when_a_schedule_hides_them'],
    ),
    (
        'a chord raises the list of keys',
        '        if evt.key == Key::F1 && plain {',
        '        if evt.key == Key::F1 {',
        ['a_chord_is_neither_a_settings_key_nor_typing'],
    ),
    (
        'Alt+Escape abandons the exclusion pattern',
        '                Key::Escape if plain => {\n                    self.exclusion_draft.clear();',
        '                Key::Escape => {\n                    self.exclusion_draft.clear();',
        ['a_chord_is_neither_a_settings_key_nor_typing'],
    ),
    (
        'Alt+Enter adds the exclusion pattern',
        '                Key::Enter if plain => {\n                    self.add_exclusion();',
        '                Key::Enter => {\n                    self.add_exclusion();',
        ['a_chord_is_neither_a_settings_key_nor_typing'],
    ),
    (
        'Ctrl+F leaves the keyboard with the exclusion field',
        '            self.search_focused = true;\n            self.focused_field = None;\n',
        '            self.search_focused = true;\n',
        ['a_chord_is_neither_a_settings_key_nor_typing'],
    ),
    (
        'Alt+Escape leaves the search',
        '            if evt.key == Key::Escape && plain {\n                self.search_focused = false;',
        '            if evt.key == Key::Escape {\n                self.search_focused = false;',
        ['a_chord_is_neither_a_settings_key_nor_typing'],
    ),
    (
        'Escape cannot leave the search',
        '            if evt.key == Key::Escape && plain {\n                self.search_focused = false;',
        '            if evt.key == Key::Escape && plain && !plain {\n                self.search_focused = false;',
        ['a_chord_is_neither_a_settings_key_nor_typing'],
    ),
    (
        "a chord works the page's keys",
        '        if !plain {\n            return EventResult::Ignored;\n        }\n        // Category navigation',
        '        // Category navigation',
        ['a_chord_is_neither_a_settings_key_nor_typing'],
    ),
    (
        'a copy reaches only its own field',
        '            *clipboard = copied;\n',
        '            let _ = copied;\n',
        ['a_chord_is_neither_a_settings_key_nor_typing'],
    ),
    # -- the search box and the text field, the toolkit's (c-e-a-theme-can-shape-the-controls)
    (
        'the search box is drawn the same wherever the pointer is',
        '                hovered: self.search_hovered,',
        '                hovered: false,',
        ['the_search_box_and_the_text_field_are_the_toolkits'],
    ),
    (
        'the pointer over the search box is not noticed',
        '        let search = sidebar && Self::search_rect().contains(mx, my);',
        '        let search = false;',
        ['the_search_box_and_the_text_field_are_the_toolkits'],
    ),
    (
        'the pointer over a page control is not noticed',
        '            self.row_at(mx, my)\n',
        '            None\n',
        ['the_search_box_and_the_text_field_are_the_toolkits'],
    ),
    (
        'the text field is drawn the same wherever the pointer is',
        '                self.page_hovered == Some(RowHit::Focus(FieldId::ExclusionDraft)),',
        '                false,',
        ['the_search_box_and_the_text_field_are_the_toolkits'],
    ),
    (
        'the search box takes the toolkit\'s focus width, not the user\'s',
        '            },\n            self.appearance.settings.focus_ring_width(),\n',
        '            },\n            guitk::style::FOCUS_RING_WIDTH,\n',
        ['the_search_box_and_the_text_field_are_the_toolkits'],
    ),
    (
        'the text field takes the toolkit\'s focus width, not the user\'s',
        '                self.page_hovered == Some(RowHit::Focus(FieldId::ExclusionDraft)),\n                self.appearance.settings.focus_ring_width(),',
        '                self.page_hovered == Some(RowHit::Focus(FieldId::ExclusionDraft)),\n                guitk::style::FOCUS_RING_WIDTH,',
        ['the_search_box_and_the_text_field_are_the_toolkits'],
    ),
    (
        'leaving a control lit is not a change',
        '        let changed = category != self.sidebar_hovered\n',
        '        let changed = category.is_some() && category != self.sidebar_hovered\n',
        ['leaving_a_category_for_the_page_puts_its_light_out'],
    ),
    (
        'the pointer leaving the window leaves the search box lit',
        '                let search = std::mem::take(&mut self.search_hovered);',
        '                let search = false;',
        ['the_search_box_and_the_text_field_are_the_toolkits'],
    ),
]

CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

MUTATIONS += [
    # -- The list of keys takes the pointer (2026-10-04; known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        "a press goes through the list of keys",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n"
        "                    return EventResult::Consumed;\n"
        "                }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the list of keys away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        [CARD],
    ),
    (
        "a press under the list of keys leaves it up",
        "                    self.show_help = false;\n"
        "                    return EventResult::Consumed;\n",
        "                    return EventResult::Consumed;\n",
        [CARD],
    ),
    (
        "the wheel scrolls a dropdown under the list of keys",
        "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        [CARD],
    ),
]

# -- the Recycle Bin page (design-decisions §1238, §1240) --
MUTATIONS += [
    (
        "the bins are not read when the page is opened",
        "            self.bin_error = None;\n            self.refresh_bins();\n",
        "            self.bin_error = None;\n",
        [BINS_LISTED],
    ),
    (
        "the default chosen is not kept",
        "                        .bin_default\n                        .store_as_user_default()\n",
        "                        .bin_default\n                        .max_age\n"
        "                        .map_or(Ok::<(), std::io::Error>(()), |_| Ok(()))\n",
        [BINS_DEFAULT],
    ),
    (
        "turning a drive's own limits on writes nothing",
        "            Ok(None) => bin.set_limits(Some(&self.bin_default)),",
        "            Ok(None) => Ok(()),",
        [BINS_OWN],
    ),
    (
        "turning a drive's own limits off leaves them",
        "            Ok(Some(_)) => bin.set_limits(None),",
        "            Ok(Some(_)) => Ok(()),",
        [BINS_OWN],
    ),
    (
        "a drive's own limit is not kept",
        "            .set_limits(Some(&own))\n",
        "            .set_limits(Some(&self.bin_default))\n",
        [BINS_OWN],
    ),
    (
        "the switch is offered over limits nobody can read",
        "            match &row.own {\n                Ok(own) => {",
        "            match &row.own.clone().or(Ok::<Option<recyclebin::Limits>, String>(None)) {\n                Ok(own) => {",
        [BINS_UNREAD],
    ),
    (
        "the switch is a plain toggle",
        "            RowHit::Toggle(ToggleId::BinOwnLimits(index)) => self.set_bin_own_limits(index),\n",
        "",
        [BINS_OWN],
    ),
]

# -- how long notifications are kept (2026-10-10; lane C,
# c-e-a-setting-for-how-long-notifications-are-kept): a choice made only by
# editing notifications.yaml by hand.
HISTORY = "how_long_notifications_are_kept_is_chosen_and_written"
HISTORY_OFF_LIST = "a_history_length_not_on_the_list_is_shown_as_itself"

MUTATIONS += [
    (
        "the history row opens another dropdown",
        '            "Keep for",\n            DropdownId::NotifHistory,',
        '            "Keep for",\n            DropdownId::NotifImportance(usize::MAX),',
        [HISTORY],
    ),
    (
        "a length chosen is not kept",
        "                    self.notif.settings.history.days = *days;",
        "                    let _ = days;",
        [HISTORY],
    ),
    (
        "a length written by hand snaps to the list",
        "    if !days.contains(&current) {\n        let at = days.iter().position(|d| *d > current).unwrap_or(days.len());",
        "    if false {\n        let at = days.iter().position(|d| *d > current).unwrap_or(days.len());",
        [HISTORY_OFF_LIST],
    ),
    (
        "a length written by hand goes at the end of the list",
        "        let at = days.iter().position(|d| *d > current).unwrap_or(days.len());",
        "        let at = days.len();",
        [HISTORY_OFF_LIST],
    ),
    (
        "a week reads as seven days",
        '        7 => String::from("1 week"),',
        '        7 => String::from("7 days"),',
        [HISTORY],
    ),
    (
        "nothing kept reads as zero days",
        '        0 => String::from("Don\'t keep"),',
        '        0 => String::from("0 days"),',
        [HISTORY],
    ),
    (
        "the list opens on the first length, not the one kept",
        "                let at = choices.iter().position(|d| *d == current).unwrap_or(0);\n                (choices.into_iter().map(history_label).collect(), at)",
        "                let _ = current;\n                (choices.into_iter().map(history_label).collect(), 0)",
        [HISTORY, HISTORY_OFF_LIST],
    ),
]

RECYCLEBINS = [
    (
        "a limit written by hand is not offered",
        "        values.insert(at, current);\n        (values, at)",
        "        let _ = (at, current);\n        (values, 0)",
        [BINS_BY_HAND],
    ),
    (
        "a limit written by hand is offered in the wrong place",
        ".position(|v| v.is_some_and(|v| current.is_some_and(|c| v > c)))",
        ".position(|v| v.is_some_and(|v| current.is_some_and(|c| v < c)))",
        [BINS_BY_HAND],
    ),
    (
        "a week reads in days",
        '                7 => "1 week".to_string(),',
        "",
        [BINS_DEFAULT],
    ),
    (
        "a size in gigabytes reads in megabytes",
        "                if megabytes >= 1024 && megabytes % 1024 == 0 {",
        "                if false {",
        [BINS_BY_HAND],
    ),
    (
        "a number of items is not set",
        "            Self::Count => limits.max_items = value.and_then(|n| u32::try_from(n).ok()),",
        "            Self::Count => {}",
        [BINS_OWN],
    ),
]

TABLES = {
    "main.rs": MUTATIONS,
    "recyclebins.rs": RECYCLEBINS,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "settings", timeout=900, only=mine))
    raise SystemExit(worst)
