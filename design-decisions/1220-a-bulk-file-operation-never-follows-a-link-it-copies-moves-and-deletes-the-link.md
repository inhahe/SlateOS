## 1220. A bulk file operation never follows a link: it copies, moves and deletes the link itself

**Date:** 2026-09-27
**Lane:** E
**Decided by:** Claude (autonomous)

**In short:** a link (a shortcut to another file or folder -- on Windows a
junction as well) inside something you copy, move or delete is now copied,
moved or deleted *as the link*. Before, the file manager went through it:
deleting a folder that held a link to your Documents deleted the files in
Documents, moving it copied Documents' files and then deleted them from
Documents, and a link to a folder above made it scan for ever. None of those
files had been selected. The price is one case where following would have
been what you wanted: copy a link onto a USB stick and you get a link, which
on another machine points at nothing -- to copy what it names, open it and
copy that.

**The rule.** `apps/explorer/src/fileops.rs` plans with `symlink_metadata`,
never `metadata`: a link is one action (`PlannedAction::is_link`), made again
at the destination with the same target, removed with `remove_link`, which
refuses if the path is no longer a link. The recycle bin's cross-drive
fallback (`apps/recyclebin`) carries links the same way. A file or a link
never replaces a folder: `remove_link_or_file` refuses one, where it used to
remove it whole.

**Alternatives:**
- *Follow links, as it did* -- the three failures above, the first two with
  no warning and on files outside the selection.
- *Follow links for copies, never for deletes and moves* -- a copy that
  follows a link to a folder above itself is still endless, and a move is a
  copy and a delete, so the two cannot differ without a move becoming a copy
  of one thing and a delete of another.
- *Ask when a link is met* -- the question ("this folder holds a link to
  Documents: copy the link or what it names?") is a real one, but it is
  asked in the middle of a copy about something the user did not choose, and
  the safe answer is the one that needs no question. Worth revisiting if
  "copy what it names" turns out to be wanted: it would be a menu choice, as
  "When the name is taken" is.

**Also settled here:** a source whose destination is itself -- a paste back
into the folder it came from -- is never a taken name: a copy of it is a
numbered duplicate, a move of it is nothing to do. "Replace it" had made the
file replace itself and then be deleted as the move's source.
