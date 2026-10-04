"""Mutation test for the notes library: what is kept, how it is read back, and
what happens when keeping fails.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The app kept nothing: every note was held in memory and gone when the window
closed, and the one way out was exporting the selected note.  The table covers
the library it keeps now (design-decisions §1205), the stamps that order it,
and the close; the rest of the suite predates them.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "a_library_written_and_read_again_is_the_same_library"
REFUSED = "a_library_that_cannot_be_read_whole_is_refused_and_says_where"
KEPT = "what_is_written_is_there_next_time"
MARKED = "every_change_is_marked_for_keeping"
FAILING = "a_save_that_fails_says_so_and_the_next_change_tries_again"
ASKS = "closing_while_a_save_fails_asks_first"
LEAVES = "closing_over_a_failing_save_can_leave_without_it"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a save writes nothing",
        "            .and_then(|()| safeio::write_str_atomically(&path, &text));",
        "            .and_then(|()| Ok::<(), std::io::Error>(()));",
        [KEPT, "deleting_is_kept_too", "closing_while_writing_keeps_what_was_written"],
    ),
    (
        "the file loses the tags",
        r'            out.push_str(&format!("tag\t{}\n", tsv::escape(tag)));' "\n",
        "",
        [ROUND, KEPT],
    ),
    (
        "the file loses the ticks on a checklist",
        "                flag(item.checked),",
        "                flag(false),",
        [ROUND],
    ),
    (
        "the file loses a table's cells",
        "                push_fields(&mut out, row);\n",
        "",
        [ROUND],
    ),
    (
        "the file loses the history",
        "        for version in &note.versions {",
        "        for version in note.versions.iter().take(0) {",
        [ROUND],
    ),
    (
        "a later format is half-read",
        "    if version > LIBRARY_FORMAT {",
        "    if version > LIBRARY_FORMAT + 100 {",
        [REFUSED],
    ),
    (
        "a note in a notebook that is not there is read",
        "        if !notebook_ids.contains(&note.notebook_id) {",
        "        if false {",
        [REFUSED],
    ),
    (
        "a notebook inside itself is read",
        '                return Err(format!("line {at}: a notebook is inside itself"));',
        "                break;",
        [REFUSED],
    ),
    (
        "two notes with one number are both taken",
        "                if !note_ids.insert(id) {",
        "                if !note_ids.insert(id) && false {",
        [REFUSED],
    ),
    (
        "a file too big to read whole is read in part",
        "        if read.truncated {",
        "        if false {",
        ["a_library_too_big_to_read_whole_is_refused"],
    ),
    (
        "a refused library is saved over",
        "            Err(why) => {\n                self.persist = false;\n",
        "            Err(why) => {\n",
        ["a_library_that_cannot_be_read_is_left_as_it_is"],
    ),
    (
        "a stamp after a restart is earlier than the kept ones",
        "        self.last_stamp = stamps.fold(self.last_stamp, u64::max);\n",
        "",
        ["a_change_made_now_is_later_than_every_kept_one"],
    ),
    (
        "a stamp is only the clock",
        "        self.last_stamp = clock_ms().max(self.last_stamp.saturating_add(1));",
        "        self.last_stamp = clock_ms();",
        ["a_change_made_now_is_later_than_every_kept_one"],
    ),
    (
        "an event's change is not written",
        "        let result = self.route_event(event);\n        self.keep();\n",
        "        let result = self.route_event(event);\n",
        [KEPT, FAILING],
    ),
    (
        "a failed save is not said",
        '                self.store_error = Some(format!("Not saved to {}: {err}", path.shown()));',
        "                drop(err);",
        [FAILING],
    ),
    (
        "the failure is drawn nowhere",
        "        if let Some(error) = &self.store_error {\n            cmds.push(",
        "        if let Some(error) = None::<&String> {\n            cmds.push(",
        [FAILING],
    ),
    (
        "the same text again is a change",
        "        if note.content == new_content {\n            return true;\n        }\n",
        "",
        ["leaving_a_note_as_it_was_is_no_change"],
    ),
    (
        "a pin is not kept",
        "        f(note);\n        self.after_change();\n",
        "        f(note);\n",
        [MARKED],
    ),
    (
        "a restored version is not kept",
        "        if restored {\n            self.after_change();\n        }\n",
        "",
        [MARKED],
    ),
    (
        "a deleted note comes back",
        "        if deleted {\n            self.after_change();\n",
        "        if deleted {\n",
        [MARKED, "deleting_is_kept_too"],
    ),
    (
        "the window closes over a failing save",
        "        if !(self.persist && self.unsaved) {",
        "        if true {",
        [ASKS, LEAVES],
    ),
    (
        "keys reach the window under the question",
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "            && false",
        [ASKS, LEAVES],
    ),
    (
        "Save leaves while the save still fails",
        "                self.quit = !self.unsaved;",
        "                self.quit = true;",
        [ASKS],
    ),
    (
        "what is being written is dropped at a close",
        "        self.finish_note_body();\n        self.keep();\n        if !(self.persist",
        "        self.keep();\n        if !(self.persist",
        ["closing_while_writing_keeps_what_was_written"],
    ),
    (
        "a new note goes in a notebook that is not there",
        "            && self.find_notebook(id).is_some()\n",
        "",
        ["a_new_note_goes_in_a_notebook_the_library_has"],
    ),
    (
        "the empty window says nothing",
        "        } else if self.notes.is_empty() {",
        "        } else if false {",
        ["the_empty_window_says_how_to_start_and_nothing_else_does", KEPT],
    ),
    # The body is the toolkit's multi-line field
    # (requests/c-e-a-multi-line-text-field-for-the-apps-that-edit-text.md).
    (
        "a press elsewhere keeps the writing before it lands",
        "        let result = self.press_elsewhere(event);\n        if ends_writing {\n"
        "            self.finish_note_body();\n            return EventResult::Consumed;\n"
        "        }\n        result\n",
        "        if ends_writing {\n            self.finish_note_body();\n        }\n"
        "        self.press_elsewhere(event)\n",
        ["a_click_elsewhere_keeps_the_writing_and_does_its_own_thing"],
    ),
    (
        "a press elsewhere goes on writing",
        "        let ends_writing = self.writing().is_some()\n",
        "        let ends_writing = false && self.writing().is_some()\n",
        ["a_click_elsewhere_keeps_the_writing_and_does_its_own_thing"],
    ),
    (
        "the version panel ends the writing",
        "            && !self.on_version_panel(event.x, event.y);",
        "            && true;",
        ["a_version_clicked_while_writing_goes_into_the_field"],
    ),
    (
        "a version is restored past the field",
        "            && if self.writing().is_some() {\n                self.restore_into_body(index)",
        "            && if false {\n                self.restore_into_body(index)",
        ["a_version_clicked_while_writing_goes_into_the_field"],
    ),
    (
        "an untouched field is written back",
        "        if untouched || !self.update_note_content(id, &text) {",
        "        if !self.update_note_content(id, &text) {",
        ["opening_a_note_with_other_line_endings_changes_nothing"],
    ),
    (
        "the field is filled again on every event",
        "            .is_some_and(|held| held.id == note.id && held.source == note.content)",
        "            .is_some_and(|held| held.id == note.id && held.source == note.content && false)",
        ["a_long_note_scrolls_and_stays_in_its_panel"],
    ),
    (
        "a note is shown through a field holding its old text",
        "            .is_some_and(|held| held.id == note.id && held.source == note.content)",
        "            .is_some_and(|held| held.id == note.id)",
        ["a_version_restored_while_reading_shows_at_once"],
    ),
    (
        "a click does not put the caret where it lands",
        "                self.body\n                    .press(event.x - left, event.y - top, clicks, false, &m);",
        "                let _ = (left, top, clicks, &m);",
        ["a_click_on_the_text_writes_where_it_lands", "a_drag_selects_the_text_it_passes_over"],
    ),
    (
        "a drag does not select",
        "                self.body.drag_to(event.x - left, event.y - top, &m);",
        "                let _ = (left, top, &m);",
        ["a_drag_selects_the_text_it_passes_over"],
    ),
    (
        "a move with the button up selects",
        "            MouseEventKind::Release(MouseButton::Left) if self.body_drag => {\n"
        "                self.body_drag = false;",
        "            MouseEventKind::Release(MouseButton::Left) if self.body_drag => {\n"
        "                self.body_drag = true;",
        ["a_drag_selects_the_text_it_passes_over"],
    ),
    (
        "a double click is one click",
        "                let clicks = if matches!(event.kind, MouseEventKind::DoubleClick(_)) {\n"
        "                    2",
        "                let clicks = if matches!(event.kind, MouseEventKind::DoubleClick(_)) {\n"
        "                    1",
        ["a_double_click_selects_a_word"],
    ),
    (
        "a click on a Markdown page lands in its source",
        "                    if markdown {",
        "                    if false {",
        ["a_click_on_a_markdown_page_writes_at_its_end"],
    ),
    (
        "Tab types nothing",
        '                self.body.insert_str("\\t");',
        "",
        ["tab_writes_a_tab_in_a_note", KEPT],
    ),
    (
        "Ctrl+Enter does not finish",
        "            Key::Enter if ctrl => {\n                self.finish_note_body();",
        "            Key::Enter if false => {\n                self.finish_note_body();",
        ["ctrl_enter_finishes_writing"],
    ),
    (
        "Ctrl+S in the middle of writing keeps the old text",
        "            Key::S if ctrl => {\n                self.commit_note_body();",
        "            Key::S if ctrl => {",
        ["ctrl_s_while_writing_keeps_what_is_written"],
    ),
    (
        "a checklist is opened as text",
        "        NoteKind::Checklist => Some(CHECKLIST_NOT_TEXT),",
        "        NoteKind::Checklist => None,",
        ["a_checklist_or_a_table_is_not_opened_as_text"],
    ),
    (
        "the counts sit still while writing",
        "        let (word_count, char_count, link_count) = if self.writing() == Some(note.id) {",
        "        let (word_count, char_count, link_count) = if false {",
        ["the_counts_follow_the_writing"],
    ),
    (
        "bytes are counted as characters",
        "    text.chars().count()\n}",
        "    text.len()\n}",
        ["characters_are_counted_not_bytes", "the_counts_follow_the_writing"],
    ),
    (
        "the caret's width setting is ignored",
        "        self.caret_width = settings.caret_width();",
        "        let _ = settings;",
        ["the_caret_is_as_wide_as_the_setting_says"],
    ),
    (
        "a note being read shows a caret",
        "                focused: writing,",
        "                focused: true,",
        ["the_caret_is_as_wide_as_the_setting_says"],
    ),
    (
        "a Markdown note is written through its page",
        "        if self.writing() == Some(note.id) {\n            // What is written",
        "        if false {\n            // What is written",
        ["a_markdown_note_is_written_as_its_source"],
    ),
    (
        "a long page is not kept in its panel",
        "        cmds.push(RenderCommand::PushClip {\n            x,\n            y: editor_y,",
        "        drop(RenderCommand::PushClip {\n            x,\n            y: editor_y,",
        ["a_long_markdown_page_stays_in_its_panel"],
    ),
    (
        "an export is the title and the content",
        "        let body = export_markdown(note);",
        '        let body = format!("# {}\\n\\n{}\\n", note.title, note.content);',
        ["an_export_holds_a_checklists_items_a_tables_rows_and_the_tags"],
    ),
    (
        "a checklist exports no items",
        "                items.push_str(&item.text);",
        "                let _ = &item.text;",
        ["an_export_holds_a_checklists_items_a_tables_rows_and_the_tags"],
    ),
    (
        "an empty note exports a blank body",
        "    if !body.is_empty() {\n        out.push('\\n');",
        "    if true {\n        out.push('\\n');",
        ["an_export_holds_a_checklists_items_a_tables_rows_and_the_tags"],
    ),
    (
        "a key held with Alt or the Windows key is a shortcut",
        "        if key.modifiers.alt || key.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        let ctrl = key.modifiers.ctrl;",
        "        let ctrl = key.modifiers.ctrl;",
        [
            "altgr_types_into_a_field_and_runs_no_shortcut",
            "a_command_types_nothing_and_alt_or_windows_is_no_shortcut",
        ],
    ),
    (
        "a key held with the Windows key is a shortcut",
        "        if key.modifiers.alt || key.modifiers.super_key {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        let ctrl = key.modifiers.ctrl;",
        "        if key.modifiers.alt {\n"
        "            return EventResult::Ignored;\n        }\n"
        "        let ctrl = key.modifiers.ctrl;",
        ["a_command_types_nothing_and_alt_or_windows_is_no_shortcut"],
    ),
    (
        "a field refuses what AltGr types",
        "                if !textline::types_into_field(key) {",
        "                if !textline::types_into_field(key) || key.modifiers.ctrl {",
        ["altgr_types_into_a_field_and_runs_no_shortcut"],
    ),
    (
        "a field types a command's letter",
        "                if !textline::types_into_field(key) {",
        "                if !key.types_text() {",
        ["a_command_types_nothing_and_alt_or_windows_is_no_shortcut"],
    ),
    # -- the shortcut card's hold on the pointer
    (
        "a press goes through the shortcut card",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n"
        "                    return EventResult::Consumed;\n"
        "                }\n",
        "",
        ["the_shortcut_card_takes_a_press_rather_than_passing_it_on"],
    ),
    (
        "only the left button puts the card away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        ["the_shortcut_card_takes_a_press_rather_than_passing_it_on"],
    ),
    (
        "the wheel scrolls the note the card covers",
        "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        ["the_wheel_scrolls_no_note_under_the_card"],
    ),
]

# A name is asked for in the toolkit's input dialog -- a tag, a notebook's
# new name, a new note's title, a new notebook's name -- where it was typed
# blind into a line under the toolbar (2026-10-04; lane C,
# c-e-a-theme-can-shape-the-controls).
NAMED = "a_new_note_and_notebook_are_named_in_the_input_dialog"
NO_COMMAND = "a_command_is_not_typed_into_the_name_dialog"
RENAMED = "a_notebook_can_be_renamed"
TAGGED = "a_tag_typed_from_the_menu_reaches_the_note"
TAG_SHOWN = "the_tag_being_typed_is_shown"

MUTATIONS += [
    (
        "a command is typed into the name dialog",
        "        if let Event::Key(key) = event\n"
        "            && textline::is_command(key.modifiers)\n"
        "        {\n"
        "            return EventResult::Consumed;\n"
        "        }\n",
        "",
        [NO_COMMAND],
    ),
    (
        "the name dialog does not have the keys",
        "        if self.asking.is_some() && matches!(event, Event::Key(_) | Event::Mouse(_)) {\n",
        "        if false {\n",
        [NAMED, NO_COMMAND],
    ),
    (
        "the name dialog is not drawn",
        "            dialog.render(&palette, width, height, &mut tree);\n",
        "            let _ = dialog;\n",
        [NAMED, TAG_SHOWN],
    ),
    (
        "Ctrl+N asks for no title",
        "                self.ask(Asking::NewNote, \"\");\n",
        "",
        [NAMED],
    ),
    (
        "an answered tag goes nowhere",
        "                Asking::Tag(id) => self.commit_tag(id, &typed),\n",
        "                Asking::Tag(_) => {}\n",
        [TAGGED],
    ),
    (
        "a renamed notebook keeps its old name",
        "                Asking::NotebookName(id) => self.commit_notebook_name(id, &typed),\n",
        "                Asking::NotebookName(_) => {}\n",
        [RENAMED],
    ),
    (
        "a rename starts from nothing",
        "            self.ask(Asking::NotebookName(id), &current);\n",
        "            self.ask(Asking::NotebookName(id), \"\");\n",
        [RENAMED],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "notes", timeout=900, only=only))
