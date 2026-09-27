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
        '                self.store_error = Some(format!("Not saved to {}: {err}", path.display()));',
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
        "        if let Some(TextEntry::NoteBody(id, body)) = self.text_entry.clone() {",
        "        if let Some(TextEntry::NoteBody(id, body)) = None::<TextEntry> {",
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
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "notes", timeout=900, only=only))
