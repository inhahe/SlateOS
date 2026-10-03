### [E] Two windows of one program: the last to save throws away what the other saved -- 2026-09-28

**Status:** OPEN -- which fix is `open-questions.md` E-Q5.

**In short:** Many programs keep their data in a file of their own -- notes,
contacts, the e-book reader's library, sticky notes, reminders. Each window
reads the file when it opens and, when something changes (the e-book reader:
when it closes), writes its own copy back whole. Nothing stops a program being
opened twice, and neither window sees what the other changed, so whichever
saves last writes its copy over the other's. A note made in one window is gone
as soon as the other window saves.

**Reproduce:** open Notes twice. Make a note in the first window, then one in
the second. Close both and open Notes again: only the second window's note is
there. The e-book reader the same way: open a different book in each of two
readers and close both -- the library holds only the books of the one closed
last, and a place reached in the other is lost.

**Where.** Every program under `apps/` that keeps a file under
`settingsfile::config_dir()` -- a grep finds seventeen: `backup`,
`calendarstore` (the calendar's store), `contacts`, `credmanager`, `ebook`,
`email`, `finance`, `flashcards`, `kanban`, `notes`, `photomanager`,
`pinball`, `reminders`, `rssreader`, `snippets`, `stickynotes`,
`systemrestore`. Read and confirmed in `ebook` (`EbookApp::keep` writes the
window's library), `notes` (`NotesApp::keep`, `library_text` of the window's
notebooks and notes) and `stickynotes` (`persist`, `write_store` of the
window's store); the rest to be confirmed program by program as they are
fixed -- one that appends rather than rewrites is not affected.

**Not this:** a program's settings file (`<config>/<name>.yaml`). Since
2026-09-28 a change to one is announced to every window
(`requests/c-e-a-changed-settings-file-is-announced-now.md`, §1434) and the
programs that keep one read it again when told, which closes the same gap for
settings. The data files above are not settings files, are not announced, and
are not read again.

**The fix** is the operator's choice (E-Q5): one window per program, a second
launch bringing the first forward; or two windows allowed, each reading the
file again before it writes and writing only its own change into it.
