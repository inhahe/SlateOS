## 1205. The notes library is one tab-separated file in the settings folder, kept on every change and read whole or not at all

**Date:** 2026-09-25
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** The notes app now keeps everything written in it -- every
notebook and note, with its tags, checklist, table and history of earlier
versions -- in one plain-text file in the user's settings folder
(`notes/library.txt`), rewritten after every change; there is no Save. Until
now nothing was kept at all: every note was gone when the window closed. If
the file is there but cannot be read completely, the app reads none of it,
writes nothing over it, and says so for as long as the window is open. When a
note was made or changed now comes from the clock and is shown as a date and
time; it was a counter ("Modified: 1004").

### One file rather than a file per note

| | One library file (chosen) | A folder of Markdown files, one per note |
|---|---|---|
| What another program sees | a record file: `grep` finds words, but a note's line breaks are written `\n` | each note readable as itself in any editor |
| Notebooks, tags, pins, checklists, tables, history | fields and lines of the one file | folders for notebooks, a header in each note for the rest, the history kept beside it |
| A crash in the middle of a save | the old file or the new one, whole (`safeio`) | each note's file is safe alone, but deleting a notebook with its notes, or moving notes, is many files and can stop half-way |
| Refusing what it cannot read | one decision about one file | per file -- and a folder other programs edit is not what this one wrote |

The folder is friendlier to someone who lives in other editors, and is the one
to move to if that is how the notes turn out to be used; Ctrl+E's Markdown
export is the bridge meanwhile. The single file wins on the property the store
exists for: whatever happens, the notes the user had are the notes they get
back.

### The ledger's format and the ledger's rules

Tab-separated lines with the four escapes, now `textfmt::tsv`, which the ledger
(§1202) and the workbook (§1204) share; the first line names the format
(`slateos-notes 1`); and all-or-nothing reading, for the ledger's reason: a
library read in part and then saved would lose, without a word, whatever was
not read. A note in a notebook the file does not have, two things with one
number, or a notebook inside itself is refused the same way, naming the line.
Not YAML: a note keeps up to fifty past versions, so this is the largest record
any of these apps keeps, and `yamldoc` finds each key by scanning (§1204's
table).

### Kept once per event

Every change marks the library, and the event that made it writes it before
the next event is handled -- so one keystroke that makes a notebook and a note
in it writes the file once. A save that fails is shown in red in the status
bar, and the next change -- or Ctrl+S, which otherwise just says where the
notes are kept -- tries again. Closing the window while a save is failing asks
first (`apps/unsaved`); a window whose library was refused does not ask, since
it has said from the start that nothing written in it is kept. What is being
written in a note when the window closes is committed and kept first, where
before it was dropped.

### Stamps from the clock

A stamp is milliseconds since 1970 from the clock, but never earlier than the
last stamp given or read: a later change must sort later, within one
millisecond or after the clock is set back. Shown as `2026-09-25 14:03`, in the
desktop's zone -- UTC until the system has one, as the habit tracker and the
finance app read theirs.

**Where it lives:** `apps/notes/src/main.rs`: `library_path`, `library_text`,
`parse_library`, `when`, `clock_ms`; `NotesApp::{from_settings, load_library,
take_library, after_change, keep, keeping_line, stamp, request_close}`.

**How to reverse:** the format is `library_text`/`parse_library`; a folder of
files replaces those two and `keep`, and keeps `load_library`'s all-or-nothing
rule and `stamp`.
