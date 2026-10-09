## 1557. Mounts stack, as a tree of parents over the path table, and pivot_root moves the tree

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** on Linux, mounting something where something is already
mounted puts the new mount on top, and the one below comes back when the top
one goes; a mount on a directory hides whatever was mounted beneath it until
it goes. Here such a mount was refused (`EBUSY`) until now. It is taken now,
and with it `pivot_root(2)` -- the call that swaps a container's own root in
for the real one -- which every container runtime makes: runc, podman,
bubblewrap and LXC. `/proc/<pid>/mountinfo` now says which mount each one
sits on, so `findmnt` draws the real tree.

**What exists now** (`kernel/src/fs/vfs.rs`):

- Each mount-table entry records the mount it is attached to
  (`MountPoint::parent`): the mount a lookup of its mount point reached when
  it was mounted -- the one below it, when it went on top of one.
- `recompute_visibility`, after every change to a table, marks the mounts no
  lookup reaches (`MountPoint::hidden`); `find_mount` and every lookup pass
  them by. Listings show them, as Linux's do.
- `Vfs::mount_on_top` (Linux's `mount(2)`), and binds and moves, go on top.
  An unmount is refused while a mount is attached to it, not while one sits
  anywhere beneath its path; a lazy unmount, a move, a recursive bind and
  `MS_REC` propagation take a mount with the mounts attached under it.
- `Vfs::pivot_root_tree`, `pivot_root(2)`: the mount at `new_root` becomes the
  root, and the old root goes to `put_old` with every mount on it.
- mountinfo prints each mount's own ID and its parent's.

**Decision 1 -- a path table with parent links, not a dentry-anchored
tree.**

| | Parents over the path table (chosen) | Linux's tree (a mount hangs on a parent mount and a directory entry in it) |
|---|---|---|
| What a lookup does | the longest mount path over it among the reached mounts, as before | crosses into the last mount at each directory it steps into |
| What it needs | a parent per entry and a reached flag, recomputed on each change (every pair of mounts: cheap at tens, and a table holds at most `MAX_MOUNTS`, 1024) | a mount point per directory entry, which the filesystems' path-addressed interface (`FileSystem`) does not have |
| What it gets wrong | nothing a program can see in the rules tested (`build/stacktest.c`, `build/bindtest.c`, checked against Linux 6.6) | -- |

The rule: a mount is covered when one mounted later sits on its mount point
or on a directory above it and is not one of its own ancestors. Table order
is attach order; a move or a pivot re-appends what it attaches.

**Decision 2 -- lookups start at the namespace's root and never cross into a
mount stacked on it.** As on Linux, where a process's root is where its
lookups start: `mount -t tmpfs none /` covers nothing, and
`pivot_root(".", ".")` leaves the old root stacked on the new one at `/`,
reached by nothing.

**Decision 3 -- `umount2("/")` takes the mount stacked on the root.** runc,
bubblewrap and LXC follow `pivot_root(".", ".")` with `fchdir` to a
descriptor on the old root and `umount2(".", MNT_DETACH)`. A working
directory here is a path, and the old root's path after the pivot is `/`, so
`/` is what names it: with a mount stacked on the root, `umount2("/")` takes
that one; with none, it is still `EBUSY`.

**Decision 4 -- only Linux's `mount(2)` stacks.** The kernel's own mounts
(`Vfs::mount`, the native `SYS_FS_MOUNT`) keep refusing an occupied mount
point, so a second mount of `/proc` at boot is an error rather than a silent
shadow over the first.

**Decision 5 -- a pivot renames the working directories by path.** Linux keeps
a working directory as a mount and a directory, so it stays on the same
directory wherever that moves. Here it is a path: each process of the
namespace without a root of its own gets its directory renamed to where the
pivot put it (one at the old root goes to the new root, as Linux's
`chroot_fs_refs` moves it), and path-taken advisory locks move with their
files. An open directory or a watch keeps its old path (known-issues
A-LINUX-MOUNT-GAPS).

**Test.** `fs::vfs::self_test_mount_tree` (the rules, on tables made for
it); `spawn::self_test_linux_stacked_mounts` (`build/stacktest.c`, checked
against Linux 6.6 as root): stacking, covering, a move and a bind on top, a
directory bound onto itself, `pivot_root`'s refusals, `pivot_root(new,
new/old)` and `pivot_root(".", ".")` as runc makes it.
