"""Mutation test for the weather app's pointer layer and its settings.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The app drew six tabs, a settings list and a list of places, and handled no
pointer event (known-issues, TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-
CANNOT-BE-CLICKED).  With nothing fetched -- which is always, today -- it also
drew no shortcut card (while the card swallowed every key), could not show
its settings, and forgot the units at every start.

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
        "the card is drawn only over fetched weather",
        "        // was never drawn and that swallowed every key until it was closed.\n"
        "        if self.show_help {",
        "        // was never drawn and that swallowed every key until it was closed.\n"
        "        if self.show_help && self.current.is_some() {",
        ["with_nothing_fetched_the_shortcut_card_is_drawn_and_a_press_closes_it"],
    ),
    (
        "the settings are hidden behind the cannot-fetch notice",
        "        if self.active_view == ActiveView::SettingsView {\n"
        "            self.render_settings_view(cmds, title_y);\n            return;\n        }\n",
        "",
        ["with_nothing_fetched_the_settings_can_be_seen_and_changed"],
    ),
    (
        "a tab is drawn and records no hit box",
        "            cmds.hit(Target::Tab(*view), tab);",
        "",
        ["every_tab_is_a_button", "the_pointer_lights_the_tab_it_is_over"],
    ),
    (
        "a settings row records no hit box",
        "            cmds.hit(Target::Setting(*setting), row);",
        "",
        ["with_nothing_fetched_the_settings_can_be_seen_and_changed"],
    ),
    (
        "a place records no hit box",
        "            cmds.hit(\n                Target::Location(i),\n"
        "                Rect::new(padding, cy, self.width - padding * 2.0, row_h),\n            );",
        "",
        ["a_place_is_chosen_by_pressing_it"],
    ),
    (
        "the hourly strip records no hit box",
        "        cmds.hit(Target::HourlyStrip, Rect::new(x, y, strip_w, strip_h));",
        "",
        ["the_wheel_scrolls_the_hourly_strip_either_way"],
    ),
    (
        "the sideways wheel does not move the strip",
        "                let turn = if dx.abs() > dy.abs() { -dx } else { dy };",
        "                let turn = dy;",
        ["the_wheel_scrolls_the_hourly_strip_either_way"],
    ),
    (
        "the units are not kept",
        '            doc.set_str(&["units", key], word);',
        "            let _ = (key, word);",
        ["the_units_are_kept_between_sessions"],
    ),
    (
        "a kept unit is not read back",
        '            Some("fahrenheit") => self.settings.temp_unit = TempUnit::Fahrenheit,',
        "",
        ["the_units_are_kept_between_sessions"],
    ),
    (
        "the keys change the units without keeping them",
        "                self.change_setting(Setting::Temperature);",
        "                self.toggle_temp_unit();",
        ["the_units_are_kept_between_sessions"],
    ),
    (
        "the pointer lights nothing",
        "                self.hover = over;",
        "                let _ = over;",
        ["the_pointer_lights_the_tab_it_is_over"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "weather", timeout=900, only=only))
