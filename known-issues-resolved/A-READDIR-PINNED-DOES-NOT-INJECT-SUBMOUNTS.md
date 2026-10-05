### A-READDIR-PINNED-DOES-NOT-INJECT-SUBMOUNTS — ✅ FIXED 2026-09-01

**Status:** FIXED in the commit that logged it. **Lane:** A. **Found:**
2026-09-01, alongside the above.

**The fix:** `Vfs::finish_listing(path, entries)` — one helper that drops
volume labels and injects submounts — is now the single route by which a raw
driver listing becomes a VFS listing, and `readdir`, `readdir_at_resolved` and
`readdir_pinned` all go through it. `readdir_pinned` scopes its `fs` guard so
the guard is released before `finish_listing` takes `VFS`, preserving the lock
order the other two establish. `fat::mkfs_self_test` now mounts a memfs at
`/_fmt_selftest/MNT` — a mount point with no physical directory behind it, so
the entry can *only* come from injection — and asserts all three routes list
it and agree on the whole set of names.

The description below is kept because the failure mode is worth having on
record: it is the second instance in two days of a rule written down in two
places and forgotten in a third.

`Vfs::readdir` and `Vfs::readdir_at_resolved` both inject submount
directories into a listing. `Vfs::readdir_pinned` (`fs/vfs.rs`) does not — it
verifies the handle and returns `guard.readdir(&dir_rel)` unchanged.

So `SYS_FS_GETDENTS_PINNED` (664) omits a mount point that
`SYS_FS_READDIR_AT` (647) and `SYS_FS_LIST_DIR` (603) both show. Listing `/`
by path shows `tmp`, `proc`, `sys`, `dev`; listing the same directory through
a pinned handle shows only what the root filesystem physically contains.

**This is the same defect class the volume-label work just closed, in the
same pair of calls, in the opposite direction** — 664 is meant to be the
race-free substitute for a listing taken by path, so a program that swaps
routes to close a race must not thereby lose entries. It has no callers yet
(lane B has not wired 664), which is why nothing has reported it.

**Why the three-route agreement test did not catch it on its own**: the check
first written for the volume-label work compares `readdir` / `readdir_at` /
`readdir_pinned` on the FAT volume's root, and that directory had no
submounts. Three routes that all omit the same thing agree perfectly. The
test only became able to see this defect once it mounted something — which is
the same lesson as §663's "a mask can only be tested by a value outside it",
in a different costume: **an agreement test proves nothing about a case none
of the parties encounters.**
