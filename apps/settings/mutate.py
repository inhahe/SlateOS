"""Mutation test for the Settings app's pages that write the desktop's files.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what lane C asked this app for on 2026-09-27 and 2026-09-28: the
taskbar's window-titles switch, the automatic mode's hours, the pointer's size
and colours, the colour and icon themes, and the Date & Time page -- each a
setting the desktop obeyed that nothing here could change.  The rest of the
suite predates this table.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

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
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "settings", timeout=900, only=only))
