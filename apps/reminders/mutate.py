"""Mutation test for the reminders list kept on disk, the reminder form, its
steps, the question before a delete, and a repeating reminder coming round.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

There was no way to add a reminder: the store's `add` had no caller but the
tests and the JSON import; nothing could change or delete one or reach its
steps; a repeating reminder done once was finished for good; and nothing was
kept.  The table covers what replaced that; the rest of the suite predates it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "the_kept_list_reads_back_what_it_wrote_whatever_the_text"
REFUSED = "a_list_that_cannot_be_read_whole_is_refused_and_says_why"
KEPT = "a_reminder_is_added_from_the_keyboard_and_is_there_next_time"
QUIET = "a_window_made_by_new_keeps_nothing"
WRONG = "the_form_says_what_is_wrong_and_keeps_what_was_typed"
CHANGE = "e_changes_the_selected_reminder_and_keeps_what_the_form_does_not_show"
STEPS = "steps_are_added_ticked_and_taken_off_in_the_form_and_ticked_from_the_list"
DELETE = "delete_asks_first_and_each_answer_does_what_it_says"
MODAL = "the_form_takes_every_key_while_it_is_up"
REPEAT = "a_repeating_reminder_comes_round_again_when_it_is_done"
BROKEN = "a_list_file_that_cannot_be_read_is_left_as_it_is"
BIG = "a_list_file_too_big_to_read_whole_is_refused"
FAILING = "closing_while_a_save_fails_asks_first"
COUNTS = "the_store_counts_its_changes_and_nothing_else"
NOTICE = "the_warning_lines_are_not_painted_over"
EMPTY = "a_fresh_window_holds_no_tasks_and_says_how_to_add_one"
KEYS = "every_advertised_key_does_something"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ---- the store counts its changes ----
    (
        "an add is not counted",
        "        self.tasks.push(task);\n        self.changed();\n        id",
        "        self.tasks.push(task);\n        id",
        [COUNTS, KEPT],
    ),
    (
        "a change through get_mut is not counted",
        "        let at = self.tasks.iter().position(|t| t.id == id)?;\n        self.changed();\n",
        "        let at = self.tasks.iter().position(|t| t.id == id)?;\n",
        [COUNTS],
    ),
    (
        "removing nothing is counted",
        "        let removed = self.tasks.len() < before;\n        if removed {",
        "        let removed = self.tasks.len() < before;\n        if true {",
        [COUNTS],
    ),
    # ---- the file ----
    (
        "the file loses the title",
        "            tsv::escape(&t.title),",
        "            String::new(),",
        [ROUND, KEPT],
    ),
    (
        "the file loses the repeat",
        "            t.recurrence.to_json_str(),",
        "            RecurrenceRule::None.to_json_str(),",
        [ROUND],
    ),
    (
        "the file loses the snooze",
        "            optional_when(t.snoozed_until),",
        "            optional_when(None),",
        [ROUND],
    ),
    (
        "the file loses the steps",
        '            out.push_str("step\\t");',
        '            out.push_str("gone\\t");',
        [ROUND],
    ),
    (
        "two reminders with one number are read",
        "                if tasks.iter().any(|t| t.id == id) {",
        "                if false {",
        [REFUSED],
    ),
    (
        "a step before any reminder is dropped quietly",
        '                    .ok_or_else(|| bad("a step with no reminder before it"))?',
        '                    .ok_or_else(|| bad("elsewhere"))?',
        [REFUSED],
    ),
    # ---- keeping it ----
    (
        "a first run is taken for a broken file",
        "            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,\n",
        "",
        [KEPT],
    ),
    (
        "a file too large is read in part",
        "        if read.truncated {\n            self.persist = false;",
        "        if false {\n            self.persist = false;",
        [BIG],
    ),
    (
        "nothing is written",
        "        if !self.persist || self.store.revision() == self.kept_revision {",
        "        if true {",
        [KEPT],
    ),
    (
        "a window made by new keeps its reminders",
        "        if !self.persist || self.store.revision() == self.kept_revision {",
        "        if self.store.revision() == self.kept_revision {",
        [QUIET],
    ),
    (
        "the list is not kept after an event",
        "        let result = self.route_event(event);\n        self.keep();\n",
        "        let result = self.route_event(event);\n",
        [KEPT],
    ),
    (
        "Save closes although the save failed",
        "                self.running = self.unkept();",
        "                self.running = false;",
        [FAILING],
    ),
    (
        "the close question is not drawn",
        "            question.render(&palette, width, height, &mut tree);\n",
        "",
        [FAILING],
    ),
    # ---- the form ----
    (
        "a reminder needs no title",
        "        let title = self.title.text().trim();\n        if title.is_empty() {",
        "        let title = self.title.text().trim();\n        if false {",
        [WRONG],
    ),
    (
        "a changed due time keeps a stale snooze",
        "        if due != task.due {",
        "        if false {",
        [CHANGE],
    ),
    (
        "Enter in the step field saves the form",
        "                if field == TaskField::NewStep && form.add_step() {",
        "                if false && form.add_step() {",
        [KEPT],
    ),
    (
        "a step is not ticked in the form",
        "                        step.completed = !step.completed;",
        "                        let _ = &step;",
        [STEPS],
    ),
    (
        "a step is not taken off in the form",
        "                    form.steps.remove(form.step_at);",
        "                    let _ = form.step_at;",
        [STEPS],
    ),
    (
        "an imported repeat is not offered",
        "        out.extend(self.other_repeat.clone());\n",
        "",
        [CHANGE],
    ),
    (
        "a reminder out of sight is left out of sight",
        "            self.view = ViewFilter::All;",
        "            let _ = ViewFilter::All;",
        [WRONG],
    ),
    (
        "the form's keys reach the list",
        "            Event::Key(key_ev) if key_ev.pressed && self.form.is_some() => {",
        "            Event::Key(key_ev) if key_ev.pressed && self.form.is_some() && false => {",
        [MODAL],
    ),
    # ---- the list's keys ----
    (
        "Shift and a digit changes the view",
        "        if key.modifiers.shift\n            && let Some(index) = step_for_digit(key.key)",
        "        if false\n            && let Some(index) = step_for_digit(key.key)",
        [STEPS],
    ),
    (
        "N adds nothing",
        "            Key::N => {\n                self.open_new_task();",
        "            Key::N => {\n                let _ = 0;",
        [KEPT],
    ),
    (
        "Delete deletes without asking",
        "                Some(id) if self.store.get(id).is_some() => {\n                    self.pending_delete = Some(id);",
        "                Some(id) if self.store.get(id).is_some() => {\n                    self.delete_task(id);",
        [DELETE],
    ),
    (
        "Escape deletes",
        "            Key::Escape | Key::N => self.pending_delete = None,",
        "            Key::Escape | Key::N => self.delete_task(id),",
        [DELETE],
    ),
    (
        "a key reaches the list under the delete question",
        "            Event::Key(key_ev) if key_ev.pressed && self.pending_delete.is_some() => {",
        "            Event::Key(key_ev) if key_ev.pressed && self.pending_delete.is_some() && false => {",
        [DELETE],
    ),
    # ---- a repeating reminder ----
    (
        "a repeating reminder is finished for good",
        "            && let Some(next) = next_due_after(&task.recurrence, due, now)",
        "            && let Some(next) = next_due_after(&RecurrenceRule::None, due, now)",
        [REPEAT],
    ),
    (
        "a repeating reminder comes round at a time already past",
        "        if next > now {",
        "        if true {",
        [REPEAT],
    ),
    # ---- the strip ----
    (
        "why the list is not kept is not said",
        "            lines.push((error.clone(), true));\n",
        "",
        [NOTICE, BROKEN],
    ),
    (
        "the snooze question is drawn nowhere",
        "            lines.push((String::from(SNOOZE_PROMPT), false));\n",
        "",
        [NOTICE],
    ),
    (
        "an empty list does not say how to fill it",
        "        if self.store.is_empty() {\n            lines.push((String::from(NO_TASKS_LINE), false));",
        "        if false {\n            lines.push((String::from(NO_TASKS_LINE), false));",
        [EMPTY, NOTICE],
    ),
    (
        'AltGr is taken for Ctrl in the list',
        '        // not Ctrl held: AltGr arrives as Ctrl+Alt.\n        if textline::is_ctrl_chord(key.modifiers) {',
        '        // not Ctrl held: AltGr arrives as Ctrl+Alt.\n        if key.modifiers.ctrl {',
        ['a_chord_is_not_a_reminders_key_and_altgr_is_not_ctrl'],
    ),
    (
        "a chord works the list's keys",
        '        if !textline::is_plain(key.modifiers) {\n            return EventResult::Ignored;\n        }\n        // Shift and a digit',
        '        // Shift and a digit',
        ['a_chord_is_not_a_reminders_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord answers the question before a delete',
        '        if textline::is_plain(key.modifiers) {\n            match key.key {\n                Key::Enter | Key::Y',
        '        if !key.modifiers.shift || key.modifiers.shift {\n            match key.key {\n                Key::Enter | Key::Y',
        ['a_chord_is_not_a_reminders_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord answers the snooze prompt',
        '    fn handle_snooze_key(&mut self, key: &KeyEvent) -> EventResult {\n        if !textline::is_plain(key.modifiers) {\n            return EventResult::Ignored;\n        }\n',
        '    fn handle_snooze_key(&mut self, key: &KeyEvent) -> EventResult {\n',
        ['a_chord_is_not_a_reminders_key_and_altgr_is_not_ctrl'],
    ),
    (
        "Alt+Tab walks the form's fields",
        '            Key::Tab if plain => {',
        '            Key::Tab => {',
        ['a_chord_is_not_a_reminders_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord saves the form',
        '            Key::Enter if plain => {\n                if field == TaskField::NewStep',
        '            Key::Enter => {\n                if field == TaskField::NewStep',
        ['a_chord_is_not_a_reminders_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord closes the form',
        '            Key::Escape if plain => {\n                self.form = None;',
        '            Key::Escape => {\n                self.form = None;',
        ['a_chord_is_not_a_reminders_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chord works a field that is not typed into',
        '            _ if !plain && !field.is_text() => EventResult::Ignored,\n',
        '',
        ['a_chord_is_not_a_reminders_key_and_altgr_is_not_ctrl'],
    ),
    # -- the shortcut card is modal for the keys
    (
        "the card is modal for nothing",
        '        if self.show_help\n            && let Event::Key(key_ev) = event\n',
        '        if false\n            && let Event::Key(key_ev) = event\n',
        ['the_shortcut_card_takes_every_key_while_it_is_up'],
    ),
    (
        "a key that is not the card's acts behind it",
        '                return EventResult::Consumed;\n'
        '            }\n'
        '            return EventResult::Ignored;\n'
        '        }\n'
        '        match event {\n',
        '                return EventResult::Consumed;\n'
        '            }\n'
        '        }\n'
        '        match event {\n',
        ['the_shortcut_card_takes_every_key_while_it_is_up'],
    ),
    (
        "Escape does not put the card away",
        '            let closes = matches!(key_ev.key, Key::F1 | Key::Escape)\n',
        '            let closes = matches!(key_ev.key, Key::F1)\n',
        ['the_shortcut_card_takes_every_key_while_it_is_up'],
    ),
    (
        "? does not put the card away",
        '                || key_ev.key == Key::Slash && key_ev.modifiers.shift;\n',
        '                || false;\n',
        ['the_shortcut_card_takes_every_key_while_it_is_up'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "reminders", timeout=900, only=only))
