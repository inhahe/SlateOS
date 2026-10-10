## 1529. Deferred deletes and renames get their system calls, a second home on the system volume, and a queue only the kernel can write

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous); lane B proposed the call shapes and the fallback location (`requests/b-ade-deferred-ops-needs-a-syscall-and-a-queue-that-can-live-off-the-volume.md`), lane A chose how the queue is trusted · **Lane:** A

**In short:** "delete this when the USB stick is writable again" had a queue
in the kernel but no way in. Now `rm`, `mv` and the file manager can ask,
through three system calls: queue, list, cancel. Asking is refused at once
if the user could not do the operation anyway. The request is kept on the
volume itself, or on the system disk when the volume is read-only or full.
It runs when the volume is mounted or remounted writable. While building
this, the queue turned out to trust any file in its directory and to run
it as root. A forged entry on a USB stick could have deleted a system file
on mount. That is fixed: the kernel seals each entry it writes, and an
entry can touch only files on its own volume.

**The doors** (`syscall/number.rs` 1126-1128):

| Call | Does | Who |
|---|---|---|
| `SYS_FS_DEFER(op, path, dest, reason)` | queues a delete (1) or rename (2); `reason` 1 busy, 2 read-only, 3 full; returns the id | `File` with `DELETE` (delete) or `WRITE` (rename); refused now if the operation would be |
| `SYS_FS_DEFER_LIST(path, buf)` | the entries of `path`'s volume, each an `id=` line, its record, a blank line | the caller's own; root all |
| `SYS_FS_DEFER_CANCEL(path, id)` | removes one | whoever queued it, or root |

An absent volume has no reason code. With it gone there is no file to name
by inode, so the request fails with `NotFound`.

**Where entries live.** `<mount>/.deferred-ops/` first. On `ReadOnlyFilesystem`,
`DiskFull` or `DeviceBusy`, the system volume's
`/var/lib/deferred-ops/<volume UUID>/` instead. That is lane B's proposal:
filing under the UUID costs none of the protection an on-volume queue was
chosen for, because an entry already names its file by inode. Ids are
allocated over both places. A mount, and a remount from read-only to
read-write, replays both. Volumes need a UUID for this, so the filesystem
trait gained `volume_uuid` (ext4's `s_uuid`).

**How an entry is trusted -- the part with real alternatives.** The kernel
enforces no mode bits, and anyone may `chown`. So a queue file's
permissions and owner say nothing about who wrote it.

| Option | For | Against |
|---|---|---|
| **Seal each entry immutable; honour sealed entries only (chosen)** | only root can set the mark (`attr_policy`), and it persists in the inode on ext4; the kernel re-reads each entry after sealing, so one altered between writing and sealing is discarded | the files stay readable by anyone (`A-DEFERRED-OPS-ENTRIES-ARE-READABLE-BY-ANYONE`) |
| A capability tag on the queue directories | unreadable by others too | tags live in memory and are keyed by path, so every mount would have to re-apply them; a tag missed once leaves the queue open |
| Sign entries with a kernel key | survives any filesystem | the key must persist, so it would sit on a disk any process can read |

On removable media a forger with the disk in another machine can seal
anything. Rule 1 therefore went from "the inode number matches" to "the
name, not followed, leads to that inode on the volume being replayed". The
worst a forged entry can do is what its forger could already do to their
own disk.

**Acting for the kernel inside a system call.** The queue's files are the
kernel's, but the call runs in the caller's context. There, the VFS would
judge each write by the caller's ACLs and tags, and would refuse a
non-root caller the immutable mark the kernel needs. So the thread module
gained `as_kernel`, Linux's `override_creds(prepare_kernel_cred())`: for its
length, the VFS's seven caller lookups (`acting_process`) see a kernel
task. The other way, unsetting the thread's owner for the duration, would
make a concurrent `pthread_join` see the thread as gone.

**Not done:** the libc entry points are lane D's, then `rm`/`mv` lane B's
and the file manager lane E's. Entries are not protected from reading,
and an inode number reused before replay is not detected
(`A-DEFERRED-OPS-AN-INODE-NUMBER-CAN-BE-REUSED`).
