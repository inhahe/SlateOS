# C → E — The desktop's Recycle Bin icon has nowhere to open: the file manager has no view of the bin

**From:** Lane C (`gui/desktop`). **To:** Lane E (`apps/explorer`).
**Filed:** 2026-09-25.
**Status:** lane E's half DONE (2026-09-27, b75f7e48a) -- `explorer --recycle-bin`
opens the file manager on the bin: one row per item under its original name
and folder, when it was deleted and its size, and Restore, Delete permanently
and Empty the recycle bin (`apps/explorer/src/binview.rs`). The sidebar has a
Recycle Bin entry too. Lane C's half -- the icon launching
`launcher::FILE_MANAGER` with `--recycle-bin` -- is ready to be done. (The
status line was left at OPEN until 2026-09-28.)

**In short:** Double-clicking the Recycle Bin icon on the desktop should show
what is in the bin, so a file can be found and put back. Nothing can show
that today. The file manager moves files into the bin (`RecycleBin`, under
`$HOME/.recycle`) and undoes a move with Undo, but it has no view that lists
the bin's contents — and pointing it at the bin's storage would list internal
entry folders named by ids. So since 2026-09-25 the icon explains itself in a
notification ("Nothing can show the recycle bin's contents yet …") instead of
doing nothing, which is what it did before.

## What is asked

A way to start the file manager showing the recycle bin: a flag such as
`explorer --recycle-bin`, or any argument lane E prefers. What the view needs
to be useful:

- one row per entry, under its **original** name and folder — the names are
  recorded losslessly already (`RecycleBin::recycle`, `encode_path`);
- when it was deleted, so the newest is findable;
- **Restore** (to where it came from) and **Delete permanently**;
- **Empty the recycle bin**.

"This PC" had the same problem and did not need lane E: it now opens the file
manager at `/`, which is the nearest thing to a drive list this system has.
If lane E ever grows a real "computer" view (drives and mounts), say so and
the desktop will open that instead.

## Lane C's half, for when it lands

`DesktopShell::open_icon` (`gui/desktop/src/lib.rs`) answers the Recycle Bin
icon with a notice today. It becomes one line: a `Launch` of
`launcher::FILE_MANAGER` with the agreed argument. A test drives the icon and
asserts that launch.

## If the answer is no

Say so here and the icon will keep explaining itself; if the bin is never to
be browsable, lane C would rather remove the icon than keep a door that does
not open.
