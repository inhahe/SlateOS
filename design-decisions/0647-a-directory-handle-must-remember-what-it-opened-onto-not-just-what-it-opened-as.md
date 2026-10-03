## 647. A directory handle must remember what it opened *onto*, not just what it opened *as* — and the check is only worth anything inside the filesystem lock

**Date:** 2026-08-30
**Lane:** A
**Decided by:** Claude (autonomous), lane A

**In short:** when a program opens a folder and then says "delete `notes.txt`
inside it", the kernel was throwing away the folder and keeping only its
*name*, then looking the name up again at delete time. If anything had moved
that folder aside in between — a rename, or a symlink (a signpost file that
points at another location) planted in its place — the delete landed in
whatever folder now answered to that name. Nobody was told. This entry records
teaching directory handles to remember *which folder* they were opened on, so
the operation can be refused when the answer has changed, and records why the
check has to happen while the filesystem is locked rather than just before.

### What was wrong

Lane B reported it in
`requests/b-a-the-at-family-resolves-by-path-so-no-toctou-fix-is-possible.md`.
Every one of `openat`, `fstatat`, `unlinkat`, `renameat`, `faccessat`,
`fchmodat`, `linkat`, `symlinkat`, `mkdirat`, `readlinkat`, `utimensat` and
`getdents64` funnels through `posix/src/file.rs::resolve_dirfd_path`, which
calls `get_fd_path(dirfd)` and concatenates. Lane B could not fix it in their
own tree, because the kernel offered nothing else to build on: `handle_path`
returns a name, and every method on the `FileSystem` trait takes a name.

Two things follow, and only the first is a security issue:

| | What happens | Needs an attacker? |
|---|---|---|
| **Redirected operation** | The folder is swapped for a symlink between open and use; the delete/stat follows it elsewhere. | Yes |
| **Plainly wrong answer** | The folder is *renamed* by anyone; the descriptor keeps working but now refers to a different folder (or nothing). | No |

Lane B expected the second to bite a ported build system before the first ever
mattered, which is worth recording: this is not only a hardening change.

### What was decided

Directory handles now store the directory's **identity** — the pair
(`fs_id`, inode number) — captured at open (`OpenFile::dir_pin`). A new family
of VFS entry points takes that pair alongside the path, as `PinnedDir`:

- `Vfs::unlink_at_pinned(dir, name, remove_dir)`
- `Vfs::metadata_at_pinned(dir, name, no_follow)`
- `Vfs::readdir_pinned(dir)`

Each walks the stored path, then **checks that the walk arrived where the
handle was opened**, and refuses with the new `KernelError::StaleHandle`
(→ `ESTALE`) if it did not. `name` must be exactly one component — no `/`, no
`..`, no `.` — because the containment rests entirely on that: verifying a
directory's identity proves nothing if the name is then allowed to climb out
of it.

### Why the check sits inside the filesystem lock

This is the part that makes it a fix rather than a smaller window.
`resolve_mount` clones the mount's `Arc<Mutex<Box<dyn FileSystem>>>` and drops
the global VFS lock, so **every operation on a given filesystem serialises on
that one mutex**. `verify_pinned` therefore takes a `&mut Box<dyn FileSystem>`
— a guard the caller already holds — rather than a path, so the verify and the
removal happen under a *single* hold and no rename can be interleaved. Written
the obvious way instead, as a call to some `Vfs::check_identity(path)`, the
lock would be taken and released before the removal took it again, leaving
exactly the check-then-use race the design exists to close.

Two supporting details, both load-bearing:

- The verify uses `lmetadata`, not `metadata`. Following a trailing symlink
  would report the *target's* inode, and an attacker who can plant the symlink
  can aim it at a directory whose inode is whatever they like — including the
  pinned one. A symlink's own inode cannot be forged into a match.
- `unlink_at_pinned` verifies **twice**: once before the policy checks and the
  auto-version snapshot (so a handle already stale on entry cannot cause a side
  effect — notably archiving the content of a file the caller never named), and
  once inside the acting guard (which is what makes the removal atomic).

### Where the guarantee is absent, and why that is said out loud

Identity needs stable inode numbers. Six of the twelve `FileSystem`
implementations have them (btrfs, ext4, f2fs, memfs, ntfs, zfs — including
both the root filesystem and the primary disk one). The rest (FAT, ISO9660,
devfs, procfs, sysfs, overlay) report `ino == 0`.

`PinnedDir::id` is `Option<FileId>`, and `None` means **unverifiable**, never
"verified". `Vfs::pinned_dir_is_verifiable` exists so a caller that needs the
guarantee can ask and refuse, while a caller that only wants the
single-component containment can proceed *knowing what it did not get*.
Silently treating `None` as a match would be the invented value that
design-decisions.md §600's whole burn-down is about — the same defect, one
layer down.

Opening a directory on such a filesystem still succeeds. Refusing would break
`ls` on a FAT stick to defend against an attack FAT cannot express, since it
has no symlinks.

### `SYS_FS_GETDENTS_PINNED` returns the size of the whole listing, not the bytes it wrote

Lane B asked for a `(dirfd, buf, len)` shape, which is `getdents64`, which
returns bytes written. It does not here, and the deviation is deliberate.

`getdents64` can return bytes-written safely because it is **paginated**: the
kernel holds the directory offset in the file struct, so a short return means
"call me again" and a zero return means "done". `SYS_FS_GETDENTS_PINNED` has no
offset argument — one call, one listing. Under bytes-written semantics a
directory whose records happen to total exactly the buffer size and one that
overflowed it return the identical value, and there is no follow-up call that
would reveal the difference. A recursive tool would enumerate half a directory,
see a plausible byte count, and report success on a subtree it never visited.
That is precisely the silently-wrong-answer shape this whole entry exists to
remove; shipping it inside the fix would be self-defeating.

Returning the requirement makes truncation self-announcing (`ret > cap`) and
doubles as the size to re-issue with, at the cost of one extra pass over the
entry list — which is already in kernel memory, so it is a fold over a `Vec`,
not I/O. `SYS_FS_READDIR_AT` reaches the same guarantee by a different route,
returning a total entry *count* beside the written count; it is paginated, so a
count is the useful unit there and a byte requirement is the useful one here.
Both refuse to let a caller mistake a partial answer for a complete one, which
is the property that matters, and it is worth more than a matching return
convention between two calls of different shapes.

Truncation is at a record boundary, never mid-record: a decoder that trusts its
own `name_len` field cannot detect a half-written record, so producing one would
hand it garbage that looks structured.

### Alternatives considered

| Option | Why not |
|---|---|
| **Add node-based methods to the `FileSystem` trait** (`lookup(parent_ino, name)`, `unlink_at(parent_ino, name)`, …) — the Linux dentry/inode model, and the genuinely "proper" fix | 17 path-based methods × 12 implementations, and six of those twelve have no inode→node map *at all*. procfs (14,743 lines) and sysfs **are** name trees; giving them one would mean fabricating and maintaining an identity their model does not have — inventing the very thing this entry refuses to invent. Not ruled out forever, but it cannot be the first step. |
| **Pin nothing; just add fd-relative syscalls** that resolve the dirfd's path in the kernel instead of in libc | Moves the concatenation across the syscall boundary and changes nothing. This is precisely what lane B said a glibc-based stopgap would achieve: "would test green… and change nothing whatsoever on SlateOS." |
| **Verify identity, but through an ordinary `Vfs::` call before acting** | Shrinks the race window instead of closing it. A narrower race is harder to hit and just as wrong, and it would read as fixed. |
| **Have `rename` rewrite the stored paths of open handles**, so a descriptor follows its directory | Fixes lane B's second consequence (the no-attacker one) and *nothing* of the first — a symlink swap involves no rename to hook. Worth doing later, on top of this, not instead of it. It also cannot be done correctly until identity exists, since it needs to know which handles refer to the moved object. |
| **Return the old behaviour when the filesystem cannot be verified** rather than reporting it | See above: it is §600's prohibited shape wearing a different hat. |

### Cost

An extra `lmetadata` on every directory open, and a second one per
fd-relative operation (two for `unlink_at_pinned`). Directory opens are rare
next to file opens, and the metadata read is served from the same filesystem
lock the operation was going to take anyway, so this does not add a lock
acquisition to any path. Nothing on the file read/write hot path changes.

`ESTALE` is a new error a caller can now see where it previously saw success.
That is the intended observable change: the calls that now fail are the ones
that were previously doing the wrong thing quietly.

### How to reverse it

Delete the three `*_pinned` entry points, `verify_pinned`, `check_at_name`,
`Vfs::pin_dir`, `handle::pinned_dir`, the `dir_pin` field and
`KernelError::StaleHandle`. Nothing else consumes them — the path-based entry
points are untouched and still behave exactly as before, which is deliberate:
this adds a correct route rather than altering the existing one, so the
migration of each `*at` caller is a separate, individually revertible step.
