### A-READDIR-AT-TRAIT-METHOD-HAS-TWO-IMPLEMENTATIONS-AND-NO-CALLERS

**Status:** OPEN. **Lane:** A. **Found:** 2026-09-01, while making
"a listing contains no volume label" a VFS invariant.

`FileSystem::readdir_at` (`kernel/src/fs/vfs.rs:483`) is declared with a
default implementation that calls `readdir` and slices, and the doc comment
tells implementors that "filesystem implementations with native pagination
(e.g., ext4 htree) should override for efficiency". Two of them did:

| Override | What it does |
|---|---|
| `FatFs::readdir_at` (`fs/fat.rs:3134`) | skips `to_vfs_entry`'s name/date formatting for entries outside the window |
| `Ext4Fs::readdir_at` (`fs/ext4/vfs_impl.rs:194`) | native paginated walk |

**Nothing calls it.** `Vfs::readdir_at_resolved` — the only path from a
syscall to a paginated listing — open-codes the default instead: it calls
`fs.lock().readdir(&relative)`, collects the entire directory, and slices.
So `SYS_FS_READDIR_AT` (647) on a 100k-entry directory reads and formats all
100k entries to return 32, and the two overrides written to prevent exactly
that have never executed.

**Why it is open-coded, which is also why this is not a one-line fix.**
`readdir_at_resolved` has to inject *submount* directories — a filesystem
mounted at `/mnt/usb` must appear as an entry in a listing of `/mnt` even
though the underlying filesystem has no such directory — and it must not
inject one whose name a real entry already occupies. That dedup is
`!entries.iter().any(|e| e.name == name)` over the **whole** listing. A
paginated driver call returns one page, so the dedup cannot be evaluated
without the rest, and the submounts' position in the combined ordering
depends on the driver's `total`. Delegating therefore needs a real design:
either (a) submounts are numbered after the driver's `total` and the dedup is
resolved by mount-time rejection of a colliding name rather than at listing
time, or (b) directories with submount children fall back to the unpaginated
path and everything else delegates.

**The proper fix** is (a): reject a mount whose mount-point name collides
with an existing entry in the parent, at mount time where it is one lookup,
and then `readdir_at_resolved` can delegate straight through and pagination
becomes real. (b) is the cheap version and leaves the pathological case —
listing `/` — on the slow path forever.

**Until then the doc comment is a trap**: it invites the next filesystem
author to write a paginated `readdir_at` that will never run. Whichever fix
lands, that sentence must stop promising something untrue.
