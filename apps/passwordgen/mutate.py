"""Mutation test for the password generator's rules and its analyser.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-27 (C-Q26): the checking rules stopped
being compiled in -- the Rules tab draws and changes them, and
`passwordgen.yaml` keeps them, refusing a value it cannot use and saying so --
and each tab began judging the password it shows.  The analyser draws what is
typed (a dot per character until Ctrl+R), takes digits as part of the password
rather than as tab keys, measures characters rather than bytes, and a tab
switch measures the password on the new tab.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ARROWS = "the_arrows_change_the_rule_under_the_cursor"
CURSOR = "the_cursor_stays_on_the_list"
LENGTHS = "no_limit_sits_above_the_longest_length_and_the_lengths_hold_each_other"
DRAWN = "the_rules_tab_draws_every_rule_and_what_they_say"
INSIDE = "the_rules_tab_stays_inside_its_panel_and_keeps_the_cursor_in_view"
KEPT = "a_rule_changed_is_kept_and_the_next_window_starts_with_it"
QUIET = "a_window_a_test_builds_keeps_no_rules"
REFUSED = "a_kept_rule_that_cannot_be_used_is_said_and_the_default_used"
ROUND_TRIP = "the_rules_read_back_as_they_were_written"
PROBLEM = "a_settings_problem_is_said_when_the_window_opens_and_cleared_by_a_change"
DIGITS = "digits_are_part_of_the_password_in_the_analyser"
CONTROL = "a_key_that_carries_a_control_character_is_not_typed"
DOTS = "the_analyser_shows_what_is_typed_as_dots_until_asked"
EACH_TAB = "each_tab_measures_the_password_it_shows"
CHARS = "a_password_is_as_long_as_its_characters_not_its_bytes"
STATUS = "the_status_bar_judges_the_password_on_show"
EMPTIED = "deleting_every_character_takes_the_strength_away_too"
TAB_ORDER = "tab_visits_every_tab_in_the_toolbar_order"
ADVERTISED = "every_advertised_key_does_something"

MUTATIONS = [
    # -- the settings file ---------------------------------------------------
    (
        "a kept number outside its range is used",
        "    let n = doc.get_i64(&path).filter(|n| (lo..=hi).contains(n));",
        "    let n = doc.get_i64(&path);",
        [REFUSED],
    ),
    (
        "an unreadable kept switch is not said",
        "    if on.is_none() {",
        "    if false {",
        [REFUSED],
    ),
    (
        "a longest below the shortest is used",
        "            if n >= rules.min_length {",
        "            if true {",
        [REFUSED],
    ),
    (
        "no limit is written as the old limit",
        '            doc.remove(&[RULES_KEY, "longest"]);',
        "            ();",
        [ROUND_TRIP],
    ),
    (
        "a window a test builds writes the rules",
        "        if !self.keeps_settings {\n"
        "            return;\n"
        "        }\n"
        "        let mut doc = settingsfile::load(CONFIG_NAME);\n"
        "        self.policy.store_into(&mut doc);",
        "        let mut doc = settingsfile::load(CONFIG_NAME);\n"
        "        self.policy.store_into(&mut doc);",
        [QUIET],
    ),
    (
        "the program's window keeps nothing",
        "        self.keeps_settings = true;\n        let (rules, problems)",
        "        self.keeps_settings = false;\n        let (rules, problems)",
        [KEPT],
    ),
    (
        "the kept rules are not read",
        "        self.policy = rules;\n        if let Some(first) = problems.first() {",
        "        let _ = rules;\n        if let Some(first) = problems.first() {",
        [KEPT],
    ),
    (
        "a settings problem is not said on opening",
        "        if let Some(first) = problems.first() {",
        "        if let Some(first) = None::<&String> {",
        [PROBLEM],
    ),
    (
        "a file written whole keeps its problems",
        "                    self.settings_problems.clear();",
        "                    ();",
        [PROBLEM],
    ),
    (
        "the settings problems are not drawn",
        "        if !self.settings_problems.is_empty() && fits(cy) {",
        "        if false && fits(cy) {",
        [PROBLEM],
    ),
    # -- the Rules tab's keys ------------------------------------------------
    (
        "the Rules tab key does nothing",
        "            Key::Num4 => self.set_tab(ActiveTab::Rules),",
        "",
        [ARROWS, ADVERTISED],
    ),
    (
        "Tab never reaches the rules",
        "            Self::History => Self::Rules,",
        "            Self::History => Self::Generator,",
        [TAB_ORDER],
    ),
    (
        "the cursor runs off the list",
        "        self.rule_cursor = step(before, delta, 0, RuleRow::ALL.len().saturating_sub(1));",
        "        self.rule_cursor = step(before, delta, 0, RuleRow::ALL.len());",
        [CURSOR],
    ),
    (
        "an unchanged rule is answered as a change",
        "        if self.policy == before {\n            return EventResult::Ignored;\n        }",
        "        if false {\n            return EventResult::Ignored;\n        }",
        [ARROWS, LENGTHS],
    ),
    (
        "Space changes a number",
        "            Some(row) if row.is_switch() => self.change_rule(1),",
        "            Some(_) => self.change_rule(1),",
        [ARROWS],
    ),
    (
        "strength moves in ones",
        "const RULE_BITS_STEP: u32 = 5;",
        "const RULE_BITS_STEP: u32 = 1;",
        [ARROWS],
    ),
    (
        "the shortest can pass the longest",
        "                    .unwrap_or(RULE_LENGTH_MAX)\n"
        "                    .clamp(1, RULE_LENGTH_MAX);",
        "                    .map_or(RULE_LENGTH_MAX, |_| RULE_LENGTH_MAX)\n"
        "                    .clamp(1, RULE_LENGTH_MAX);",
        [LENGTHS],
    ),
    (
        "no limit is not above the numbers",
        "    (moved < no_limit).then_some(moved)",
        "    Some(moved)",
        [LENGTHS],
    ),
    (
        "the longest can go below the shortest",
        "    let lowest = shortest.clamp(1, RULE_LENGTH_MAX);",
        "    let lowest = shortest.clamp(1, 1);",
        [LENGTHS],
    ),
    # -- the Rules tab's drawing ---------------------------------------------
    (
        "the rows never follow the cursor",
        "        let first = self.rule_cursor.saturating_add(1).saturating_sub(room);",
        "        let first = 0;",
        [INSIDE],
    ),
    (
        "what the rules say runs past the panel",
        "    let room = rows_that_fit(top, column.bottom, RULE_LINE_HEIGHT);",
        "    let room = usize::MAX;",
        [INSIDE],
    ),
    (
        "every password meets every rule",
        '        if broken.is_empty() {\n            let met = ["Meets every rule".to_owned()];',
        '        if true {\n            let met = ["Meets every rule".to_owned()];',
        [DRAWN],
    ),
    # -- the analyser and which password is judged ---------------------------
    (
        "a digit leaves the analyser",
        "                Key::Tab => {}",
        "                Key::Tab | Key::Num1 => {}",
        [DIGITS],
    ),
    (
        "Ctrl+R is typed",
        "                Key::R if textline::is_ctrl_chord(key.modifiers) => {",
        "                Key::R if false => {",
        [DOTS],
    ),
    (
        "a control character is typed",
        "                    if !textline::types_into_field(key) {",
        "                    if key.text.is_empty() || textline::is_command(key.modifiers) {",
        [CONTROL],
    ),
    (
        "the analyser draws the password in the clear",
        "            let whole = if self.analyzer_revealed {",
        "            let whole = if true {",
        [DOTS],
    ),
    (
        "bytes are counted as characters",
        "    let length = password.chars().count();",
        "    let length = password.len();",
        [CHARS],
    ),
    (
        "every tab judges the generated password",
        "        if self.active_tab == ActiveTab::Analyzer {\n            &self.analyzer_input",
        "        if false {\n            &self.analyzer_input",
        [EACH_TAB, STATUS],
    ),
    (
        "a tab switch keeps the last tab's strength",
        "        self.current_analysis = (!shown.is_empty()).then(|| analyze_password(shown));",
        "        let _ = shown;",
        [EACH_TAB],
    ),
    (
        "an emptied field is given a strength",
        "            (!self.analyzer_input.is_empty()).then(|| analyze_password(&self.analyzer_input));",
        "            Some(analyze_password(&self.analyzer_input));",
        [EMPTIED],
    ),
    (
        "the status bar judges an empty field",
        "            let rules = if shown.is_empty() {",
        "            let rules = if false {",
        [STATUS],
    ),
    (
        "AltGr+E opens the export dialog",
        "        if textline::is_ctrl_chord(key.modifiers) && key.key == Key::E {",
        "        if key.modifiers.ctrl && key.key == Key::E {",
        ["altgr_types_into_the_analyser_and_runs_no_chord"],
    ),
    (
        "AltGr+R reveals the password",
        "                Key::R if textline::is_ctrl_chord(key.modifiers) => {",
        "                Key::R if key.modifiers.ctrl => {",
        ["altgr_types_into_the_analyser_and_runs_no_chord"],
    ),
    (
        "the analyser refuses what AltGr types",
        "                    if !textline::types_into_field(key) {",
        "                    if !textline::types_into_field(key) || key.modifiers.ctrl {",
        ["altgr_types_into_the_analyser_and_runs_no_chord"],
    ),
    (
        "the analyser types a command's letter",
        "                    if !textline::types_into_field(key) {",
        "                    if !key.types_text() {",
        ["altgr_types_into_the_analyser_and_runs_no_chord"],
    ),
    (
        "a key held with Ctrl, Alt or the Windows key is a bare key",
        "        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        // On the Rules tab",
        "        // On the Rules tab",
        ["a_key_held_with_ctrl_alt_or_the_windows_key_is_no_bare_key"],
    ),
    (
        "a key held with Ctrl is a bare key",
        "        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        // On the Rules tab",
        "        if key.modifiers.alt || key.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        // On the Rules tab",
        ["a_key_held_with_ctrl_alt_or_the_windows_key_is_no_bare_key"],
    ),
    (
        "a key held with Alt is a bare key",
        "        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        // On the Rules tab",
        "        if key.modifiers.ctrl || key.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        // On the Rules tab",
        ["a_key_held_with_ctrl_alt_or_the_windows_key_is_no_bare_key"],
    ),
    (
        "a key held with the Windows key is a bare key",
        "        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        // On the Rules tab",
        "        if key.modifiers.ctrl || key.modifiers.alt {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        // On the Rules tab",
        ["a_key_held_with_ctrl_alt_or_the_windows_key_is_no_bare_key"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "passwordgen", timeout=900))
