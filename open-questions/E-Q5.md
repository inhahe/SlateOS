## E-Q5 — [E] Opening a program twice can lose data. Should a program have only one window, or should its windows share their changes? — Status: OPEN (raised 2026-09-28)

**In short:** Notes, contacts, the e-book reader's library, sticky notes,
reminders and about a dozen more programs each keep their data in one file.
Every window of such a program reads that file when it opens and later writes
its own copy back whole. If you open the same program twice, neither window
knows what the other did, so whichever saves last wipes out the other's
changes -- a note made in one window disappears when the other saves. The
question is how to stop that, because the answers differ in what you can do
with two windows.

| Option | *What changes* for the user | What it needs | Cost / risk |
|---|---|---|---|
| **A. One window per program.** Starting a program that is already running brings its window to the front instead of opening a second. | You can no longer have, say, two Notes windows side by side. Nothing is ever lost. | A way for a program to find its running copy and hand it the request (lanes C and F: the launcher and the window system), and each program opting in. | Simplest and airtight. Loses side-by-side use, which matters most for notes and contacts. |
| **B. Several windows, each writing only its own change.** Before saving, a window reads the file again and puts its one change -- a note added, a contact edited -- into what is there now, instead of writing its whole copy. The other windows are told the file changed and show it. | Two windows work side by side and both keep what is done in them. If both edit the *same* note at once, the later save wins for that note only. | Lane E, program by program: each keeps an id per record and saves record by record; a notice when a data file changes, like the one settings files have now. | Most work (about seventeen programs), and each needs its own tests for two windows. The one lasting cost is the same-note case. |
| **C. The second window opens read-only.** It shows the data and says "open in another window -- changes here are not saved" until the first closes. | You can look at the data twice but change it in one window only. | A lock on the data file, taken by the first window and released when it closes; each program's editing controls honour it. | Middle ground. A window that crashes can leave the lock behind, which needs a stale-lock rule. |

**Recommendation:** B for the programs where two windows are genuinely useful
(notes, contacts, kanban, snippets, the e-book reader, reminders, flashcards,
finance), and A for the rest -- a backup program or System Restore opened twice
is a mistake rather than a way of working. B is what the settings files already
do since this week (a change is announced and every window reads it again), so
it extends a mechanism that exists rather than adding a second one.

**If never answered:** nothing gets worse than today, but nothing gets better:
anyone who opens one of these programs twice can lose what they did in one of
the windows, silently. Nothing else is blocked.

**Where:** `known-issues.md` "[E] Two windows of one program: the last to save
throws away what the other saved"; each program's save -- `EbookApp::keep`
(`apps/ebook`), `NotesApp::keep` (`apps/notes`), `persist` (`apps/stickynotes`).
