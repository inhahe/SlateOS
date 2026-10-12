## 1531. Default ACLs are inherited by new files, and the umask is the kernel's to apply

**Date:** 2026-10-02 · **Decided by:** Claude (operator-approved scope: §978's doors for the per-file tables, of which §1527 built the access ACL's) · **Lane:** A

**In short:** on Linux, `setfacl -d` gives a directory a *default* ACL: every
file made in it starts with an ACL derived from it. Without that, sharing a
directory with another user means re-running `setfacl` on every new file.
Now a directory can have one, and every way of making a file, directory or
socket node applies it, as Linux's `posix_acl_create` does. Doing that
correctly forced a second change. Under a default ACL Linux ignores the
umask, so the umask must not be applied before the kernel sees the mode.
The kernel now applies it where the node is made; the Linux shim stopped
pre-applying it, and lane D's libc is asked to stop.

**What a new node gets** (`fs::acl::inherit`, the VFS's `new_node_mode`):

| Where | Mode | ACLs |
|---|---|---|
| A directory with a default ACL | the mode asked for, narrowed by the default's owner, mask (or owning group) and other entries; **no umask** | an access ACL, unless the result says nothing the mode cannot; a new directory also takes the default as its own |
| Anywhere else | the mode asked for, less the creator's umask | none |
| Made by the kernel itself | the mode asked for (the kernel has no umask) | as above |

The routes are `write_file`, `open(O_CREAT)` (now one hold of the
filesystem's lock: the file never exists, even for a moment, at 0644 before
its mode), `mkdir`, `mkdirat` through a held directory, socket nodes and
unnamed files (`O_TMPFILE`). Symlinks take none, as on Linux.

**The door:** `system.posix_acl_default` on a directory, read, set, listed
and removed by its owner or root, as the access ACL. On anything else:
reading gives `ENODATA`, setting `EACCES`, and removing succeeds, as Linux
answers. A default ACL the mode bits could say is still stored: a default
is not a mode.

| Where the umask is applied | For | Against |
|---|---|---|
| **In the VFS, where the node is made (chosen -- Linux's place, `current_umask()` in `posix_acl_create`)** | a default ACL can replace it, as it must; one place for every ABI | the ABIs that applied it before must stop, or their pre-masking narrows what a default ACL would grant (harmless otherwise: a umask applied twice is applied once) |
| In each ABI, before the call (as before) | no change | a default ACL could never widen a mode the umask had already narrowed: under `setfacl -d -m g::rwx` and umask 022, Linux makes group-writable files, and this would make read-only ones |

**Not done:**
- Lane D's libc still applies the umask for native programs
  (`requests/a-d-the-umask-is-the-kernels-to-apply-now.md`). Until it
  stops, a native program's file under a default ACL loses the bits its
  umask removes. Nothing else changes.
- Like the access ACLs, default ACLs live in memory and are gone at reboot
  (`A-PER-FILE-STATE-OUTLIVED-ITS-FILE` names their home).
