## 1241. Two windows of a program merge record by record, and each program mends what a merge leaves

**Date:** 2026-10-09 · **Decided by:** Claude (operator-approved scope: §1239 chose two windows where they are useful, each saving only its own changes; how a save does that, and what each program does where the two windows' changes meet, are Claude's) · **Lane:** E

**In short:** With two windows of Notes, Contacts, the kanban board,
Snippets, Reminders, the e-book reader, Flashcards or Finance open, each
window's save used to
write its own copy over the file, so the window that saved last threw away
the other's work. Now a save reads the file first and adds only what this
window changed, one record (a note, a card, a book) at a time; and a window
reads the file again when the other saves. Where both windows changed the
same record, the later save wins for that record alone. This records the
rules of that merge and, program by program, what is done when the merge
leaves something pointing at nothing -- a note whose notebook the other
window deleted, a card in two columns.

**The merge** (`apps/recordfile`, `merge` and `merge_by`). Each record has a
key -- a random id below 2^52 (so it survives a JSON number), or for the
e-book reader's books the book's file. A save compares the base (the file as
this window last read or wrote it), mine (the window now) and theirs (the
file now):

- changed or added here -> mine's, over the other window's change to it and
  over its deletion of it (an edit here beats a deletion elsewhere);
- deleted here -> gone;
- anything else -> as the file has it, so what the other window added,
  changed and deleted stands;
- an order changed here is kept; otherwise the file's, with this window's
  additions after their predecessor and after the other window's additions
  at the same place.

**The file.** A save that finds the file as this window last left it (same
length, time and inode) does not read it. A file that cannot be read at
save time -- broken, another program's -- is not written over: the save is
refused and says why. A file that is not there reads as the base, not as an
empty file: nothing has been deleted from it, so a data file removed while a
window is open is written back whole rather than emptied of everything the
window did not change. A first run's base is what the window starts on --
Snippets' examples, the kanban board's starting board -- and that content has
fixed ids, so two windows opened on a first run have the same examples rather
than one copy each.

**What each program mends.** A merge taken record by record can leave what
the program's own reader refuses, and a file written so could not be read
again by any window. Each program mends it before writing:

| Program | Left by a merge | Mended to | Why this and not the other way |
|---|---|---|---|
| Notes | a note in a notebook the other window deleted (either window) | the notebook comes back, from this window's copy or the file's | a note must be in a notebook; deleting a notebook deletes its notes, so the note would otherwise be lost or unreadable |
| Contacts | a contact in a group the other window deleted | the group comes back from this window's copy if it has it; otherwise the membership goes | a contact is whole without the group; a group this window deleted stays deleted |
| Kanban | a card in two columns | this window's column -- the later save | |
| Kanban | a card in a column and archived (one window archived it, the other moved it) | archived: the card's own record says | it is restorable from the archive; the file reader refuses a card in two places |
| Kanban | a card whose column the other window deleted | this window's column for it, else the first column -- or archived when the board has no column left | a card must be somewhere it can be seen |
| Snippets | a snippet or folder in a folder the other window deleted | the top level | what deleting a folder does to the snippets in it; restoring the folder would undo the deletion |
| E-book reader | a book being read that another window took out (and not moved in here) | back to the library, saying so | rather than a reader showing a book the library no longer has |
| Snippets, Reminders, Flashcards, Finance | an item another window deleted while this one's editor was open on it | the editor's Save puts it back, with the edit | an edit here beats a deletion elsewhere, as in the merge; it was dropped without a word |
| Finance | a transaction in an account the other window deleted (either window) | the account comes back, from this window's copy or the file's | a transaction must be in an account, and the reader refuses one that is not; as Notes' notebooks |
| Finance | a budget set for one category in both windows | the later save's -- budgets merge by category, not by id | there is one budget per category |
| Flashcards | a deck another window deleted while this one changed it | this window's next save writes its file again | each deck is a file of its own; an edit here beats a deletion elsewhere |

**Closing while a save is failing.** A window whose last save failed holds
a change no file has, and closing it then loses that change. Notes,
Contacts, the kanban board, Reminders, Snippets, Flashcards and Finance ask
first (`apps/unsaved`: Save tries again and goes only if that works, Don't
save goes, Cancel stays); the e-book reader, which keeps only places and the
shelf, says so once and goes at the second close. Finance and Flashcards
went at once until 2026-10-10 -- a review or a transaction lost without a
word. A window that could not read its file when it opened is the
exception: it keeps nothing from then on and has said so from the start,
so it goes without asking (its every save would be refused anyway, since a
save reads the file first).

| Alternative | Why not |
|---|---|
| One window per program, a second launch raising the first | §1239 chose two windows where they are useful; the single-window programs are the other half of it |
| Lock the file while a window has it open | the second window's every save fails, for a reason the user cannot see |
| Keep the whole file from whichever window saved last, and ask "reload?" | loses whichever side the user does not pick; right for one document, wrong for a list of records each window changed a different one of |
| Bring back whatever the other window deleted, everywhere | undoes deliberate deletions; done only where something kept needs it (a notebook a note is in) |

**Reversal.** The merge is `recordfile::merge_by`; each program's repairs are
one function: `restore_needed_notebooks` (notes), `ContactStore::merged`
(contacts), `merge::mend` (kanban), `mend_library` (snippets), `EbookApp::adopt`
(the e-book reader), `merge_ledgers` (finance), and the editors' Save
(`App::save_edit` in snippets, `RemindersApp::save_form`, the card and
transaction forms in flashcards and finance). The close question is each
program's `request_close`.
