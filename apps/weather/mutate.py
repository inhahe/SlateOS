"""Mutation test for the weather app: its pointer layer, its settings, and
its forecasts from Open-Meteo.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The app drew six tabs, a settings list and a list of places, and handled no
pointer event (known-issues, TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-
CANNOT-BE-CLICKED).  With nothing fetched it also drew no shortcut card (while
the card swallowed every key), could not show its settings, and forgot the
units at every start.

The forecasts (design-decisions §1236) are off until the user turns them on:
the rows over `source.rs` are first of all about nothing being sent before
that, and then about the requests -- one of each kind out at once, an answer
for a place no longer shown dropped, a failure tried again after a while
rather than at once.  `openmeteo.rs` is what is asked and how the replies
read; `fetch.rs` the HTTP exchange.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# (name, old, new, [tests that must fail])
MAIN = [
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
        "                Rect::new(padding, cy, (width - remove_w - 20.0).max(0.0), row_h),\n"
        "            );",
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
        # Pressed alone: in a sequence the next key's store kept U's change
        # for it, and this row survived.
        ["each_unit_key_keeps_its_own_change"],
    ),
    (
        "the pointer lights nothing",
        "                self.hover = over;",
        "                let _ = over;",
        ["the_pointer_lights_the_tab_it_is_over"],
    ),
    (
        'an announced change is not read',
        '            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {',
        '            Event::SettingsChanged { group } if false && group.file_name() == CONFIG_NAME => {',
        ['a_unit_changed_in_one_window_reaches_the_others'],
    ),
    (
        "every program's announcement is read as this one's",
        '            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {',
        '            Event::SettingsChanged { group } if !group.file_name().is_empty() => {',
        ['a_unit_changed_in_one_window_reaches_the_others'],
    ),
    (
        'a re-read starts from the units it had, so a deleted file keeps them',
        '        let before = std::mem::take(&mut self.settings);',
        '        let before = self.settings.clone();',
        ['a_unit_changed_in_one_window_reaches_the_others'],
    ),
    (
        'a re-read says it changed nothing',
        '        before != self.settings\n    }',
        '        false\n    }',
        ['a_unit_changed_in_one_window_reaches_the_others'],
    ),
    (
        "a chord works the window's keys",
        '        if !textline::is_plain(key.modifiers) {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['a_key_held_with_a_modifier_is_not_the_windows'],
    ),
    # -- the forecasts: the window's side --------------------------------
    (
        "the turn-on button turns them on and asks nothing",
        "            Target::TurnOn => {\n                self.set_forecasts(true);",
        "            Target::TurnOn => {\n                self.source.on = true;",
        ["turning_forecasts_on_asks_for_the_place_shown_and_keeps_the_choice"],
    ),
    (
        "the Settings row turns them off and leaves the forecast up",
        "            Setting::Forecasts => {\n                self.set_forecasts(!self.source.on);",
        "            Setting::Forecasts => {\n                self.source.on = !self.source.on;",
        ["turning_forecasts_off_takes_what_was_fetched_from_the_window"],
    ),
    (
        "Try again asks nothing",
        "            Target::Retry => {\n                self.ask_forecast();\n",
        "            Target::Retry => {\n",
        ["a_failed_forecast_says_why_and_try_again_asks_again"],
    ),
    (
        "a place a search offered is not added when pressed",
        '                self.add_place(&place);\n                self.search.set_text("");',
        '                let _ = place;\n                self.search.set_text("");',
        ["a_place_is_found_by_name_and_added_with_where_it_is"],
    ),
    (
        "Enter in the search box searches for nothing",
        "                self.search_places(&name);\n                return EventResult::Consumed;",
        "                let _ = name;\n                return EventResult::Consumed;",
        ["a_place_is_found_by_name_and_added_with_where_it_is"],
    ),
    (
        "the search box has the keys on every tab",
        "        if self.active_view == ActiveView::Locations && self.source.on && !self.show_help {",
        "        if self.source.on && !self.show_help {",
        ["the_search_box_has_the_letters_only_on_its_own_tab"],
    ),
    (
        "the search box has the keys with forecasts off",
        "        if self.active_view == ActiveView::Locations && self.source.on && !self.show_help {",
        "        if self.active_view == ActiveView::Locations && !self.show_help {",
        ["nothing_is_sent_until_forecasts_are_turned_on"],
    ),
    (
        "R asks nothing",
        "            Key::R if self.source.on && !self.locations.is_empty() => {\n"
        "                self.ask_forecast();\n",
        "            Key::R if self.source.on && !self.locations.is_empty() => {\n",
        ["the_search_box_has_the_letters_only_on_its_own_tab"],
    ),
    (
        "a refresh going out is not drawn",
        "                    // What the window says changes: it is asking.\n"
        "                    changed = true;\n",
        "",
        ["a_failed_refresh_is_tried_again_after_a_while_not_at_once"],
    ),
    (
        "the clock runs with nothing to wait for",
        "        if self.source.waiting() {\n            return Some(Duration::from_millis(500));",
        "        if true || self.source.waiting() {\n            return Some(Duration::from_millis(500));",
        ["nothing_is_sent_until_forecasts_are_turned_on"],
    ),
    (
        "a failed refresh takes the credit's place",
        "            lines.insert(\n                0,",
        "            lines.clear();\n            lines.insert(\n                0,",
        ["a_failed_refresh_is_tried_again_after_a_while_not_at_once"],
    ),
    (
        "removing the place shown asks nothing for the one now shown",
        "        if shown_after != shown_before {\n"
        "            self.forget_weather();\n            self.ask_forecast();\n        }",
        "        if shown_after != shown_before {\n            self.forget_weather();\n        }",
        ["removing_the_place_shown_shows_the_next_and_asks_for_it"],
    ),
    (
        "a place removed above the one shown moves the window down a row",
        "            self.active_location_idx = self.active_location_idx.saturating_sub(1);\n"
        "        } else if self.active_location_idx >= self.locations.len() {",
        "        } else if self.active_location_idx >= self.locations.len() {",
        ["removing_a_place_above_the_one_shown_leaves_it_shown"],
    ),
    (
        "a window's own save, read back, shows the default place",
        "        {\n            self.active_location_idx = i;\n        }",
        "        {\n            let _ = i;\n        }",
        ["a_windows_own_save_announced_back_leaves_it_showing_the_same_place"],
    ),
    (
        "forecasts turned off elsewhere stay up here",
        "        if was_on && !self.source.on {",
        "        if false && was_on && !self.source.on {",
        ["forecasts_turned_on_and_off_in_one_window_reach_the_others"],
    ),
    (
        "forecasts turned on elsewhere ask nothing here for the same place",
        "(!was_on || shown_after != shown_before)",
        "(shown_after != shown_before)",
        ["forecasts_turned_on_elsewhere_ask_for_the_place_already_shown_here"],
    ),
]

# -- the forecasts: what is sent, when, and what is done with the answers ----
SOURCE = [
    (
        "forecasts are on before the user turns them on",
        "            on: false,\n            fetcher: DEFAULT_FETCHER,",
        "            on: true,\n            fetcher: DEFAULT_FETCHER,",
        ["nothing_is_sent_until_forecasts_are_turned_on"],
    ),
    (
        "a forecast is asked for with forecasts off",
        "        if !self.source.on {\n            return;\n        }\n        let Some(location)",
        "        let Some(location)",
        ["nothing_is_sent_until_forecasts_are_turned_on"],
    ),
    (
        "a search is sent with forecasts off",
        "        if !self.source.on || name.is_empty() {",
        "        if name.is_empty() {",
        ["nothing_is_sent_until_forecasts_are_turned_on"],
    ),
    (
        "the switch is not kept",
        "            self.stop_fetching();\n        }\n        self.store_places();\n    }",
        "            self.stop_fetching();\n        }\n    }",
        [
            "turning_forecasts_on_asks_for_the_place_shown_and_keeps_the_choice",
            "turning_forecasts_off_takes_what_was_fetched_from_the_window",
        ],
    ),
    (
        "turned off, the forecast stays up",
        "        self.source.search_error = None;\n        self.forget_weather();\n    }",
        "        self.source.search_error = None;\n    }",
        ["turning_forecasts_off_takes_what_was_fetched_from_the_window"],
    ),
    (
        "a search takes the forecast request's place",
        "            Ok(answer) => {\n                self.source.search = Some(Pending {",
        "            Ok(answer) => {\n                self.source.forecast = None;\n"
        "                self.source.search = Some(Pending {",
        ["a_search_while_the_forecast_is_out_leaves_it_out"],
    ),
    (
        "a failed search is said as the forecast's failure",
        "                Err(why) => {\n                    self.source.search_error = Some(format!(",
        "                Err(why) => {\n                    self.source.error = Some(format!(",
        ["a_failed_search_is_said_under_the_box_and_not_over_the_forecast"],
    ),
    (
        "a failed request is asked for again at once",
        "            self.source.tried_at?.checked_add(RETRY)",
        "            Some(self.source.tried_at?)",
        ["a_failed_refresh_is_tried_again_after_a_while_not_at_once"],
    ),
    (
        "a failed request is never asked for again",
        "            self.source.tried_at?.checked_add(RETRY)",
        "            None",
        ["a_failed_refresh_is_tried_again_after_a_while_not_at_once"],
    ),
    (
        "the forecast is asked for again on the hour, not the half hour",
        "            self.source.fetched_at?.checked_add(REFRESH)",
        "            self.source.fetched_at?.checked_add(REFRESH * 2)",
        ["the_forecast_is_asked_for_again_on_the_half_hour"],
    ),
    (
        "an answer for another place is shown under this one",
        "same_spot(l.latitude, asked.latitude) && same_spot(l.longitude, asked.longitude)",
        "same_spot(l.latitude, l.latitude) && same_spot(l.longitude, asked.longitude)",
        ["an_answer_for_a_place_no_longer_shown_is_not_shown_under_it"],
    ),
    (
        "an air-quality index nobody gave is shown as 0",
        "            .ok();\n            Ok((forecast, air))",
        "            .ok();\n            Ok((forecast, air.or(Some(0))))",
        ["the_forecast_stands_when_the_air_quality_service_does_not_answer"],
    ),
    (
        "the air quality failing takes the forecast with it",
        "            .and_then(|body| openmeteo::read_us_aqi(&body))\n            .ok();",
        "            .and_then(|body| openmeteo::read_us_aqi(&body))\n            .map(Some)?;",
        ["the_forecast_stands_when_the_air_quality_service_does_not_answer"],
    ),
    (
        "a place's latitude is not kept",
        '            doc.set_f64(&["places", &key, "latitude"], location.latitude);\n',
        "",
        ["turning_forecasts_on_asks_for_the_place_shown_and_keeps_the_choice"],
    ),
    (
        "a place off the globe is read back",
        "            if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {\n"
        "                continue;\n            }\n",
        "",
        ["places_read_back_leave_out_what_cannot_be_a_place"],
    ),
    (
        "a default naming no place is taken",
        "            .filter(|i| *i < locations.len())\n",
        "",
        ["places_read_back_leave_out_what_cannot_be_a_place"],
    ),
    (
        "the default is not read back",
        '            .get_i64(&["default"])',
        '            .get_i64(&["no default"])',
        ["places_read_back_leave_out_what_cannot_be_a_place"],
    ),
    (
        "a misspelt switch is taken as the user's consent",
        '        self.source.on = doc.get_bool(&["forecasts"]) == Some(true);',
        '        self.source.on = doc.get_bool(&["forecasts"]) != Some(false);',
        ["places_read_back_leave_out_what_cannot_be_a_place"],
    ),
    (
        "the same place is added twice",
        "same_spot(l.latitude, place.latitude) && same_spot(l.longitude, place.longitude)",
        "same_spot(l.latitude, place.latitude) && false",
        ["a_place_is_found_by_name_and_added_with_where_it_is"],
    ),
    (
        "with no place left, its request stays out",
        "        let Some(location) = self.locations.get(self.active_location_idx) else {\n"
        "            self.source.forecast = None;\n            return;",
        "        let Some(location) = self.locations.get(self.active_location_idx) else {\n"
        "            return;",
        ["removing_the_place_shown_shows_the_next_and_asks_for_it"],
    ),
    (
        "a place shown is not asked for",
        "        if !same {\n            self.forget_weather();\n            self.ask_forecast();\n        }",
        "        if !same {\n            self.forget_weather();\n        }",
        ["a_place_is_found_by_name_and_added_with_where_it_is"],
    ),
]

# -- what is asked, and how the replies read ---------------------------------
OPENMETEO = [
    (
        "a place name is sent as typed",
        "        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {",
        "        if byte != b'%' {",
        ["a_place_name_is_percent_encoded"],
    ),
    (
        "the forecast asks for three days",
        "pub const FORECAST_DAYS: u32 = 7;",
        "pub const FORECAST_DAYS: u32 = 3;",
        ["the_forecast_asks_for_what_the_reader_reads"],
    ),
    (
        "nothing found is an error",
        "        return Ok(Vec::new());\n    };\n    let results",
        '        return Err(String::from("no results"));\n    };\n    let results',
        ["a_search_reply_is_read_as_places"],
    ),
    (
        "the hours start at midnight, not the hour observed",
        "        .position(|t| t.as_str() == this_hour.as_deref())",
        "        .position(|_| true)",
        ["a_forecast_reply_is_read_as_the_app_shows_it"],
    ),
    (
        "every hour of the week is shown",
        "        if hours.len() == HOURS_SHOWN {\n            break;\n        }\n",
        "",
        ["a_forecast_reply_is_read_as_the_app_shows_it"],
    ),
    (
        "an hour with no rain chance is shown as 0%",
        "            rain.get(i).and_then(JsonValue::as_f64),\n",
        "            rain.get(i).and_then(JsonValue::as_f64).or(Some(0.0)),\n",
        ["an_hour_or_a_day_with_a_value_missing_is_left_out"],
    ),
    (
        "a day with no high is shown as 0 degrees",
        '            at("temperature_2m_max"),',
        '            at("temperature_2m_max").or(Some(0.0)),',
        ["an_hour_or_a_day_with_a_value_missing_is_left_out"],
    ),
    (
        "the first day is not called Today",
        "            day_name: if i == 0 {",
        "            day_name: if i == 99 {",
        ["a_forecast_reply_is_read_as_the_app_shows_it"],
    ),
    (
        "visibility in metres is shown as kilometres",
        '        visibility_km: as_f32(need("visibility")? / 1000.0),',
        '        visibility_km: as_f32(need("visibility")?),',
        ["a_forecast_reply_is_read_as_the_app_shows_it"],
    ),
    (
        "the service's refusal is read as a reply",
        '    if reply.get("error").and_then(JsonValue::as_bool) == Some(true) {',
        '    if reply.get("error").and_then(JsonValue::as_bool) == Some(false) {',
        ["the_service_refusing_says_so_in_its_own_words"],
    ),
    (
        "heavy rain reads as rain",
        "        65 | 82 => WeatherCondition::HeavyRain,",
        "        65 | 82 => WeatherCondition::Rain,",
        ["wmo_codes_read_as_the_weather_they_name"],
    ),
    (
        "a code nobody knows reads as clear",
        "        _ => WeatherCondition::Overcast,",
        "        _ => WeatherCondition::Clear,",
        ["wmo_codes_read_as_the_weather_they_name"],
    ),
    (
        "the weekdays forget the century rule",
        "        .checked_sub(y.div_euclid(100))?",
        "        .checked_sub(0)?",
        ["a_date_reads_as_its_weekday"],
    ),
    (
        "a time past midnight is read",
        "    (hour < 24 && minute < 60).then_some((hour, minute))",
        "    Some((hour, minute))",
        ["a_date_reads_as_its_weekday"],
    ),
]

# -- the HTTP exchange ------------------------------------------------------
FETCH = [
    (
        "a reply of any length is read",
        "    if raw.len() > MAX_REPLY {",
        "    if raw.len() > MAX_REPLY * 2 {",
        ["a_reply_longer_than_any_forecast_is_refused_unread"],
    ),
    (
        "a failure's body is read as the reply",
        "    if !response.is_success() {",
        "    if false && !response.is_success() {",
        ["a_reply_that_is_not_a_success_says_its_status"],
    ),
    (
        "the connection is not closed after the reply",
        '        .header("Connection", "close")\n',
        "",
        ["a_request_reaches_the_server_and_its_reply_comes_back"],
    ),
    (
        "a server not there is not said to be unreachable",
        '            Some(e) => format!("{host} could not be reached: {e}"),',
        '            Some(e) => format!("{host}: {e}"),',
        ["a_server_that_is_not_there_is_said_to_be_unreachable"],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "source.rs": SOURCE,
    "openmeteo.rs": OPENMETEO,
    "fetch.rs": FETCH,
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
        worst = max(worst, sweep(SRC / file, rows, "weather", timeout=900, only=mine))
    raise SystemExit(worst)
