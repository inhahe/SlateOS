## `TD-C-THE-TOOLKIT-CAN-SELECT-A-FOLDER-AND-NO-APPLICATION-ASKS-IT-TO` (lane C, 2026-09-17) -- FIXED 2026-09-30: eight callers now, and a test driving one found a defect

**Status, 2026-09-30.** The title had gone stale before this was closed:
seven of lane E's applications had become callers --
`archivemanager`, `filediff`, `filesearch`, `mediaconvert`, `renamer`,
`settings`, `soundrecorder` -- and the desktop's photo frame makes eight
("Choose folder…" on its menu, design-decisions §1452). The warning below --
budget for discovering, not for plumbing -- was borne out all the same: the
frame's test, driving the picker through the keys a user presses, found
that opening a folder -- a double click, or Enter on it -- *chose* it
(`FileDialog::activate_entry`), so the picker could not be walked down a
folder at a time; anything deeper than the folders listed where it stood
was reachable only by typing its path. Fixed in the toolkit: opening a
folder opens it in every mode, and the Select button -- or Enter with
nothing highlighted -- chooses the folder highlighted or the one shown, an
empty one included. The seven applications share the fix with nothing to
change -- the toolkit's `FilePicker` lists a folder opened as it lists any
other -- and their tests pass with it; lane E was told
(`requests/c-e-the-folder-picker-opens-a-folder-now-rather-than-choosing-it.md`).
`gui/desktop/src/photo_frame_tests.rs` drives the picker end to end;
`test_select_folder_mode` holds the rule. The four applications named
below as wanting a folder -- `photomanager`, `musicplayer`, `podcast`,
`backup` -- still ask for one file at a time; that is lane E's to take up.

**In short:** guitk's file dialog knows how to choose a *folder*. It has a
named constructor for it, `FileDialog::select_folder()`, and the mode is
handled in four places in the dialog's own logic -- the listing, the
confirmation, the button label, the navigation. No application has ever raised
one. Every picker in the tree opens to read or to write a file.

**Found by** asking what it would take to give `apps/photomanager` the
"import from directory" its feature list used to claim. The answer turned out
to be "call a function that already exists", which is the recurring shape in
this file, one layer further down than usual: not a feature that was never
built, but one built and never *asked for*.

**Why it is worth writing down rather than just doing.** Whoever wires it will
be the mode's first caller, so they are not integrating a known-good
component -- they are finding out whether it works end to end. Four handling
sites and a constructor is a lot of implemented behaviour with no user, and
the parts most likely to be wrong are the ones no test exercises: what
`Picked::Chose` carries for a folder, whether the confirm button enables on a
directory rather than a file, what happens when the chosen folder is empty.
Budget for discovering, not for plumbing.

**The applications that would want it,** each of which currently imports or
exports one file at a time: `photomanager` (import a directory of
photographs), `musicplayer` and `podcast` (a library folder), `backup` (a
source directory). None of them is blocked on anything else.
