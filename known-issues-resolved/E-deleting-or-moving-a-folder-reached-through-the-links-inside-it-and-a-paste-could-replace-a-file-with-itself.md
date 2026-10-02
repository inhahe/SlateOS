### [E] Deleting or moving a folder reached through the links inside it, and a paste could replace a file with itself -- 2026-09-27

**Status: FIXED 2026-09-27** (lane E, before either reached `main`'s users
in a published build -- the first is old, the second came with the folder
menu's "Replace it" the same day).

**What happened.** `apps/explorer/src/fileops.rs` planned every bulk
operation with `fs::metadata`, which follows links:
- a **permanent delete** of a folder holding a link to another folder deleted
  the other folder's files, through the link;
- a **move** of it copied those files and then deleted them at the source;
- a link to a folder **above** it made the scan recurse until the stack ran
  out;
- the recycle bin's cross-drive fallback (`apps/recyclebin`) copied a linked
  folder's contents into the bin.

Separately, with "Replace it" chosen, a **cut pasted back into its own
folder** copied the file onto itself and then deleted the source -- the only
copy; a link made in its own folder deleted the file to put a link to it in
its place; and "replace" with a link dropped on a folder's name removed the
folder whole.

**The fix.** Links are planned with `symlink_metadata` and carried as links
(`PlannedAction::is_link`, design-decisions §1220); a source whose destination
is itself is duplicated (copy, link) or left alone (move); a folder cannot be
copied or moved inside itself; a file or link never replaces a folder.
**Where:** `plan_transfer`, `scan_source`, `scan_delete`, `copy_link`,
`remove_link`, `remove_link_or_file`, `same_entry`; `recyclebin::move_path`.
**Tests:** junctions on the Windows host (which cannot make symbolic links
without a privilege), symbolic links elsewhere; each test fails against the
old code.

**Still true:** on a Windows host without the symbolic-link privilege a link
cannot be *copied* -- the one action fails and says why, and a move leaves
that link where it was. The target OS makes links like any unix.
