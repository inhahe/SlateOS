### [E] Notes, contacts, snippets and kanban keep nothing -- 2026-09-25
**Status:** FIXED -- notes, contacts and kanban (lane E, 2026-09-25), snippets (2026-09-26).

**In short:** the notes app, the address book, the snippet library and the
kanban boards each hold everything the user puts in them in memory only. There
is no store on disk: each opens empty (or, for snippets, on a built-in sample)
and forgets every note, contact, snippet and board the moment its window
closes. The one way to keep anything is an export -- the selected note as
Markdown, the whole book as vCard, a snippet as JSON, one board as JSON (Ctrl+E)
-- which the user has to remember to do, and which does not come back on the
next start. Asking "save your changes?" on close would be the wrong fix: nobody
expects to save a notes app.

**Where.** `apps/notes/src/main.rs` (`main`: "Until there is a store on disk
this is what there is to show"; `save_selected_note` is an export),
`apps/contacts/src/main.rs` (`ContactsApp::new`, `write_vcards`/`read_vcards`
are import and export), `apps/snippets/src/main.rs` (`App::new`, the JSON
export), `apps/kanban/src/main.rs` (`KanbanApp::boards`; `write_board` and
`read_board` export one board and import one as a new board).

**The proper fix** is the one `apps/stickynotes`, `apps/flashcards`,
`apps/finance` and `apps/habits` already use: the library is a file in the
settings directory, read at start and written -- atomically, through `safeio`
or `settingsfile` -- as it changes, with a failed write said on screen and the
record kept marked unsaved so the next change tries again. The exports stay as
exports. See `todo.txt` -> Lane E -> "The habit tracker keeps its record in the
settings directory" for why the settings directory rather than a data one.

**Notes, the same day.** The library is `notes/library.txt` in the settings
directory: every notebook and note with its tags, checklist, table, pins and
version history, tab-separated with `textfmt::tsv`'s escapes, rewritten after
every event that changed it and read whole or not at all (design-decisions
§1205). A failed save is drawn in red in the status bar and retried by the next
change or Ctrl+S; a close while it fails asks on `apps/unsaved`; a note being
written when the window closes is committed and kept, where it was dropped.
Ctrl+S, which was the Markdown export, now says where the notes are kept, and
the export is Ctrl+E. Also fixed on the way:
- **The empty window's text was never seen.** "No notes yet -- Ctrl+N makes
  one." and the line under it were drawn at the top of every frame, notes or
  not, and *before* the window's background, which painted over them. They are
  drawn in the editor, and only when there are no notes; the second line now
  says where notes are kept, or why they are not.
- **Timestamps were a counter** from 1000, shown as "Modified: 1004" and
  "v3 (1003)", and restarting at 1000 would have stamped every note made after
  a restart earlier than every kept one. They are the clock's, never earlier
  than a stamp already given, shown as `2026-09-25 14:03`.
- **Leaving a note unchanged changed it.** Escape out of the writing mode
  committed the body whether or not anything was typed, putting a copy of the
  same text in the history and moving the note to the top of the list; now
  the same text, title, notebook or name is no change.
Mutation table `apps/notes/mutate.py` (new, 26 rows, all caught).

**Contacts, the same day.** The address book is
`contacts/address-book.txt` in the settings directory: every contact with all
its numbers, addresses, accounts and groups, the groups with their colours,
and the recently viewed, in the notes library's kind of file, read whole or not
at all (design-decisions §1206). The store counts its own changes
(`ContactStore::revision`) and the window writes the book after any event that
moved the count; a failed save is drawn in red on the status line and retried;
closing over a contact being edited, or while a save fails, asks on
`apps/unsaved`. Ctrl+S says where the book is kept; export moved to Ctrl+E.
Also fixed on the way:
- **Saving an edit lost data.** The form shows the names, the work fields,
  the notes, the birthday and the *first* phone number, email address and
  postal address. Saving rebuilt the contact from the form and copied back
  four things (groups, the star, when it was added, when it was last reached),
  so every other number, address and account, a display name imported from a
  vCard, and the photo were dropped by the first edit -- beside a comment
  saying they must not be. The form is now written onto the contact; an
  emptied field removes what it showed; the display name follows the names
  only when it was theirs. Saving an unchanged form changes nothing.
- **Nothing had a time.** No contact was ever given one, so "recently added"
  sorted nothing, and "recently contacted" ran on a counter from
  2,000,000,000. Both are the clock's now, never earlier than a time already
  kept; an import is stamped as added now.
- **The vCard import read the whole file** into memory and cut it at 8 MiB
  afterwards; it reads through `safeio::read_to_string_capped`, which stops at
  the cap.
- **A contact could be put in a group that did not exist**
  (`add_contact_to_group` did not look); it cannot, since that would make the
  book a file that cannot be read back.
- The empty window said "Nothing is saved automatically -- press Ctrl+S to
  write a vCard file"; it says where the book is kept.
Mutation table `apps/contacts/mutate.py`: 31 rows added, and four of its
older rows moved to where the code now is.

**Kanban, the same day.** Every board is `kanban/boards.txt` in the settings
directory, the same kind of file, read whole or not at all; a card is written
once and each column lists its cards by id (design-decisions §1207). After
every key or click the window compares the boards' text with what it last
wrote, and writes it when they differ; a first run's starting board is not
written until something on it changes. A failed save is on the status line and
retried; closing while it fails asks. Also fixed:
- **A card made after an import could replace an imported one.** The import
  keeps the ids its file carries, and did not move the id counter past them,
  so the next card made could be given the id of one just read -- and cards
  are kept in a map by id. Every id read moves the counter now
  (`Id::from_stored`).
- **An import could hold a card in two columns**, or a column naming a card
  the board does not have; it drops them, keeping the first place a card is
  named.
- **What an import or an export did was drawn nowhere** (`last_file_action`,
  "for the status line", which there was not). There is a status line now.
- **The empty board's two lines were never seen**: drawn at the top of the
  window, on every board, before the toolbar, which painted over them -- and
  pinned by a test that read the command list rather than the screen. The
  status line says how to start and where boards are kept; the test now also
  asks that nothing drawn after it covers it.
- Card times were a counter from 1000; they are the clock's, never earlier
  than a time already kept.
Mutation table `apps/kanban/mutate.py` (new).

**Snippets, 2026-09-26.** A snippet can be written now: F2, the new Edit
button, or making one (N, which still names it from the search box) opens it
in the column it is shown in -- title, language, folder, tags, a description
and the code itself, in the fixed-pitch face it is shown in, Tab indenting
there (`apps/textarea`, which learned to measure in that face). Ctrl+S saves;
Escape over changes asks before throwing them away. The library is
`snippets/library.txt` in the settings directory, written after every change
and read whole or not at all (design-decisions §1211); a first run still opens
on the examples, which are not written until something changes. Also fixed:
Delete deleted at once, with no undo -- it asks now; a snippet's time was its
id, which restarted with every window -- it is the clock's; and the JSON
export was written with `fs::write`, which truncates before writing -- it is
atomic now.
