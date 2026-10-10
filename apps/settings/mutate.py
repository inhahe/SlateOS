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

And, from 2026-10-10, why a disabled control is disabled, said while the
pointer rests on it (lane C, requests/c-e-say-why-a-control-is-disabled.md).

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
        "                    Press::Does(RowHit::Press(ButtonId::EventColour(event.id))),",
        '                    Press::Cannot("Hidden."),',
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
        '            WallpaperSource::TimeOfDay => self.build_wallpaper_schedule(s),',
        '            WallpaperSource::TimeOfDay => self.build_wallpaper_plain(s),',
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
        '            s.note(&format!("Up now: {up}."), 28.0);',
        '            let _ = up;',
        ['a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page'],
    ),
    (
        "the picture up now goes by UTC, not this machine's zone",
        '.scheduled_wallpaper_at(utc_secs, self.datetime.settings.rule(self.system_zone))',
        '.scheduled_wallpaper_at(utc_secs, datetimesettings::Tz::utc())',
        ['the_picture_up_now_is_the_one_for_this_machines_time_of_day'],
    ),
    # No rows for the notes saying a source is hidden under a schedule: since
    # design-decisions §1243 a source the desktop does not show is not on the
    # page at all, which only_the_source_the_desktop_shows_has_its_controls_on_
    # the_page holds and the WallpaperSource rows below sweep.
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

# -- a theme's other axes, each chosen in a list of its own: the controls and
# the motion (2026-10-06; lane C, c-e-a-theme-can-shape-the-controls and
# c-e-a-theme-can-set-the-motion -- written without rows then, so given them
# here), and windows' frames and the taskbar panel (2026-10-10; lane C,
# c-e-choose-the-window-frames-in-settings and
# c-e-choose-the-taskbar-panel-in-settings). The four are built alike, so
# every one gets the same eight rows: its row, the name the row shows, the
# choice kept, a theme with nothing for the axis refused, such a theme listed
# as such, an unreadable one listed as unreadable, the list opening on the
# choice, and the chosen theme's problem said.
AXES_APART = "controls_and_motion_are_chosen_apart_from_the_colours"
FRAMES_APART = "window_frames_and_the_taskbar_panel_are_chosen_apart"
AXIS_ROWS = "each_theme_axis_row_opens_its_own_list"
UNREADABLE = "a_theme_that_cannot_be_read_says_why_in_every_list"
CANNOT_SERVE = "a_theme_chosen_for_an_axis_it_cannot_serve_says_why"
FRAMES_WRITTEN = "window_frames_and_a_taskbar_panel_chosen_are_written"
SOURCES_SHOWN = "only_the_source_the_desktop_shows_has_its_controls_on_the_page"
SOURCES_CLEARED = "choosing_a_source_clears_those_above_it_and_keeps_those_below"
SOURCE_PENDING = "a_source_chosen_and_not_set_up_is_shown_until_it_is"
EACH_CLEAR = "each_clear_empties_its_source_and_stays_on_it"
SHOW_LIST = "the_show_list_names_every_source_and_opens_on_the_one_shown"
THEME_CHOSEN = "a_theme_is_chosen_by_its_picture"
BUNDLED = "the_pictures_themes_bring_are_offered_as_pictures"
THEME_PROBLEM = "a_theme_whose_pictures_cannot_be_used_says_why"
NO_THEMES = "with_no_theme_that_brings_pictures_the_page_says_where_they_come_from"
FRAMES_AND_WAKES = "a_picture_decoded_is_drawn_and_a_wake_asks_for_a_frame"


def axis_rows(what, label, variant, field, provides, without, apart, extra=()):
    """The eight rows every theme axis gets; `extra` are tests a choice made
    through the page also breaks (the frames and panel are saved by one)."""
    loader = variant
    return [
        (
            f"the {what} row opens another list",
            f'"{label}", DropdownId::{variant}, &',
            f'"{label}", DropdownId::IconTheme, &',
            [AXIS_ROWS, *extra],
        ),
        (
            f"the {what} row names the colour theme",
            f"_name = self.theme_name(self.appearance.settings.{field}.id());",
            "_name = self.theme_name(self.appearance.settings.color_theme.id());",
            [apart],
        ),
        (
            f"a {what} theme chosen is not kept",
            f"                    self.appearance.settings.{field} =\n"
            f"                        appearance::themes::{loader}::load_from(&self.theme_dirs, &info.id);",
            "                    let _ = &info.id;",
            [apart, *extra],
        ),
        (
            f"a theme with no {what} can be chosen for it",
            f"                    && info.{provides}()\n",
            "                    && !info.name.is_empty()\n",
            [apart, UNREADABLE],
        ),
        (
            f"a theme with no {what} is listed as having it",
            f"        if info.{provides}() {{\n            info.name.clone()",
            "        if info.problem.is_none() {\n            info.name.clone()",
            [apart],
        ),
        (
            f"an unreadable theme is listed as having no {what}",
            '            format!("{} -- cannot be used: it {problem}", info.name)\n'
            f'        }} else {{\n            format!("{{}} -- {without}", info.name)',
            "            let _ = problem;\n"
            f'            format!("{{}} -- {without}", info.name)\n'
            f'        }} else {{\n            format!("{{}} -- {without}", info.name)',
            [UNREADABLE],
        ),
        (
            f"the {what} list opens on the colour theme",
            f"t.id.as_os_str() == self.appearance.settings.{field}.id()",
            "t.id.as_os_str() == self.appearance.settings.color_theme.id()",
            [apart],
        ),
    ]


def problem_row(what, field):
    """A chosen theme's problem said under the axis's row."""
    return (
        f"a {what} theme that cannot be used says nothing",
        f"        if let Some(problem) = self.appearance.settings.{field}.problem() {{\n"
        "            s.note(problem, 28.0);",
        f"        if let Some(problem) = self.appearance.settings.{field}.problem() {{\n"
        "            let _ = problem;",
        [CANNOT_SERVE],
    )


MUTATIONS += [
    *axis_rows(
        "controls",
        "Controls",
        "WidgetTheme",
        "widget_theme",
        "provides_widget_style",
        "no control shapes",
        AXES_APART,
    ),
    problem_row("controls", "widget_theme"),
    *axis_rows(
        "motion",
        "Motion",
        "AnimationTheme",
        "animation_theme",
        "provides_animation",
        "no motion",
        AXES_APART,
    ),
    *axis_rows(
        "window frames",
        "Window frames",
        "DecorationTheme",
        "decoration_theme",
        "provides_decorations",
        "no window frames",
        FRAMES_APART,
        extra=(FRAMES_WRITTEN,),
    ),
    problem_row("window frames", "decoration_theme"),
    *axis_rows(
        "taskbar panel",
        "Taskbar panel",
        "PanelTheme",
        "panel_theme",
        "provides_panel",
        "no taskbar panel",
        FRAMES_APART,
        extra=(FRAMES_WRITTEN,),
    ),
    problem_row("taskbar panel", "panel_theme"),
    # The motion's notes: what it is, then the theme's problem.
    (
        "the motion is not described",
        "        for note in self.animation_theme_notes() {\n            s.note(&note, 28.0);",
        "        for note in self.animation_theme_notes() {\n            let _ = note;",
        [AXES_APART, CANNOT_SERVE],
    ),
    (
        "a motion theme that cannot be used says nothing",
        "        if let Some(problem) = theme.problem() {\n            notes.push(problem.to_string());",
        "        if let Some(problem) = theme.problem() {\n            let _ = problem;",
        [CANNOT_SERVE],
    ),
    (
        "nothing moving is described as moving",
        "    let mut notes = vec![if motion.is_still() {",
        "    let mut notes = vec![if false {",
        [AXES_APART],
    ),
    (
        "springing is described as gliding",
        'guitk::motion::Curve::Spring => "Springs a little past its place and back",',
        'guitk::motion::Curve::Spring => "Glides in and settles",',
        [AXES_APART],
    ),
    (
        "an even pace is described as gliding",
        'guitk::motion::Curve::Linear => "Moves at an even pace",',
        'guitk::motion::Curve::Linear => "Glides in and settles",',
        [AXES_APART],
    ),
    (
        "gliding is described as an even pace",
        'guitk::motion::Curve::EaseOut => "Glides in and settles",',
        'guitk::motion::Curve::EaseOut => "Moves at an even pace",',
        [AXES_APART],
    ),
    (
        "every motion takes the built-in time",
        '"{how}, {} ms a move at Normal speed.", motion.standard_ms())',
        '"{how}, {} ms a move at Normal speed.", 200)',
        [AXES_APART],
    ),
    # The cursor theme (2026-10-10; lane C,
    # c-e-choose-the-cursor-theme-in-settings): every XCursor theme installed,
    # other desktops' included.
    (
        "the cursors row opens another list",
        '"Cursors", DropdownId::CursorTheme, &',
        '"Cursors", DropdownId::IconTheme, &',
        [AXIS_ROWS, "a_cursor_theme_chosen_is_written"],
    ),
    (
        "the cursors row names the colour theme",
        "        let id = self.appearance.settings.cursor_theme.id();\n"
        "        self.cursor_themes",
        "        let id = self.appearance.settings.color_theme.id();\n"
        "        self.cursor_themes",
        ["the_cursor_theme_is_chosen_from_every_installed_one"],
    ),
    (
        "a cursor theme chosen is not kept",
        "                    self.appearance.settings.cursor_theme =\n"
        "                        appearance::cursors::CursorTheme::load(&info.id);",
        "                    let _ = &info.id;",
        [
            "the_cursor_theme_is_chosen_from_every_installed_one",
            "a_cursor_theme_chosen_is_written",
        ],
    ),
    (
        "the cursor list opens on the built-in pointer",
        "t.id.as_os_str() == self.appearance.settings.cursor_theme.id())",
        "t.id.as_os_str() == appearance::cursors::CursorTheme::built_in().id())",
        ["the_cursor_theme_is_chosen_from_every_installed_one"],
    ),
    (
        "other desktops' cursor themes are not looked for",
        "            .chain(self.cursor_icon_dirs.iter().cloned())\n",
        "",
        [
            "the_cursor_theme_is_chosen_from_every_installed_one",
            "a_cursor_theme_chosen_is_written",
        ],
    ),
    (
        "the theme folders' own cursor themes are not looked for",
        "            .chain(std::iter::once(self.theme_dirs.system.clone()))\n"
        "            .chain(self.cursor_icon_dirs.iter().cloned())\n"
        "            .collect();",
        "            .chain(std::iter::once(self.theme_dirs.system.clone()))\n"
        "            .chain(self.cursor_icon_dirs.iter().cloned())\n"
        "            .skip(1)\n"
        "            .collect();",
        ["the_cursor_theme_is_chosen_from_every_installed_one"],
    ),
    (
        "a cursor theme not installed is not said",
        "        if let Some(note) = self.cursor_theme_note() {\n            s.note(&note, 28.0);",
        "        if let Some(note) = self.cursor_theme_note() {\n            let _ = note;",
        ["a_cursor_theme_not_installed_says_so"],
    ),
    (
        "every cursor theme is said not to be installed",
        "        (!theme.is_built_in() && !self.cursor_themes.iter().any(|t| t.id.as_os_str() == id))",
        "        (!theme.is_built_in())",
        ["the_cursor_theme_is_chosen_from_every_installed_one"],
    ),
    (
        "the built-in pointer is said not to be installed",
        "        (!theme.is_built_in() && !self.cursor_themes.iter().any(|t| t.id.as_os_str() == id))",
        "        (!self.cursor_themes.iter().skip(1).any(|t| t.id.as_os_str() == id))",
        ["a_cursor_theme_not_installed_says_so"],
    ),
    # The Background page's one choice of source, and themes by their
    # pictures (2026-10-10; design-decisions §1243, lane C's
    # c-e-a-themes-wallpapers-on-the-background-page).
    (
        "a schedule is never what the desktop shows",
        "        if !settings.wallpaper_schedule.is_empty() {\n            Self::TimeOfDay",
        "        if false {\n            Self::TimeOfDay",
        [SOURCES_SHOWN, SOURCES_CLEARED],
    ),
    (
        "a folder is never what the desktop shows",
        "        } else if settings.wallpaper_folder.is_some() {\n            Self::Folder",
        "        } else if false {\n            Self::Folder",
        [SOURCES_SHOWN, SOURCES_CLEARED],
    ),
    (
        "a theme is never what the desktop shows",
        "        } else if !settings.wallpaper_theme.is_built_in() {\n            Self::Theme",
        "        } else if false {\n            Self::Theme",
        [SOURCES_CLEARED, THEME_PROBLEM],
    ),
    (
        "a picture is never what the desktop shows",
        "        } else if settings.wallpaper.is_some() {\n            Self::Picture",
        "        } else if false {\n            Self::Picture",
        [SOURCES_SHOWN, SOURCES_CLEARED],
    ),
    (
        "choosing a source leaves the schedule above it",
        "        if source.rank() < WallpaperSource::TimeOfDay.rank() {\n"
        "            s.wallpaper_schedule.clear();\n"
        "        }\n",
        "",
        [SOURCES_CLEARED],
    ),
    (
        "choosing the folder clears the folder",
        "        if source.rank() < WallpaperSource::Folder.rank() {",
        "        if source.rank() <= WallpaperSource::Folder.rank() {",
        [SOURCES_CLEARED],
    ),
    (
        "choosing a theme clears the theme",
        "        if source.rank() < WallpaperSource::Theme.rank() {",
        "        if source.rank() <= WallpaperSource::Theme.rank() {",
        [SOURCES_CLEARED],
    ),
    (
        "choosing a picture clears the picture",
        "        if source.rank() < WallpaperSource::Picture.rank() {",
        "        if source.rank() <= WallpaperSource::Picture.rank() {",
        [SOURCES_CLEARED],
    ),
    (
        "a source chosen and not set up is not shown",
        "            Some(pending) if pending.rank() > shown.rank() => pending,",
        "            Some(pending) if false => pending,",
        [SOURCE_PENDING, "a_morning_and_an_evening_picture_are_chosen_on_the_wallpaper_page"],
    ),
    (
        "a source chosen is shown over one set above it since",
        "            Some(pending) if pending.rank() > shown.rank() => pending,",
        "            Some(pending) if pending.rank() > 0 => pending,",
        [SOURCE_PENDING],
    ),
    (
        "a source chosen and never set up outlives the page",
        "            // the desktop shows; the page opens on what it does.\n"
        "            self.wallpaper_source_pending = None;",
        "            // the desktop shows; the page opens on what it does.",
        [SOURCE_PENDING],
    ),
    (
        "the schedule's Clear leaves its source",
        "                self.wallpaper_source_pending = Some(WallpaperSource::TimeOfDay);",
        "",
        [EACH_CLEAR],
    ),
    (
        "the folder's Clear leaves its source",
        "                self.wallpaper_source_pending = Some(WallpaperSource::Folder);",
        "",
        [EACH_CLEAR],
    ),
    (
        "the picture's Remove leaves its source",
        "                self.wallpaper_source_pending = Some(WallpaperSource::Picture);",
        "",
        [EACH_CLEAR],
    ),
    (
        "the Show list opens on the first source",
        "                    .position(|source| *source == shown)",
        "                    .position(|source| *source == WallpaperSource::Picture)",
        [SHOW_LIST],
    ),
    (
        "choosing in the Show list does nothing",
        "                    self.choose_wallpaper_source(*source);",
        "                    let _ = source;",
        [SOURCES_CLEARED, SOURCE_PENDING],
    ),
    (
        "the fit is offered with nothing to place",
        "            && WallpaperSource::shown(&self.appearance.settings) == source\n",
        "",
        [SOURCE_PENDING],
    ),
    (
        "the theme source draws no themes",
        "            WallpaperSource::Theme => self.build_wallpaper_themes(s),",
        "            WallpaperSource::Theme => self.build_wallpaper_plain(s),",
        [THEME_CHOSEN, THEME_PROBLEM, NO_THEMES],
    ),
    (
        "a theme with pictures is not offered",
        "            if info.provides_wallpapers() {",
        "            if false {",
        [THEME_CHOSEN],
    ),
    (
        "a theme is shown by its other mode's picture",
        "                        .picture(light)",
        "                        .picture(!light)",
        [THEME_CHOSEN],
    ),
    (
        "the pictures themes bring are not offered",
        "            for picture in &info.wallpapers {",
        "            for picture in info.wallpapers.iter().take(0) {",
        [BUNDLED],
    ),
    (
        "the pictures are not asked for",
        "                self.thumbs.ask(picture);",
        "                let _ = picture;",
        [THEME_CHOSEN, FRAMES_AND_WAKES],
    ),
    (
        "a theme's card chooses nothing",
        "                        appearance::themes::WallpaperTheme::load_from(&self.theme_dirs, &id);",
        "                        appearance::themes::WallpaperTheme::built_in();\n                    let _ = &id;",
        [THEME_CHOSEN],
    ),
    (
        "a bundled picture's card chooses nothing",
        "                    self.appearance.settings.wallpaper = Some(picture);",
        "                    let _ = picture;",
        [BUNDLED],
    ),
    (
        "the picture source offers no bundled pictures",
        "        if !self.bundled_picture_cards.is_empty() {",
        "        if false {",
        [BUNDLED],
    ),
    (
        "every card is the first card",
        "                RowHit::Select(select, idx),",
        "                RowHit::Select(select, 0),",
        [BUNDLED],
    ),
    (
        "the theme chosen is not marked",
        "card.theme.as_deref() == Some(chosen)",
        "card.theme.as_deref() == Some(chosen) && false",
        [THEME_CHOSEN],
    ),
    (
        "a card draws another picture",
        "                image_id: id,",
        "                image_id: id ^ 1,",
        [THEME_CHOSEN],
    ),
    (
        "a theme that cannot be used says nothing",
        "        if let Some(problem) = settings.wallpaper_theme.problem() {\n            s.note(problem, 28.0);",
        "        if let Some(problem) = settings.wallpaper_theme.problem() {\n            let _ = problem;",
        [THEME_PROBLEM],
    ),
    (
        "no uploads reach the window",
        "        self.thumbs.take_uploads()\n",
        "        Vec::new()\n",
        [THEME_CHOSEN, FRAMES_AND_WAKES],
    ),
    (
        "a frame does not collect decoded pictures",
        "        self.thumbs.collect();\n        self.render_tree()",
        "        self.render_tree()",
        [FRAMES_AND_WAKES],
    ),
    (
        "a wake does not collect decoded pictures",
        "        if self.thumbs.collect() {",
        "        if false {",
        [FRAMES_AND_WAKES],
    ),
    (
        "the window asks for no waker",
        "    fn wants_waker(&self) -> bool {\n        true",
        "    fn wants_waker(&self) -> bool {\n        false",
        [FRAMES_AND_WAKES],
    ),
    # The colour list's own unreadable theme.
    (
        "an unreadable theme is listed as an icon pack",
        '            format!("{} -- cannot be used: it {problem}", info.name)\n'
        "        } else if info.provides_icons() {",
        "            let _ = problem;\n"
        '            format!("{} -- icons only", info.name)\n'
        "        } else if info.provides_icons() {",
        [UNREADABLE],
    ),
]


# -- why a disabled control is disabled, said while the pointer rests on it
# (2026-10-10; lane C, requests/c-e-say-why-a-control-is-disabled.md): every
# dimmed button and every row that cannot be used yet gives a reason, which
# appears after the toolkit's delay, goes with the pointer, and is drawn over
# everything -- and is never started under a list, a picker, the card or a
# drag.
WHY_BUTTONS = "every_dimmed_button_says_why_when_the_pointer_rests_on_it"
WHY_ROWS = "a_row_that_cannot_be_used_says_why"
WHY_GOES = "the_reason_goes_when_the_pointer_leaves_the_control"
WHY_LIVE = "a_control_that_can_be_used_explains_nothing"
WHY_CARD = "nothing_is_explained_under_the_shortcut_card"
WHY_COVERS = "nothing_is_explained_under_a_list_a_picker_or_a_drag"

MUTATIONS += [
    (
        "a dimmed button gives no reason",
        "                self.disabled_rect(x + dx, y + dy, button_width(label), BUTTON_HEIGHT, why);",
        "                let _ = why;",
        [WHY_BUTTONS, WHY_GOES, WHY_CARD, WHY_COVERS],
    ),
    (
        "a dimmed button's reason is kept for another place",
        "                self.disabled_rect(x + dx, y + dy, button_width(label), BUTTON_HEIGHT, why);",
        "                self.disabled_rect(x, y, button_width(label), BUTTON_HEIGHT, why);",
        [WHY_BUTTONS],
    ),
    (
        "a button that can be pressed gives a reason",
        "                self.hit_rect(x + dx, y + dy, button_width(label), BUTTON_HEIGHT, what);",
        "                self.hit_rect(x + dx, y + dy, button_width(label), BUTTON_HEIGHT, what);\n"
        '                self.disabled_rect(x + dx, y + dy, button_width(label), BUTTON_HEIGHT, "Live.");',
        [WHY_LIVE],
    ),
    (
        "a row that cannot be used gives no reason",
        "        let text = value.to_string();\n        let (x, y) = (self.x(), self.y());\n        self.disabled_rect(x - ROW_HIT_INSET, y, ROW_HIT_WIDTH, ITEM_HEIGHT, why);",
        "        let text = value.to_string();\n        let (x, y) = (self.x(), self.y());\n        let _ = (x, y, why);",
        [WHY_ROWS],
    ),
    (
        "the page's reasons are not kept",
        "        self.disabled.push(((x, y, w, h), why.to_owned()));",
        "        let _ = (x, y, w, h, why);",
        [WHY_BUTTONS, WHY_ROWS],
    ),
    (
        "the reasons are asked of the page from another place",
        "        let mut sink = WhySink {\n            x: Self::content_x(),\n            y: self.page_top(),",
        "        let mut sink = WhySink {\n            x: Self::content_x(),\n            y: 0.0,",
        [WHY_BUTTONS, WHY_ROWS],
    ),
    (
        "the pointer is not followed",
        "            self.pointer = match mouse.kind {\n                MouseEventKind::Leave => None,\n                _ => Some((mouse.x, mouse.y)),\n            };",
        "            let _ = mouse;",
        [WHY_BUTTONS, WHY_ROWS],
    ),
    (
        "a pointer that left is still followed",
        "                MouseEventKind::Leave => None,\n                _ => Some((mouse.x, mouse.y)),",
        "                MouseEventKind::Leave => self.pointer,\n                _ => Some((mouse.x, mouse.y)),",
        [WHY_GOES],
    ),
    (
        "a tick does not move the clock",
        "            self.clock_ms = self.clock_ms.saturating_add(*elapsed_ms);",
        "            let _ = elapsed_ms;",
        [WHY_BUTTONS, WHY_ROWS],
    ),
    (
        "the tick that shows a reason asks for no frame",
        "            if self.why_disabled.tick(self.clock_ms) {\n                result = EventResult::Consumed;\n            }",
        "            self.why_disabled.tick(self.clock_ms);",
        [WHY_BUTTONS],
    ),
    (
        "a reason that goes asks for no frame",
        "        if self.explain_disabled() {\n            result = EventResult::Consumed;\n        }",
        "        self.explain_disabled();",
        [WHY_GOES],
    ),
    (
        "the window asks for no tick",
        "            .due_in(self.clock_ms)\n            .map(std::time::Duration::from_millis)",
        "            .due_in(self.clock_ms)\n            .filter(|_| false)\n            .map(std::time::Duration::from_millis)",
        [WHY_BUTTONS],
    ),
    (
        "the reason is not drawn",
        "        tree.commands.extend(self.why_disabled.render(pal));",
        "        let _ = self.why_disabled.render(pal);",
        [WHY_BUTTONS],
    ),
    (
        "a reason is given under the file picker",
        "        let covered = self.dialog.is_some()\n            || self.color_dialog.is_some()",
        "        let covered = self.color_dialog.is_some()",
        [WHY_COVERS],
    ),
    (
        "a reason is given under the colour picker",
        "            || self.color_dialog.is_some()\n            || self.show_help",
        "            || self.show_help",
        [WHY_COVERS],
    ),
    (
        "a reason is given under the shortcut card",
        "            || self.show_help\n            || self.open_dropdown.is_some()",
        "            || self.open_dropdown.is_some()",
        [WHY_CARD],
    ),
    (
        "a reason is given under a list",
        "            || self.open_dropdown.is_some()\n            || self.dragging.is_some();",
        "            || self.dragging.is_some();",
        [WHY_COVERS],
    ),
    (
        "a reason is given during a drag",
        "            || self.open_dropdown.is_some()\n            || self.dragging.is_some();",
        "            || self.open_dropdown.is_some();",
        [WHY_COVERS],
    ),
]


# Notes wrap to their column and take the room their lines need
# (2026-10-10): a note was one line however long, cut off by the window.
MUTATIONS += [
    (
        'a note is not wrapped',
        '        let lines = text::wrap(text, NOTE_WIDTH, NOTE_SIZE, FontWeightHint::Regular);',
        '        let lines = vec![text.to_owned()];',
        ['a_note_too_long_for_its_column_wraps_and_takes_the_room_its_lines_need'],
    ),
    (
        'a wrapped note takes only the room asked',
        '        self.advance(height.max(needed));',
        '        self.advance(height);',
        ['a_note_too_long_for_its_column_wraps_and_takes_the_room_its_lines_need'],
    ),
    (
        'a note shrinks below the room asked',
        '        self.advance(height.max(needed));',
        '        self.advance(needed);',
        ['a_note_too_long_for_its_column_wraps_and_takes_the_room_its_lines_need'],
    ),
    (
        "a note's lines are drawn on one another",
        '                line_y += NOTE_LINE_HEIGHT;',
        '                line_y += 0.0;',
        ['a_note_too_long_for_its_column_wraps_and_takes_the_room_its_lines_need'],
    ),
]


# The System Sounds section (2026-10-10, requests/c-e-a-sounds-page-for-the-
# sounds-axis.md): on or off, a volume, a sound theme and each event's sound,
# written to appearance.yaml, and each heard as it is chosen.
#
# Not rows: whether a preview is *played* (`play_previews`, set by `main`) is
# unobservable here -- on this host `sound::play` answers "no device" at once,
# and a test plays nothing aloud by design -- so the gate and `main`'s line
# setting it have no test that could fail.
SOUNDS_CHOSEN = "the_system_sounds_are_chosen_on_the_sound_page_and_reach_the_file"
SOUNDS_OFF = "with_system_sounds_off_the_rest_of_the_section_is_not_offered"
SOUNDS_HEARD = "a_sound_is_heard_as_it_is_chosen"
SLIDER_DRAG = "test_a_slider_drag_follows_the_pointer_past_the_track"

MUTATIONS += [
    (
        "the sound themes are not read on entering the Sound page",
        "            self.refresh_sound_themes();\n",
        "",
        [SOUNDS_CHOSEN],
    ),
    (
        "the page lists the machine's sound themes, not where it looks",
        "            Some(roots) => appearance::sounds::available_in(roots),",
        "            Some(_roots) => appearance::sounds::available(),",
        [SOUNDS_CHOSEN],
    ),
    (
        "a theme chosen is looked for in the standard places, not where it was listed",
        "            Some(roots) => {\n                appearance::sounds::SoundTheme::named(id, self.theme_dirs.clone(), roots.clone())\n            }",
        "            Some(_roots) => appearance::sounds::SoundTheme::load(id),",
        [SOUNDS_HEARD],
    ),
    (
        "an event's list opens on the theme's when it is Off",
        "            Some(appearance::EventSound::Off) => 1,",
        "            Some(appearance::EventSound::Off) => 0,",
        [SOUNDS_CHOSEN],
    ),
    (
        "an event's list opens on the theme's when it is the user's own",
        "            Some(appearance::EventSound::File(_)) => 2,",
        "            Some(appearance::EventSound::File(_)) => 0,",
        [SOUNDS_CHOSEN],
    ),
    (
        "an event that is Off is shown as the theme's",
        "            Some(appearance::EventSound::Off) => EVENT_SOUND_CHOICES[1].to_owned(),",
        "            Some(appearance::EventSound::Off) => EVENT_SOUND_CHOICES[0].to_owned(),",
        [SOUNDS_CHOSEN],
    ),
    (
        "an event's row does not say the file is the user's own",
        '                format!("Your own: {file}")',
        '                format!("{file}")',
        [SOUNDS_CHOSEN],
    ),
    (
        "the theme's, chosen again, keeps the user's choice",
        "                events.remove(event.name);\n",
        "",
        [SOUNDS_CHOSEN],
    ),
    (
        "Off is not kept",
        "                events.insert(event.name.to_owned(), appearance::EventSound::Off);\n",
        "",
        [SOUNDS_CHOSEN],
    ),
    (
        "a file of my own puts no picker up",
        "            2 => self.open_sound_dialog(index),",
        "            2 => {}",
        [SOUNDS_CHOSEN],
    ),
    (
        "the sound picker's answer goes to another errand",
        "        self.picker_is_for = PickerPurpose::EventSound(index);",
        "        self.picker_is_for = PickerPurpose::LoginImage;",
        [SOUNDS_CHOSEN],
    ),
    (
        "the picker's file goes to the first event",
        "                        if let Some(event) = appearance::sounds::SHELL_EVENTS.get(index) {\n                            self.appearance",
        "                        if let Some(event) = appearance::sounds::SHELL_EVENTS.get(index.min(0)) {\n                            self.appearance",
        [SOUNDS_CHOSEN],
    ),
    (
        "the picker's file is not kept",
        ".insert(event.name.to_owned(), appearance::EventSound::File(path));",
        ".insert(event.name.to_owned(), appearance::EventSound::Off);",
        [SOUNDS_CHOSEN],
    ),
    (
        "the volume is drawn at nought",
        "            SliderId::SoundVolume => self.appearance.settings.sounds.volume,",
        "            SliderId::SoundVolume => 0.0,",
        [SOUNDS_CHOSEN],
    ),
    (
        "the volume is not kept",
        "            SliderId::SoundVolume => self.appearance.settings.sounds.volume = value,",
        "            SliderId::SoundVolume => {}",
        [SOUNDS_CHOSEN],
    ),
    (
        "the volume has no readout",
        '            Self::SoundVolume => Some(format!("{}%", round_u16(value * 100.0))),',
        "            Self::SoundVolume => None,",
        [SOUNDS_CHOSEN],
    ),
    (
        "the sounds' switch flips nothing",
        "            ToggleId::SystemSounds => &mut self.appearance.settings.sounds.enabled,",
        "            ToggleId::SystemSounds => return None,",
        [SOUNDS_CHOSEN],
    ),
    (
        "the rest of the section is offered with the sounds off",
        "        if !sounds.enabled {\n            s.gap();\n            return;\n        }\n",
        "",
        [SOUNDS_OFF],
    ),
    (
        "the section has no volume",
        '        self.slider(s, "Volume", SliderId::SoundVolume);\n',
        "",
        [SOUNDS_OFF, SOUNDS_CHOSEN],
    ),
    (
        "the theme's row does not name it",
        '                |t| t.name.clone(),\n            );\n        s.dropdown_row("Sound theme"',
        '                |_t| String::new(),\n            );\n        s.dropdown_row("Sound theme"',
        [SOUNDS_CHOSEN],
    ),
    (
        "every event's row is the first event's",
        "                DropdownId::EventSound(index),\n                &self.event_sound_shown(index),",
        "                DropdownId::EventSound(0),\n                &self.event_sound_shown(index),",
        [SOUNDS_OFF],
    ),
    (
        "the theme list opens on the first theme",
        "                    .position(|t| t.id.as_os_str() == current)\n                    .unwrap_or(0);",
        "                    .position(|_t| current.is_empty())\n                    .unwrap_or(0);",
        [SOUNDS_CHOSEN],
    ),
    (
        "an event's list opens on the first choice",
        "                (items, self.event_sound_choice(index))",
        "                (items, index.min(0))",
        [SOUNDS_CHOSEN],
    ),
    (
        "a theme chosen is not kept",
        "                    self.appearance.settings.sound_theme = self.sound_theme_named(&id);\n",
        "",
        [SOUNDS_CHOSEN],
    ),
    (
        "an event's choice does nothing",
        "            DropdownId::EventSound(event) => self.choose_event_sound(event, index),",
        "            DropdownId::EventSound(_event) => {}",
        [SOUNDS_CHOSEN],
    ),
    # The previews.
    (
        "the theme's sound, chosen again, is not heard",
        "                events.remove(event.name);\n                self.preview_event(event.name);\n",
        "                events.remove(event.name);\n",
        [SOUNDS_HEARD],
    ),
    (
        "the user's file is not heard when the picker answers",
        "                            self.preview_event(event.name);\n",
        "",
        [SOUNDS_HEARD],
    ),
    (
        "a theme chosen is not heard",
        "                        self.preview(&choice);\n",
        "",
        [SOUNDS_HEARD],
    ),
    (
        "a theme chosen plays the user's sound in its place",
        "                        let choice = self.appearance.settings.sound_theme.sound(first.name);",
        "                        let choice = self.appearance.settings.sound_for(first.name);",
        [SOUNDS_HEARD],
    ),
    (
        "every slider let go plays the volume's sound",
        "                    if slider == SliderId::SoundVolume {",
        "                    if slider == slider {",
        [SLIDER_DRAG],
    ),
    (
        "the volume let go is not heard",
        "                    if slider == SliderId::SoundVolume {",
        "                    if slider != SliderId::SoundVolume {",
        [SOUNDS_HEARD],
    ),
    (
        "the volume let go plays the notification",
        '                        self.preview_event("audio-volume-change");',
        '                        self.preview_event("message-new-instant");',
        [SOUNDS_HEARD],
    ),
    (
        "a preview is not recorded",
        "        self.previewed = Some((sound.clone(), volume));\n",
        "",
        [SOUNDS_HEARD],
    ),
    (
        "a preview plays at full volume",
        "        let volume = self.appearance.settings.sounds.volume;\n        self.previewed",
        "        let volume = 1.0;\n        self.previewed",
        [SOUNDS_HEARD],
    ),
    (
        "a built-in sound is not previewed",
        "                    Some(builtin) => sound::Sound::BuiltIn(builtin),",
        "                    Some(_builtin) => return,",
        [SOUNDS_HEARD],
    ),
]


# Pages scroll (2026-10-10): a page longer than the window ran past its
# bottom, where nothing could be reached. One offset where every walk of the
# page starts; the wheel, the page keys and a scrollbar move it.
#
# Not a row: whether the scrollbar is lit under the pointer
# (`page_bar_hovered`), which only changes its colour.
SCROLL_LAST = "a_page_longer_than_the_window_scrolls_to_its_last_row"
SCROLL_ENDS = "a_page_scrolls_no_further_than_its_ends"
SCROLL_NEW_PAGE = "another_page_opens_at_its_top"
SCROLL_KEYS = "the_keys_scroll_the_page"
SCROLL_BAR = "the_scrollbar_scrolls_a_page_that_does_not_fit"
SCROLL_HEADER = "a_row_under_the_header_is_not_pressed_through_it"

MUTATIONS += [
    (
        "the page's walks ignore the scroll",
        "        Self::content_top() - self.page_scroll\n",
        "        Self::content_top()\n",
        [SCROLL_LAST],
    ),
    (
        "the page is drawn where it was before it scrolled",
        "            page_y - self.page_scroll,\n",
        "            page_y,\n",
        [SCROLL_LAST],
    ),
    (
        "the scrollbar is not drawn",
        "        self.render_page_bar(&mut tree);\n",
        "",
        [SCROLL_BAR],
    ),
    (
        "a page's rows are not measured",
        "        sink.y + PAGE_FOOT\n",
        "        PAGE_FOOT\n",
        [SCROLL_LAST],
    ),
    (
        "a page scrolls until its last row has gone",
        "        (self.page_height() - self.page_view_height()).max(0.0)\n",
        "        self.page_height().max(0.0)\n",
        [SCROLL_LAST],
    ),
    (
        "a page scrolls past its end",
        "        self.page_scroll = to.clamp(0.0, self.max_page_scroll());\n",
        "        self.page_scroll = to.max(0.0);\n",
        [SCROLL_ENDS],
    ),
    (
        "a page grown shorter is left scrolled past its end",
        "        if self.clamp_page_scroll() {\n            result = EventResult::Consumed;\n        }\n",
        "",
        [SCROLL_ENDS],
    ),
    (
        "another page opens where the last was scrolled to",
        "        self.page_scroll = 0.0;\n        self.page_bar_grab = None;\n",
        "        self.page_bar_grab = None;\n",
        [SCROLL_NEW_PAGE],
    ),
    (
        "the wheel over the sidebar scrolls the page",
        "            MouseEventKind::Scroll { dy, .. } if evt.x >= SIDEBAR_WIDTH => {\n",
        "            MouseEventKind::Scroll { dy, .. } => {\n",
        [SCROLL_LAST],
    ),
    (
        "the wheel scrolls the page the wrong way",
        "                if self.scroll_page_by(wheel::pixels(*dy, ITEM_HEIGHT)) {\n",
        "                if self.scroll_page_by(-wheel::pixels(*dy, ITEM_HEIGHT)) {\n",
        [SCROLL_LAST],
    ),
    (
        "Page Down moves a whole view, the row at the edge with it",
        "        (self.page_view_height() - ITEM_HEIGHT).max(ITEM_HEIGHT)\n",
        "        self.page_view_height().max(ITEM_HEIGHT)\n",
        [SCROLL_KEYS],
    ),
    (
        "Page Down goes up",
        "            Key::PageDown => {\n                self.scroll_page_by(self.page_step());\n",
        "            Key::PageDown => {\n                self.scroll_page_by(-self.page_step());\n",
        [SCROLL_KEYS],
    ),
    (
        "Page Up goes down",
        "            Key::PageUp => {\n                self.scroll_page_by(-self.page_step());\n",
        "            Key::PageUp => {\n                self.scroll_page_by(self.page_step());\n",
        [SCROLL_KEYS],
    ),
    (
        "End does not go to the end",
        "                self.scroll_page_to(f32::MAX);\n",
        "                self.scroll_page_to(0.0);\n",
        [SCROLL_KEYS],
    ),
    (
        "Home does not go to the top",
        "            Key::Home => {\n                self.scroll_page_to(0.0);\n",
        "            Key::Home => {\n                self.scroll_page_to(f32::MAX);\n",
        [SCROLL_KEYS],
    ),
    (
        "a page that fits has a scrollbar",
        "        if max <= 0.0 {\n            return None;\n        }\n        let view = self.page_view_height();\n",
        "        let view = self.page_view_height();\n",
        [SCROLL_BAR],
    ),
    (
        "the thumb is not taken hold of",
        "                self.page_bar_grab = Some(my - thumb.y);\n",
        "",
        [SCROLL_BAR],
    ),
    (
        "a press below the thumb moves the page up",
        "            } else {\n                self.scroll_page_by(self.page_step());\n            }\n            return EventResult::Consumed;\n",
        "            } else {\n                self.scroll_page_by(-self.page_step());\n            }\n            return EventResult::Consumed;\n",
        [SCROLL_BAR],
    ),
    (
        "a press above the thumb moves the page down",
        "            } else if my < thumb.y {\n                self.scroll_page_by(-self.page_step());\n",
        "            } else if my < thumb.y {\n                self.scroll_page_by(self.page_step());\n",
        [SCROLL_BAR],
    ),
    (
        "a held thumb does not follow the pointer",
        "        if let Some(grab) = self.page_bar_grab {\n            self.drag_page_bar_to(my, grab);\n            return EventResult::Consumed;\n        }\n",
        "",
        [SCROLL_BAR],
    ),
    (
        "the thumb is never let go",
        "                self.page_bar_grab = None;\n                EventResult::Consumed\n",
        "                EventResult::Consumed\n",
        [SCROLL_BAR],
    ),
    (
        "a drag of the thumb moves the page a pixel's worth",
        "        self.scroll_page_to(fraction * self.max_page_scroll());\n",
        "        self.scroll_page_to(fraction);\n",
        [SCROLL_BAR],
    ),
    (
        "a press on the header reaches the row under it",
        "        if my < Self::content_top() {\n            return None;\n        }\n",
        "",
        [SCROLL_HEADER],
    ),
    (
        "the reasons' boxes stay where the page was",
        "            y: self.page_top(),\n            disabled: Vec::new(),\n",
        "            y: Self::content_top(),\n            disabled: Vec::new(),\n",
        [SCROLL_HEADER],
    ),
    (
        "a reason is said through the header",
        "            Some(at) if !covered && at.1 >= Self::content_top() => {\n",
        "            Some(at) if !covered => {\n",
        [SCROLL_HEADER],
    ),
    (
        "a list opens where its button was before the page scrolled",
        "            y: self.page_top(),\n            found: None,\n",
        "            y: Self::content_top(),\n            found: None,\n",
        [SCROLL_LAST],
    ),
]

# A field lets the keyboard go when the page changes (2026-10-10): the
# Background page's exclusion box kept it, and took what was typed on the
# next page.
MUTATIONS += [
    (
        "a field keeps the keyboard on another page",
        "        // settings.\n        self.focused_field = None;\n",
        "        // settings.\n",
        ["a_field_lets_the_keyboard_go_when_the_page_changes"],
    ),
]


# The Window Rules page (2026-10-10, requests/c-e-a-window-rules-page-in-
# settings.md): lane C's rules in order, each switched, changed, moved and
# deleted, a new one added, the file's unreadable rules said and never saved
# over; the editor's fields and lists.
#
# Not rows: scrolling the editor to its top on opening and closing, which
# only moves the view; and the 256-rule limit on Add, which would need a test
# writing 256 rules for one sentence.
R_BUILT_IN = "with_no_rules_written_the_built_in_ones_are_shown_and_a_change_makes_them_yours"
R_LISTED = "each_rule_is_listed_with_what_it_matches_and_does"
R_SWITCH = "a_rule_switched_off_is_written_off_and_stays_in_its_place"
R_MOVE = "rules_move_up_and_down_and_the_ends_say_why_they_cannot"
R_DELETE = "a_rule_deleted_can_be_put_back_where_it_was"
R_ADD = "a_new_rule_is_written_above_the_rest"
R_EDIT = "a_rule_changed_keeps_its_place_and_its_name_must_be_its_own"
R_CANCEL = "cancel_keeps_nothing_of_the_editor"
R_UNREADABLE = "a_rule_the_file_cannot_read_is_shown_and_nothing_saves_over_it"
R_EVERY = "every_choice_the_editor_offers_reaches_the_file"
R_KEPT = "what_the_editor_does_not_offer_is_kept_and_a_snap_can_be_taken_off"
R_KEYS = "the_editors_fields_take_the_keyboard_in_turn"
R_TAB_SCROLL = "tab_scrolls_the_field_it_goes_to_into_view"
R_REREAD = "rules_changed_under_the_page_are_read_again"

MUTATIONS += [
    (
        "the rules are not read on entering the page",
        "        if page == SettingsPage::WindowRules {\n            self.refresh_rules();\n        }\n",
        "",
        [R_LISTED],
    ),
    (
        "with no file there are no rules, not the built-in ones",
        "                self.rules = windowrules::default_rules();\n",
        "                self.rules = Vec::new();\n",
        [R_BUILT_IN],
    ),
    (
        "the built-in rules are not said to be",
        "        if self.rules_built_in {\n",
        "        if !self.rules_built_in {\n",
        [R_BUILT_IN],
    ),
    (
        "a rule's name is not drawn",
        "            text_elided(tree, x, y + 14.0, &name, pal.text, 14.0, NAME_WIDTH);\n            render_toggle(",
        "            render_toggle(",
        [R_LISTED],
    ),
    (
        "a rule's actions are not said",
        "                    \"{}. {}\",\n",
        "                    \"{}.{}\",\n",
        [R_LISTED],
    ),
    (
        "the switch flips nothing",
        "        rule.enabled = !rule.enabled;\n",
        "",
        [R_SWITCH],
    ),
    (
        "a change is not written",
        "        let stored = windowrules::file::store(&rules);\n",
        "        let stored: std::io::Result<()> = Ok(());\n        let _ = &rules;\n",
        [R_SWITCH],
    ),
    (
        "the page shows what it holds, not what the file says",
        "        self.refresh_rules();\n        stored.is_ok()\n",
        "        stored.is_ok()\n",
        [R_BUILT_IN],
    ),
    (
        "a move moves nothing",
        "        self.rules.swap(index, other);\n",
        "",
        [R_MOVE],
    ),
    (
        "up moves down",
        "        let other = if up {\n            index.checked_sub(1)\n        } else {\n            index.checked_add(1)\n        };\n",
        "        let other = if up {\n            index.checked_add(1)\n        } else {\n            index.checked_sub(1)\n        };\n",
        [R_MOVE],
    ),
    (
        "the first rule is offered a move up",
        "            let up = if index == 0 {\n",
        "            let up = if index == usize::MAX {\n",
        [R_MOVE],
    ),
    (
        "the last rule is offered a move down",
        "            let down = if index == last {\n",
        "            let down = if index == usize::MAX {\n",
        [R_MOVE],
    ),
    (
        "a deleted rule is not offered back",
        "            self.rule_deleted = Some((index, rule));\n",
        "",
        [R_DELETE],
    ),
    (
        "a rule put back goes to the end",
        "        let at = index.min(self.rules.len());\n",
        "        let at = self.rules.len().min(index.max(self.rules.len()));\n",
        [R_DELETE],
    ),
    (
        "a rule put back is offered back again",
        "        let Some((index, rule)) = self.rule_deleted.take() else {\n",
        "        let Some((index, rule)) = self.rule_deleted.clone() else {\n",
        [R_DELETE],
    ),
    (
        "a change keeps the deleted rule on offer",
        "        rule.enabled = !rule.enabled;\n        self.rule_deleted = None;\n",
        "        rule.enabled = !rule.enabled;\n",
        [R_DELETE],
    ),
    (
        "a new rule goes to the bottom",
        "            None => self.rules.insert(0, rule),\n",
        "            None => self.rules.push(rule),\n",
        [R_ADD],
    ),
    (
        "a changed rule is added as a new one",
        "        match draft.editing.and_then(|at| self.rules.get_mut(at)) {\n",
        "        match draft.editing.and_then(|at| self.rules.get_mut(at)).filter(|_| false) {\n",
        [R_EDIT],
    ),
    (
        "Save leaves the editor open",
        "        self.close_rule_editor();\n        self.store_rules();\n",
        "        self.store_rules();\n",
        [R_ADD],
    ),
    (
        "Cancel keeps the editor",
        "            RowHit::Press(ButtonId::CancelRule) => self.close_rule_editor(),\n",
        "            RowHit::Press(ButtonId::CancelRule) => {}\n",
        [R_CANCEL],
    ),
    (
        "a closed editor's field keeps the keyboard",
        "            .is_some_and(|field| FieldId::RULE_FIELDS.contains(&field))\n        {\n            self.focused_field = None;\n        }\n",
        "            .is_some_and(|field| FieldId::RULE_FIELDS.contains(&field))\n        {\n        }\n",
        [R_CANCEL],
    ),
    (
        "the editor opens with the keyboard elsewhere",
        "        self.focused_field = Some(FieldId::RuleName);\n",
        "",
        [R_ADD],
    ),
    (
        "the editor opens on a blank rule to change one",
        "            Some((at, rule)) => rules::RuleDraft::of(rule, at),\n",
        "            Some(_) => rules::RuleDraft::new(),\n",
        [R_EDIT],
    ),
    (
        "Save is offered for a draft that is not a rule",
        "        let save = match &why {\n            None => Press::Does(RowHit::Press(ButtonId::SaveRule)),\n            Some(why) => Press::Cannot(why),\n        };\n",
        "        let save = Press::Does(RowHit::Press(ButtonId::SaveRule));\n",
        [R_EDIT],
    ),
    (
        "the reason is not said under Save",
        "        if let Some(why) = &why {\n            s.note(why, 28.0);\n        }\n",
        "",
        [R_EDIT],
    ),
    (
        "an unreadable rule's problem is not said",
        "                s.note(&line, 28.0);\n",
        "",
        [R_UNREADABLE],
    ),
    (
        "changes are not refused while a rule cannot be read",
        "        (!self.rule_problems.is_empty()).then_some(\n",
        "        (self.rule_problems.len() > usize::MAX - 1).then_some(\n",
        [R_UNREADABLE],
    ),
    (
        "a save writes over a rule that cannot be read",
        "        if self.rules_blocked().is_some() {\n            return false;\n        }\n",
        "",
        [R_UNREADABLE],
    ),
    (
        "a switch refused is kept to be written later",
        "    fn set_rule_enabled(&mut self, index: usize) {\n        if self.rules_blocked().is_some() {\n            return;\n        }\n",
        "    fn set_rule_enabled(&mut self, index: usize) {\n",
        [R_UNREADABLE],
    ),
    (
        "a move refused is kept to be written later",
        "        if index >= self.rules.len() || self.rules_blocked().is_some() {\n            return;\n        }\n        self.rules.swap(index, other);\n",
        "        if index >= self.rules.len() {\n            return;\n        }\n        self.rules.swap(index, other);\n",
        [R_UNREADABLE],
    ),
    (
        "a delete refused is kept to be written later",
        "        if index >= self.rules.len() || self.rules_blocked().is_some() {\n            return;\n        }\n        let rule = self.rules.remove(index);\n",
        "        if index >= self.rules.len() {\n            return;\n        }\n        let rule = self.rules.remove(index);\n",
        [R_UNREADABLE],
    ),
    (
        "the editor opens while a rule cannot be read",
        "    fn open_rule_editor(&mut self, index: Option<usize>) {\n        if self.rules_blocked().is_some() {\n            return;\n        }\n",
        "    fn open_rule_editor(&mut self, index: Option<usize>) {\n",
        [R_UNREADABLE],
    ),
    (
        "Remove them removes nothing",
        "        self.rule_problems.clear();\n",
        "",
        [R_UNREADABLE],
    ),
    (
        "the rules are not read again when their file changes",
        "            self.reread_rules()\n",
        "            false\n",
        [R_REREAD],
    ),
    (
        "the rule being changed is not followed to its new place",
        "            draft.editing = editing.and_then(|name| self.rules.iter().position(|r| r.name == name));\n",
        "            let _ = (draft, editing);\n",
        [R_REREAD],
    ),
    (
        "Tab does not go on to the next field",
        "            Key::Tab | Key::Enter if plain => {\n",
        "            Key::Enter if plain => {\n",
        [R_KEYS],
    ),
    (
        "Shift+Tab goes on, not back",
        "                let back = evt.key == Key::Tab && evt.modifiers.shift;\n",
        "                let back = evt.key == Key::Tab && evt.modifiers.shift && evt.modifiers.ctrl;\n",
        [R_KEYS],
    ),
    (
        "Escape keeps the keyboard in the field",
        "            Key::Escape if plain => {\n                self.focused_field = None;\n                Some(EventResult::Consumed)\n            }\n            Key::Tab | Key::Enter",
        "            Key::Escape if plain => Some(EventResult::Consumed),\n            Key::Tab | Key::Enter",
        [R_KEYS],
    ),
    (
        "what is typed does not reach the field",
        "                    .and_then(|draft| rule_input(draft, field))?;\n",
        "                    .and_then(|draft| rule_input(draft, FieldId::ExclusionDraft))?;\n",
        [R_KEYS],
    ),
    (
        "a field a choice hides keeps the keyboard",
        "            && !self.rule_fields().contains(&field)\n        {\n            self.focused_field = None;\n        }\n",
        "            && !self.rule_fields().contains(&field)\n        {\n        }\n",
        [R_KEYS],
    ),
    (
        "Tab goes to a field the editor does not show",
        "                FieldId::RulePlace => draft.place.field_label().is_some(),\n",
        "                FieldId::RulePlace => true,\n",
        [R_KEYS],
    ),
    (
        "Tab leaves the field it goes to off screen",
        "            self.scroll_into_view(AnchorId::Field(field));\n",
        "",
        [R_TAB_SCROLL],
    ),
    (
        "a field above the view is scrolled further away",
        "            self.scroll_page_by(y - top);\n",
        "            self.scroll_page_by(top - y);\n",
        [R_TAB_SCROLL],
    ),
    (
        "a field does not say where it is",
        "        self.anchor(AnchorId::Field(id), x, y);\n",
        "",
        [R_TAB_SCROLL],
    ),
    (
        "the list of ways to match opens on the first",
        "                |kind| *kind == draft.matching,\n",
        "                |kind| *kind != *kind,\n",
        [R_EDIT],
    ),
    (
        "a yes-or-no list opens on the first",
        "                    |tri| *tri == now,\n",
        "                    |tri| *tri != *tri && now == now,\n",
        [R_EDIT],
    ),
    (
        "the way to match is not chosen",
        "                    draft.matching = *kind;\n",
        "",
        [R_EVERY],
    ),
    (
        "where it opens is not chosen",
        "                    draft.place = *place;\n",
        "",
        [R_EVERY],
    ),
    (
        "how big it opens is not chosen",
        "                    draft.size = *size;\n",
        "",
        [R_EVERY],
    ),
    (
        "the desktop is not chosen",
        "                    draft.actions.desktop = *desktop;\n",
        "",
        [R_EVERY],
    ),
    (
        "how it opens is not chosen",
        "                    draft.actions.initial_state = *state;\n",
        "",
        [R_ADD],
    ),
    (
        "the opacity is not chosen",
        "                    draft.actions.opacity = *opacity;\n",
        "",
        [R_EVERY],
    ),
    (
        "a yes or no is not chosen",
        "                    flag.set(&mut draft.actions, tri.value());\n",
        "",
        [R_EVERY],
    ),
    (
        "a snap zone cannot be taken off",
        "                    draft.actions.snap_zone = None;\n",
        "",
        [R_KEPT],
    ),
    (
        "for the first window only is not kept",
        "            ToggleId::RuleOnce => &mut self.rule_draft.as_mut()?.once,\n",
        "            ToggleId::RuleOnce => return None,\n",
        [R_EVERY],
    ),
]


# Fonts from a theme (2026-10-10, requests/c-e-fonts-from-a-theme-on-the-
# fonts-page.md): where the fonts come from, chosen first; a theme's
# recommendations, which are installed and which will be drawn.
F_SOURCE = "the_fonts_come_from_your_own_choice_or_a_theme_and_reach_the_file"
F_THEME = "a_themes_fonts_say_which_are_installed_and_which_will_be_drawn"
F_PROBLEM = "a_font_theme_that_cannot_be_used_says_why_and_your_own_fonts_are_offered"
F_OWN = "a_family_of_your_own_is_your_own_fonts"
F_NONE = "with_no_theme_recommending_fonts_the_page_says_so"

MUTATIONS += [
    (
        "the themes are not read on entering the Fonts page",
        "        if page == SettingsPage::Fonts {\n            self.refresh_themes();\n        }\n",
        "",
        [F_SOURCE],
    ),
    (
        "the built-in theme is not called my own fonts",
        "        if info.origin == appearance::themes::Origin::BuiltIn {\n            MY_OWN_FONTS.to_owned()\n",
        "        if info.origin == appearance::themes::Origin::BuiltIn {\n            info.name.clone()\n",
        [F_SOURCE],
    ),
    (
        "a theme recommending no fonts does not say so",
        "            format!(\"{} -- recommends no fonts\", info.name)\n",
        "            info.name.clone()\n",
        [F_SOURCE],
    ),
    (
        "a theme recommending no fonts is chosen for them",
        "                    } else if info.provides_fonts() {\n                        self.appearance.settings.font_theme =\n",
        "                    } else if info.provides_fonts() || info.problem.is_none() {\n                        self.appearance.settings.font_theme =\n",
        [F_SOURCE],
    ),
    (
        "my own fonts cannot be chosen again",
        "                        self.appearance.settings.font_theme =\n                            appearance::themes::FontTheme::built_in();\n",
        "",
        [F_SOURCE],
    ),
    (
        "the fonts list opens on my own fonts whatever is chosen",
        ".position(|t| t.id.as_os_str() == self.appearance.settings.font_theme.id())",
        ".position(|t| t.id.as_os_str() == self.appearance.settings.font_theme.id() && t.dir.is_none())",
        [F_SOURCE],
    ),
    (
        "the fonts row does not name the theme chosen",
        "                |t| t.name.clone(),\n            )\n    }\n\n    /// A theme's row in the Taskbar panel list",
        "                |_t| MY_OWN_FONTS.to_owned(),\n            )\n    }\n\n    /// A theme's row in the Taskbar panel list",
        [F_SOURCE],
    ),
    (
        "a font theme no longer installed is shown as my own fonts",
        "                || std::path::Path::new(theme.id()).shown().to_string(),\n                |t| t.name.clone(),\n            )\n    }\n\n    /// A theme's row in the Taskbar",
        "                || MY_OWN_FONTS.to_owned(),\n                |t| t.name.clone(),\n            )\n    }\n\n    /// A theme's row in the Taskbar",
        [F_PROBLEM],
    ),
    (
        "a theme's fonts are not shown",
        "            self.build_theme_fonts(s);\n",
        "            self.build_own_fonts(s);\n",
        [F_THEME],
    ),
    (
        "a font theme that cannot be used offers no fonts",
        "        if theme.is_built_in() || theme.problem().is_some() {\n",
        "        if theme.is_built_in() {\n",
        [F_PROBLEM],
    ),
    (
        "why a font theme cannot be used is not said",
        "        if let Some(problem) = theme.problem() {\n            s.note(problem, 40.0);\n        }\n",
        "",
        [F_PROBLEM],
    ),
    (
        "every recommended family is said to be installed",
        "                if installed(family) {\n",
        "                if installed(family) || !family.is_empty() {\n",
        [F_THEME],
    ),
    (
        "the family drawn is the user's own",
        "        let drawn = settings.fonts_with_theme(installed);\n",
        "        let drawn = settings.fonts.clone();\n",
        [F_THEME],
    ),
    (
        "a family of one's own leaves the theme chosen",
        "                    self.appearance.settings.fonts.ui_font = family.clone();\n                    self.appearance.settings.font_theme = appearance::themes::FontTheme::built_in();\n",
        "                    self.appearance.settings.fonts.ui_font = family.clone();\n",
        [F_OWN],
    ),
    (
        "a fixed-pitch family of one's own leaves the theme chosen",
        "                    self.appearance.settings.fonts.mono_font = family.clone();\n                    self.appearance.settings.font_theme = appearance::themes::FontTheme::built_in();\n",
        "                    self.appearance.settings.fonts.mono_font = family.clone();\n",
        [F_OWN],
    ),
    (
        "the page says no theme recommends fonts when one does",
        "        if theme.is_built_in() && !self.themes.iter().any(|t| t.provides_fonts()) {\n",
        "        if theme.is_built_in() {\n",
        [F_NONE],
    ),
    (
        "the page never says no theme recommends fonts",
        "        if theme.is_built_in() && !self.themes.iter().any(|t| t.provides_fonts()) {\n",
        "        if theme.is_built_in() && self.themes.is_empty() {\n",
        [F_NONE],
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

# The Background page's pictures, made small off the window's thread
# (2026-10-10, design-decisions §1243).
THUMB_SMALL = "a_picture_is_made_small_and_uploaded_once"
THUMB_FAILS = "a_picture_that_cannot_be_read_fails"
THUMB_IDS = "each_picture_has_an_id_of_its_own"
THUMB_WAKE = "a_picture_decoded_wakes_the_window"

THUMBS = [
    (
        "a picture is not made small",
        "MAX_WIDTH, MAX_HEIGHT)",
        "4096, 4096)",
        [THUMB_SMALL],
    ),
    (
        "a decoded picture is not uploaded",
        "                    self.uploads.push(ImageChange::Upload {",
        "                    let _ = (ImageChange::Upload {",
        [THUMB_SMALL, THUMB_IDS],
    ),
    (
        "a picture asked for twice is decoded twice",
        "        if let Some((_, thumb)) = self.by_path.get(path) {\n            return *thumb;\n        }\n",
        "",
        [THUMB_SMALL],
    ),
    (
        "two pictures share an id",
        "        self.next_id = self.next_id.wrapping_add(1);\n",
        "",
        [THUMB_IDS],
    ),
    (
        "a picture that cannot be read waits for ever",
        "                Err(_why) => Thumb::Failed,",
        "                Err(_why) => Thumb::Loading,",
        [THUMB_FAILS],
    ),
    (
        "the worker wakes nobody",
        "                            waker.wake_by_ref();",
        "                            let _ = waker;",
        [THUMB_WAKE],
    ),
    (
        "a ready picture's size is not its own",
        "                    Thumb::Ready { id, width, height }",
        "                    Thumb::Ready { id, width: height, height: width }",
        [THUMB_SMALL],
    ),
]

# The Window Rules editor's model (`src/rules.rs`): a draft read into a rule,
# and a rule said in words.
RULE_UNTOUCHED = "a_rule_saved_untouched_is_the_rule_it_was"
RULE_NEW = "a_new_rule_does_nothing_until_it_is_told"
RULE_REFUSED = "a_draft_that_cannot_be_a_rule_says_why"
RULE_NUMBERS = "numbers_are_read_as_people_type_them"
RULE_FLAGS = "each_flag_reads_and_writes_its_own_action_in_its_rows_sense"
RULE_SAID = "a_rule_is_said_in_a_sentence"
RULE_LISTS = "a_list_shows_what_a_rule_says_even_off_the_list"

RULES = [
    (
        "only spaces part a pair of numbers",
        "matches!(c, ',' | 'x' | 'X' | '\\u{d7}')",
        "matches!(c, ' ')",
        [RULE_NUMBERS],
    ),
    (
        "a third number is not refused",
        "    if words.next().is_some() {\n        return None;\n    }\n",
        "",
        [RULE_NUMBERS],
    ),
    (
        "a percentage past a hundred is read",
        "    (0.0..=100.0).contains(&number).then_some(number / 100.0)\n",
        "    (0.0..=1000.0).contains(&number).then_some(number / 100.0)\n",
        [RULE_NUMBERS],
    ),
    (
        "a window may be no part of the screen",
        "    percent(word).filter(|fraction| *fraction > 0.0)\n",
        "    percent(word)\n",
        [RULE_NUMBERS],
    ),
    (
        "a window may be no pixels wide",
        "    word.parse().ok().filter(|n| *n > 0)\n",
        "    word.parse().ok()\n",
        [RULE_REFUSED],
    ),
    (
        "a name is not trimmed",
        "        let name = self.name.text().trim();\n",
        "        let name = self.name.text();\n",
        [RULE_NEW],
    ),
    (
        "a rule needs no name",
        "        if name.is_empty() {\n",
        "        if name.is_empty() && name.len() > 1 {\n",
        [RULE_REFUSED],
    ),
    (
        "a rule's own name is taken from it",
        "            .any(|(at, other)| Some(at) != self.editing && other.name == name)\n",
        "            .any(|(_, other)| other.name == name)\n",
        [RULE_REFUSED],
    ),
    (
        "a rule needs nothing to match",
        "            kind if text.is_empty() => return Err(kind.missing().to_owned()),\n",
        "",
        [RULE_REFUSED],
    ),
    (
        "the smallest size may be larger than the largest",
        "            && (min_w > max_w || min_h > max_h)\n",
        "            && (min_w > max_w && min_h > max_h && min_w == 0)\n",
        [RULE_REFUSED],
    ),
    (
        "for the first window only is lost",
        "        rule.one_shot = self.once;\n",
        "",
        [RULE_UNTOUCHED],
    ),
    (
        "a point's numbers are shown the wrong way round",
        "(Place::At, 0, format!(\"{x} {y}\"))",
        "(Place::At, 0, format!(\"{y} {x}\"))",
        [RULE_UNTOUCHED],
    ),
    (
        "the middle of a monitor becomes the first monitor's",
        "(Place::Centre, monitor, String::new())",
        "(Place::Centre, monitor.min(0), String::new())",
        [RULE_UNTOUCHED],
    ),
    (
        "no taskbar button is said as one",
        "            (Self::Taskbar, false) => \"no taskbar button\",\n",
        "            (Self::Taskbar, false) => \"a taskbar button\",\n",
        [RULE_SAID],
    ),
    (
        "a rule that does nothing says nothing",
        "        return \"Does nothing yet.\".to_owned();\n",
        "        return String::new();\n",
        [RULE_SAID],
    ),
    (
        "the sentence starts small",
        ".map(|first| first.to_uppercase().chain(chars).collect())",
        ".map(|first| first.to_lowercase().chain(chars).collect())",
        [RULE_SAID],
    ),
    (
        "the taskbar row reads the model's sense",
        "            Self::Taskbar => actions.skip_taskbar.map(|skip| !skip),\n",
        "            Self::Taskbar => actions.skip_taskbar,\n",
        [RULE_FLAGS],
    ),
    (
        "the taskbar row writes the model's sense",
        "            Self::Taskbar => actions.skip_taskbar = inverted,\n",
        "            Self::Taskbar => actions.skip_taskbar = value,\n",
        [RULE_FLAGS],
    ),
    (
        "a desktop past the desktops is not listed",
        "        choices.push(Some(desktop));\n",
        "",
        [RULE_LISTS],
    ),
    (
        "an opacity between the steps goes in the wrong place",
        ".position(|choice| choice.is_some_and(|step| step < opacity))",
        ".position(|choice| choice.is_some_and(|step| step > opacity))",
        [RULE_LISTS],
    ),
    (
        "a whole percentage keeps its .0",
        "    match shown.strip_suffix(\".0\") {\n",
        "    match shown.strip_suffix(\".x\") {\n",
        [RULE_LISTS],
    ),
]


TABLES = {
    "main.rs": MUTATIONS,
    "recyclebins.rs": RECYCLEBINS,
    "rules.rs": RULES,
    "thumbs.rs": THUMBS,
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
