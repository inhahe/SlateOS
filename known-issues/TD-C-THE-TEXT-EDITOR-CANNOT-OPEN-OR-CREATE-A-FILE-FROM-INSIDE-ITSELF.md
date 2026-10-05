## TD-C-THE-TEXT-EDITOR-CANNOT-OPEN-OR-CREATE-A-FILE-FROM-INSIDE-ITSELF -- FIXED 2026-09-14

**Status: FIXED.** Ctrl+N, Ctrl+O and Ctrl+Shift+S, with File gaining New,
Open... and Save As.... Ctrl+S on a document with no path now asks where to put
it instead of refusing. Four tests drive the dialog through `handle_event`, and
the reintroduction proof fails as it should.

**The fix was to call something that already existed**, which is the part worth
keeping: `guitk::dialog::FileDialog`, already driven by `apps/archivemanager`,
`apps/diskimager`, `apps/vpnmanager` and the desktop shell. This entry asserted
the opposite as fact and three programs were parked behind it. See the
correction below.

One of the three faces never needed a picker at all: **New** is an empty tab and
has no name to ask about. "One gap wearing three faces" was two faces and a
misreading.

**Date:** 2026-09-14. **Lane:** C.

**In short:** the text editor can only ever edit the files that were named on
the command line that started it. There is no New, no Open and no Save As --
not on the menu bar added today, not on the keyboard, nowhere. Start it with no
arguments and you get an empty page you can type into and then cannot keep. The
editor already says so itself: pressing Ctrl+S on that page answers *"No file
name -- Save As needs a file dialog"*.

**It is one gap wearing three faces.** New, Open and Save As all need the same
thing: a way to ask the user for a path.

**CORRECTION, 2026-09-14, and it inverts this entry.** The original text here
read "There is no file picker anywhere in `gui/` -- no dialog crate, no
chooser, no prompt". That is false. `guitk::dialog::FileDialog` is 3,436 lines
with open, save and select-folder modes, filters, an initial path, a default
filename, history navigation and a hidden-files toggle -- and it is **not an
island**: `apps/archivemanager`, `apps/diskimager`, `apps/vpnmanager` and the
desktop shell all drive it today.

How the claim was made: by running `ls gui/` and looking for a *crate* called
dialog, picker or chooser. There is no such crate. There is a *module*, inside
the toolkit every one of these programs already depends on. The check was at
the wrong granularity and the conclusion was stated as fact in an entry three
other programs are blocked by.

That is the same error as `-p sysinfo` earlier the same day -- a true answer to
a narrower question than the one being asked -- and it cost more, because it
nearly bought an afternoon building a second file picker beside the working one.

So the work is wiring, not building, and the entry's blocking claim is void.

**Where it lives.** `apps/editor/src/input.rs` -- `save_active` is the function
that prints the message above. `EditorState::open_file` exists,
`Document::save_as` exists, and both are reachable only from tests and from
`open_all` at startup. `apps/editor/src/main.rs` has no `Key::O` or `Key::N`
binding at all.

**The fix is to call what is already there.** `FileDialog::open()`,
`FileDialog::save().with_filename(..)`, and the editor's File menu grows three
rows that are already written. The pieces below are what it is *built* from,
listed here before anyone knew it had been built:

* `guitk::pathbar` -- path editing with completion, wired into the file
  explorer on 2026-09-14, so it is known to work against a real directory.
* `guitk::filetypes` -- categories and icons for the listing.
* `guitk::modal` -- the overlay and the focus trap.
* `apps/explorer`'s own listing and sorting are the model for the file list,
  though the picker must not depend on the explorer binary.

So the shape is a `guitk` module that composes three existing ones, and the
editor's File menu grows three rows that are already written. Until then the
editor is a file *editor* and not a file *creator*, which is a fair description
of what ships but not of what a text editor is.

**A second program still has the gap and is no longer blocked**, found
2026-09-14 by `scripts/check-tested-but-uncalled.py`. `apps/passwordgen`'s
`export_history` renders the generated passwords as text and is called by
nothing but its own test: the program has no clipboard and no way to name a
file, so the string it builds has nowhere to go.

The original text here said wiring it was "guesswork before a picker exists".
There is a picker, so it is not guesswork -- it is a `FileDialog::save()`, a
field on the app, routing in `handle_event`, and somewhere to report the result,
which that program currently has no status line for. Small, and open.

**FIXED 2026-09-14.** Ctrl+E puts up `guitk::dialog::FileDialog::save()`, the
history is written through `safeio::write_str_atomically`, and the status bar
reports the path -- because a generated password is on screen and a file on
disk is not, so the write is the one thing this window cannot otherwise show.
Exporting an empty history is refused with a reason rather than producing an
empty file of passwords. Four tests drive the whole path and read the file back
off the disk; three of them go red if the write is removed.

Note what was *not* done: persistence. The entry below is still right that this
program keeps nothing across runs, and for a password **generator** that is
correct rather than a gap -- passwords you generated and did not use should not
linger on disk. The export is the deliberate escape hatch, and it now exists.

**Sharper, and worse, checked 2026-09-14:** `apps/passwordgen` persists
*nothing*. No `settingsfile`, no load, no save, no config file of any kind --
the history exists for as long as the window is open and is gone when it
closes. So `export_history` is not one of two ways out of that program, it is
the only one, and it leads nowhere.

**A correction about how this was found.** The gate's own line was wrong in its
reasoning while right in its observation. It read "`export_history` is called
1x, all from tests, while its counterpart `load_history` is called in
production" -- and that counterpart is `gui/desktop`'s Run box remembering
typed commands, a different program keeping different data. The gate paired
them on a shared suffix across the whole workspace. It was the only thing it
reported on a clean tree, so its false-positive rate that day was 100%.

The observation survived the correction because it was checked separately, but
it very nearly did not need to be: the obvious fix for the false finding was to
wire passwordgen's export, watch the gate go green, and conclude the instrument
worked. Scoping, comment-stripping and test-module fixes are in c9a190c77, with
a self-test in 5a6249354 that encodes this case as a fixture.

**Not urgent, and worth saying why:** the editor opens files perfectly well
when something else chooses them -- the file explorer's double-click, a command
line, a future "Open with". The missing piece is only the case where the editor
itself has to ask.
