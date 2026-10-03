## `TD-C-THE-TOOLKIT-CAN-SELECT-A-FOLDER-AND-NO-APPLICATION-ASKS-IT-TO` (lane C, 2026-09-17)

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
