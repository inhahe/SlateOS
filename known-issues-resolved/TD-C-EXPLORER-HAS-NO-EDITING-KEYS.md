## `TD-C-EXPLORER-HAS-NO-EDITING-KEYS` (lane C, 2026-08-26) -- **CLOSED 2026-09-07**

**Closed 2026-09-07 (lane C).** Delete, Shift+Delete, F2, Ctrl+Z and
Ctrl+C/X/V are bound. Delete and Shift+Delete raise a confirmation before
acting; F2 opens a rename box holding the current name; Ctrl+Z restores.

**Two of the three blockers below were already gone, and I did not check
before writing them down.** `gui/toolkit`'s `AlertDialog::destructive` --
affirmative button carrying the verb, drawn in the error colour, focus
starting on Cancel -- is precisely the confirmation the `Delete` row asks
for, and `InputDialog` is a text entry that returns what was typed. Both
predate this entry. The entry asserted they were missing, and the assertion
was never re-tested; the work turned out to be wiring, not building.

The `Ctrl+C`/`Ctrl+V` row was wrong in a different way. It said the file
manager had no clipboard. It has had `copy_selected`, `cut_selected`,
`paste` and a `clipboard` field the whole time -- what it lacks is a
*system* clipboard, so a file copied here cannot be pasted into another
application. That is a real limitation and a narrower one, and it is now
recorded as its own entry rather than as "no clipboard".

What genuinely did not exist was the *inline* rename field, and it still
does not. A rename dialog is the other standard shape for the same
operation, so F2 works today without it.

**A latent bug fell out of wiring Ctrl+Z, and it was the reason to do this
work carefully.** `delete_selected` recorded each recycled file as
`(path, None)`, because the bin owns the data and restore is by entry id.
`execute_undo` only acted `if let Some(d) = dest`, so it skipped every
recycled file and returned `Ok(())` -- an undo that reported success and
restored nothing. It was invisible because nothing called it on a recycle
record. The cause was that `Option<PathBuf>` had to mean two opposite
things: a permanent delete also recorded `None`, for "nothing to reverse",
and skipping *that* is correct. `UndoTarget` (`Path` / `Recycled` /
`Nothing`) names the three cases, and `execute_undo` now takes the bin and
returns how many items it actually put back, so a caller cannot report a
restore that did not happen. See `0ffb599de`.

**One test was passing vacuously and mutation-checking caught it.** The
render test asserted that "notes.txt" appeared among the drawn text -- true
with the dialog entirely absent, because the listing row draws the same
name. It now asserts the whole sentence, joined across the word wrap, which
no listing row can produce.

Original entry follows.

---


**In short:** the file manager is now a real window you can click and type in,
but only the keys that *look* at files work — arrows, Home/End, Enter,
Backspace, Ctrl+A, F5. The keys everyone expects to *change* a file do nothing
at all: Delete does not delete, F2 does not rename, Ctrl+C/Ctrl+V do not copy or
paste. Pressing them is silent — no beep, no message, no greyed-out menu item —
so the window looks broken rather than incomplete.

**Where it lives.** `apps/explorer/src/main.rs` → `handle_key`, the `match` on
`Key`. Everything the file manager needs underneath already exists and is
tested: `fileops.rs` has copy, move, rename and recycle, and `ExplorerState`
already tracks a selection.

**Why it was left out rather than added.** The three editing keys each need a
piece of *window* that does not exist yet, and wiring the key without the piece
is worse than leaving the key dead:

| Key | What is missing |
|---|---|
| `Delete` | A confirmation, and a visible undo. A Delete key that recycled the selection with no prompt and no way back is a key that destroys a user's files on a mis-keypress. |
| `F2` | An in-place rename field. `fileops::rename` is ready; there is no text entry in the listing to drive it. |
| `Ctrl+C` / `Ctrl+V` | A clipboard the file manager can put a *file reference* on, and a paste target. `gui/toolkit` has a clipboard for text. |

**What the proper fix is.** Build the three missing widgets, then wire the keys
to them — not the reverse. A modal confirmation and an inline edit field are
both generally useful in `gui/toolkit`, so neither belongs in this app.
Meanwhile, add a *visible* refusal: a status-line message on a dead editing key
beats silence, because silence is indistinguishable from a bug.

**How you would notice.** Select a file, press Delete. Nothing happens, and
nothing says why.
