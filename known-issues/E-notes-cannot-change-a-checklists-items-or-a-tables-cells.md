### [E] Notes cannot change a checklist's items or a table's cells -- 2026-09-28

**Status:** open. Writing in them is refused, and says why, since 2026-09-28.

**In short:** the notes app can show a checklist note and a table note, but
nothing in its window can make one, tick an item, add or remove an item, or
change a cell. Ctrl+N makes a plain note, and the only checklists and tables
a user can have are ones in a library written before the sample content went
test-only (2026-09-15). Until 2026-09-28, pressing Enter on one opened its
hidden `content` for writing: what was typed was drawn nowhere once the
writing finished, and stayed in the library and in every search. That is
refused now (`not_written_as_text`, `apps/notes/src/main.rs`), with the
reason on the status line.

**The proper fix.** A checklist edited item by item: a click on the box ticks
it (`Note::toggle_checklist_item` exists and has no caller), each item's
words in a one-line field, Enter adding the next item and Backspace on an
empty one removing it. A table edited a cell at a time, with Tab moving along
the row. And a way to make each kind -- a kind switch on the note, or the
templates `create_note_from_template` already has and nothing offers.
