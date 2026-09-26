"""Mutation test for the calendar's kept events, the event form, and asking
before an event -- or the calendar's latest changes -- is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

There was no way to add an event: the store's `add` had no caller but the
tests and the `.ics` import, nothing could change or delete one, and nothing
was kept -- an event imported today was gone when the window closed.  The
table covers what replaced that; the rest of the suite predates it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

KEPT = "an_event_is_added_in_the_form_and_is_there_next_time"
QUIET = "a_window_made_by_new_keeps_nothing"
ROUND = "the_kept_calendar_reads_back_what_it_wrote_whatever_the_text"
REFUSED = "a_calendar_that_cannot_be_read_whole_is_refused_and_says_why"
BROKEN = "a_calendar_file_that_cannot_be_read_is_left_as_it_is"
BIG = "a_calendar_file_too_big_to_read_whole_is_refused"
FAILING = "closing_while_a_save_fails_asks_first"
COUNTS = "the_store_counts_its_changes_and_nothing_else"
WRONG = "the_form_says_what_is_wrong_and_keeps_what_was_typed"
ALLDAY = "an_all_day_event_has_no_times_to_fill_in"
CHANGE = "enter_changes_the_selected_event_and_keeps_its_number"
REPEATS = "repeats_step_through_the_list_and_keep_an_imported_one"
DELETE = "delete_asks_first_and_each_answer_does_what_it_says"
MODAL = "the_form_takes_every_key_and_press_while_it_is_up"
SECOND = "a_second_press_on_an_event_opens_it"
SEARCH = "a_search_finds_accents_in_any_case_and_places"
NOTICE = "the_warning_lines_are_not_painted_over"
EMPTY = "a_fresh_calendar_holds_no_events_and_says_how_to_add_one"
BUTTON = "the_new_event_button_gives_way_to_the_view_tabs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ---- the store counts its changes ----
    (
        "an add is not counted",
        "        self.events.push(event);\n        self.changed();\n        id",
        "        self.events.push(event);\n        id",
        [COUNTS, KEPT],
    ),
    (
        "a remove is not counted",
        "        if removed {\n            self.changed();\n        }\n",
        "",
        [COUNTS],
    ),
    (
        "removing nothing is counted",
        "        let removed = self.events.len() < len_before;\n        if removed {",
        "        let removed = self.events.len() < len_before;\n        if true {",
        [COUNTS],
    ),
    (
        "a change through get_mut is not counted",
        "        let at = self.events.iter().position(|e| e.id == id)?;\n        self.changed();\n",
        "        let at = self.events.iter().position(|e| e.id == id)?;\n",
        [COUNTS],
    ),
    (
        "an import is not counted",
        "        if count > 0 {\n            self.changed();\n        }\n",
        "",
        [COUNTS],
    ),
    (
        "an import of nothing is counted",
        "        if count > 0 {\n            self.changed();",
        "        if true {\n            self.changed();",
        [COUNTS],
    ),
    # ---- the file ----
    (
        "the file loses the title",
        "            tsv::escape(&e.title),",
        "            String::new(),",
        [ROUND, KEPT],
    ),
    (
        "the notes are written unescaped",
        "            tsv::escape(&e.description),",
        "            e.description.clone(),",
        [ROUND],
    ),
    (
        "the file loses the colour",
        '                .map_or_else(|| String::from("-"), colour_text),',
        '                .map_or_else(|| String::from("-"), |_| String::from("-")),',
        [ROUND],
    ),
    (
        "the file loses the reminder",
        "            reminder_key(e.reminder),",
        "            reminder_key(Reminder::None),",
        [ROUND],
    ),
    (
        "the file loses the repeat",
        "            repeat_key(&e.recurrence),",
        "            repeat_key(&RecurrenceRule::None),",
        [ROUND],
    ),
    (
        "two events with one number are read",
        "        if events.iter().any(|e| e.id == id) {",
        "        if false {",
        [REFUSED],
    ),
    (
        "a weekday outside the week is read",
        "                    .map(|d| d.parse::<u32>().ok().filter(|d| *d < 7))",
        "                    .map(|d| d.parse::<u32>().ok())",
        [REFUSED],
    ),
    (
        "a later format is taken for someone else's",
        '        Some(first) if first.starts_with("slateos-calendar\\t") => {',
        "        Some(first) if false && first.is_empty() => {",
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
        "an unreadable file is saved over",
        "            Err(why) => {\n                self.persist = false;\n                self.store_error = Some(refused(why));",
        "            Err(why) => {\n                self.store_error = Some(refused(why));",
        [BROKEN],
    ),
    (
        "nothing is written",
        "        if !self.persist || self.store.revision() == self.kept_revision {",
        "        if true {",
        [KEPT],
    ),
    (
        "a window made by new keeps its events",
        "        if !self.persist || self.store.revision() == self.kept_revision {",
        "        if self.store.revision() == self.kept_revision {",
        [QUIET],
    ),
    (
        "a save that worked is not remembered",
        "            Ok(()) => {\n                self.kept_revision = self.store.revision();\n                self.store_error = None;",
        "            Ok(()) => {\n                self.store_error = None;",
        [FAILING],
    ),
    (
        "the events are not kept after an event",
        "    let result = route_event(state, event);\n    state.keep();\n",
        "    let result = route_event(state, event);\n",
        [KEPT],
    ),
    # ---- the close question ----
    (
        "a window whose save fails closes without asking",
        "        if !self.unkept() {\n            return true;\n        }",
        "        if true {\n            return true;\n        }",
        [FAILING],
    ),
    (
        "Save closes although the save failed",
        "                self.running = self.unkept();",
        "                self.running = false;",
        [FAILING],
    ),
    (
        "Don't save keeps the window open",
        "            Choice::Discard => self.running = false,",
        "            Choice::Discard => {}",
        [FAILING],
    ),
    (
        "the question is not drawn",
        "            question.render(&palette, width, height, &mut tree);\n",
        "",
        [FAILING],
    ),
    (
        "a key reaches the calendar under the question",
        "            state.answer(choice);\n        }\n        return EventResult::Consumed;\n",
        "            state.answer(choice);\n        }\n",
        [FAILING],
    ),
    # ---- the form ----
    (
        "an event needs no title",
        "        if title.is_empty() {",
        "        if false {",
        [WRONG],
    ),
    (
        "an event may end before it starts",
        "            if ends < starts {",
        "            if false {",
        [WRONG],
    ),
    (
        "an all-day event still needs its times",
        "        let (start, end) = if self.all_day {",
        "        let (start, end) = if false {",
        [ALLDAY],
    ),
    (
        "all day does not toggle",
        "            FormField::AllDay => self.all_day = !self.all_day,",
        "            FormField::AllDay => {}",
        [ALLDAY],
    ),
    (
        "a changed event loses its number",
        "                    *kept = CalendarEvent { id, ..event };",
        "                    *kept = CalendarEvent {\n                        id: id.wrapping_add(1000),\n                        ..event\n                    };",
        [CHANGE],
    ),
    (
        "a new event is not selected",
        "                let id = self.store.add(event);\n                self.selected_event_id = Some(id);\n",
        "                let id = self.store.add(event);\n                let _ = id;\n",
        [KEPT],
    ),
    (
        "an imported repeat is not offered",
        "        out.extend(self.other_repeat.clone());\n",
        "",
        [REPEATS],
    ),
    (
        "an event's odd repeat is forgotten by its form",
        "            other_repeat: (!standard_repeats().contains(&e.recurrence))",
        "            other_repeat: (false && !standard_repeats().contains(&e.recurrence))",
        [REPEATS],
    ),
    (
        "the form's keys reach the calendar",
        "            Event::Key(key) if key.pressed => return handle_form_key(state, key),\n",
        "",
        [MODAL],
    ),
    (
        "a press beside the form reaches the calendar",
        "                let hit = state.target_at(mouse.x, mouse.y);\n                return handle_form_click(state, hit);",
        "                let hit = state.target_at(mouse.x, mouse.y);\n                let _ = hit;",
        [MODAL],
    ),
    # ---- the calendar's keys and presses ----
    (
        "Enter opens nothing",
        "            Some(id) if state.store.get(id).is_some() => {\n                state.open_edit_event(id);",
        "            Some(id) if state.store.get(id).is_some() => {\n                let _ = id;",
        [CHANGE],
    ),
    (
        "Delete deletes without asking",
        "            Some(id) if state.store.get(id).is_some() => {\n                state.pending_delete = Some(id);",
        "            Some(id) if state.store.get(id).is_some() => {\n                state.delete_event(id);",
        [DELETE],
    ),
    (
        "Escape deletes",
        "        Key::Escape | Key::N => state.pending_delete = None,",
        "        Key::Escape | Key::N => state.delete_event(id),",
        [DELETE],
    ),
    (
        "Keep it deletes",
        "                    Some(Target::KeepEvent) => state.pending_delete = None,",
        "                    Some(Target::KeepEvent) => state.delete_event(id),",
        [DELETE],
    ),
    (
        "a key reaches the calendar under the delete question",
        "            Event::Key(key) if key.pressed => return handle_confirm_key(state, id, key),\n",
        "",
        [DELETE],
    ),
    (
        "a second press on an event does not open it",
        "                    if state.selected_event_id == Some(id) {\n                        state.open_edit_event(id);",
        "                    if false {\n                        state.open_edit_event(id);",
        [SECOND],
    ),
    (
        "the New event button does nothing",
        "                Some(Target::NewEvent) => state.open_new_event(),",
        "                Some(Target::NewEvent) => {}",
        [MODAL, BUTTON],
    ),
    (
        "the New event button crowds the tabs",
        "        let new_event_button = (width - 8.0 - MIN_VIEW_TAB_PITCH * count >= new_right + 8.0)",
        "        let new_event_button = (width >= 0.0)",
        [BUTTON],
    ),
    # ---- search and the notice strip ----
    (
        "a search folds only ASCII",
        "                e.title.to_lowercase().contains(&lower)",
        "                e.title.to_ascii_lowercase().contains(&lower)",
        [SEARCH],
    ),
    (
        "a search skips the place",
        "                        .is_some_and(|l| l.to_lowercase().contains(&lower))",
        "                        .is_some_and(|_| false)",
        [SEARCH],
    ),
    (
        "why the events are not kept is not said",
        "            lines.push((error.clone(), true));\n",
        "",
        [NOTICE, BROKEN, FAILING],
    ),
    (
        "an empty calendar does not say how to fill it",
        "        if self.store.is_empty() {\n            lines.push((String::from(NO_EVENTS_LINE), false));",
        "        if false {\n            lines.push((String::from(NO_EVENTS_LINE), false));",
        [EMPTY, NOTICE],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "calendar", timeout=900, only=only))
