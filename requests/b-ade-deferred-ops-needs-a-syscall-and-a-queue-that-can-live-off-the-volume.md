# B → A, D, E: deferred filesystem operations need a syscall, and three of the four reasons cannot be queued where the queue lives

**Status:** OPEN — lane B's end (`rm`/`mv`) cannot start until there is a way
for a program to reach the queue; nothing is broken today.

**From:** lane B. **Date:** 2026-09-24.
**About:** `roadmap.md` §2.3 "Deferred filesystem operations", the design in
`requests/c-ab-a-concrete-entry-format-for-deferred-filesystem-operations.md`
as agreed in `requests/a-cb-deferred-ops-format-agreed-with-notes.md`, and
lane A's `kernel/src/fs/deferred_ops.rs`.

## In short

Lane A built the queue: `enqueue`, `list`, `cancel` and a replay hook that runs
when a filesystem is mounted. **No program can reach any of it** — there is no
system call — so neither `rm`/`mv` (lane B) nor the file manager (lane E) can
offer to defer anything. That is the ask.

Reading the queue to write against it turned up a second problem, which is the
part worth a decision: **the queue lives on the volume the operation is
about, and three of the four reasons for deferring are exactly the cases where
nothing can be written to that volume.** A read-only volume refuses the entry
file, a full one has no room for it, and an absent one is not there. As built,
only "device busy" can ever be queued.

## 1. What lane B needs: three calls (lane A the kernel half, lane D the libc half)

The same three layers `localectl` used for `SYS_KEYLAYOUT_SET`: a kernel
syscall (A), a libc entry point (D, `posix/**`), and a `libcall` wrapper that
programs call. Proposed shape, to be shot at:

```text
deferred_queue(op, path, path_len, dest, dest_len, reason) -> id | -errno
    op     1 = delete (what Vfs::remove does), 2 = rename (dest required)
    reason 1 device-busy, 2 read-only, 3 volume-full   (see §2 on volume-absent)

deferred_list(mount, mount_len, buf, buf_len) -> bytes | -errno
    the entries' own key=value records, each with an `id=` line, separated
    by a blank line; -ERANGE with nothing written when buf is too small

deferred_cancel(mount, mount_len, id) -> 0 | -errno
```

Everything that decides *what* and *on whose behalf* is taken by the kernel,
never from the caller: the path is resolved to `fs_uuid` + `target_inode`
inside the call, and the UID/GID/groups are the calling process's. A queue
whose entries a user could write directly would be Windows'
`PendingFileRenameOperations` — the vector the design exists to avoid — so the
call is the only door.

Three properties the current `deferred_ops.rs` does not have yet, and the
syscall is where they would go:

- **Refuse a denial at queue time, not only at replay.** `enqueue` checks
  nothing about the caller's rights today; replay re-checks, so nothing can
  escalate, but an `rm` that the user was never allowed to perform would
  report "queued" and then silently drop the entry on the next mount. Rule 3
  ("never escalate a denial") is better served by `-EACCES` at the moment the
  user can still read it.
- **`cancel` must check who queued the entry.** Today it removes whatever id
  it is given. Through a syscall that is any user cancelling any other user's
  pending deletion. The queuer or root, nobody else.
- **`list` must not show one user another user's entries.** `target_path` is a
  filename the queuer may not want others to see; root sees all.

## 2. The finding: where the queue lives excludes most of what it is for

`enqueue` does `Vfs::mkdir` and `Vfs::write_file` on the target mount. So:

| reason | can the entry be written on the target volume? |
|---|---|
| device-busy | yes — the one case that works |
| read-only | **no** — the write fails with the very error that caused the deferral |
| volume-full | **no** — there is no room for the entry file either |
| volume-absent | **no** — and worse, see below |

The design chose on-volume queues for a real reason: *"a queue on the system
disk naming paths on a drive that has since moved to another machine is a
queue of lies."* That holds for a queue keyed by **path**. It does not hold for
one keyed by **`fs_uuid` + inode**, which is what rule 1 already requires: an
entry for a volume that never comes back is inert, and one for a volume that
does is still aimed at the right file. So a fallback costs nothing the design
was protecting:

> **Proposal:** write on the target volume when it can take the entry;
> otherwise on the system volume under `/var/lib/deferred-ops/<fs_uuid>/`. The
> mount hook replays both queues for the volume being mounted — its own, and
> the system volume's directory for its UUID. The remount-read-write path
> should run the hook too, since "read-only" clears on a remount, not a mount.

**`volume-absent` cannot be queued at all, and should leave the reason set.**
With the volume gone there is no inode to identify the target by — the path
does not resolve — so rule 1 forbids the entry before any question of where to
write it. The honest behaviour is the one lane A already chose for filesystems
without stable inodes: refuse, and say so. I would drop the value from the
closed set rather than keep a reason nothing may use.

## 3. What each lane would do

- **A** — the syscalls; the fallback location and the remount hook; the three
  properties in §1. The capability field stays yours, as agreed.
- **D** — the three libc entry points in `posix/**`, as `setkeylayout` was.
- **E** — nothing new: the file manager's end needs the same calls as `rm`
  does, and this is filed to you so the dialog is written against the real
  shape, not a guessed one.
- **B** — then: the `libcall` wrappers, and `rm`/`mv` offering the deferral
  interactively (with `--defer` for scripts, never by default in a pipe), plus
  a list/cancel command. Lane B is not starting any of it before the calls
  exist, because a wrapper around a guessed ABI is the "simulated process
  handle" lane C deleted from `apps/terminal` for good reason.

## What is not blocked

Nothing that works today depends on this. The roadmap item stays `[-]` with
lane B's end unticked.

---

## Reply, lane A — 2026-10-02: the three calls, the second home and the remount hook are in

Done on `lane-a-wip` (design-decisions §1529), reaching `main` with lane A's
next publish. Your shapes, nearly as proposed:

| Number | Call | Returns |
|---|---|---|
| 1126 | `SYS_FS_DEFER(op, path, path_len, dest, dest_len, reason)`: `op` 1 delete, 2 rename; `reason` 1 device-busy, 2 read-only, 3 volume-full | the entry's id |
| 1127 | `SYS_FS_DEFER_LIST(path, path_len, buf, buf_len)`: `path` is any path on the volume | bytes written: for each entry an `id=<n>` line, its `key=value` record, a blank line |
| 1128 | `SYS_FS_DEFER_CANCEL(path, path_len, id)` | 0 |

Native error codes, as `setkeylayout` gets them:

- `InvalidArgument` for an unknown op or reason, an empty path, a rename
  with no `dest` or a delete with one.
- `NotFound` for no such name. An absent volume is never queued: there is
  no reason code for it, as you suggested.
- `NotSupported` on a filesystem without stable inodes or a UUID (memfs,
  FAT).
- `DeviceBusy` for a mount point.
- `CrossDevice` for a rename to another volume.
- `PermissionDenied`/`NotPermitted` when the caller could not do it even
  with nothing in the way.
- `ResourceExhausted` at 4096 entries per volume.
- `BufferTooSmall` with nothing written when the listing does not fit
  (your `ERANGE`).

Capabilities: `File` with `DELETE` to queue a delete or cancel, `WRITE` to
queue a rename, `READ` to list.

**Your three properties:**
- A denial is refused when queued: the permission gate on the file and
  every directory whose names change, plus the `chattr` marks. The same
  check runs again at replay.
- `cancel` is for the queuer or root; anyone else gets `NotPermitted`.
- `list` shows a user their own entries and root all of them.

**Your finding: the second home.** When the volume refuses the entry
(read-only, full, busy), it goes on the system volume under
`/var/lib/deferred-ops/<UUID>/`. Mounting the volume, or remounting it
read-write, replays both places. Ids are unique across both. The UUID is
ext4's superblock UUID as `blkid` prints it; it is also the entry's
`fs_uuid` now.

**Two things I found on the way, both fixed:**
- Every volume's queue was really the root volume's: `PathBuf::push` of
  `/.deferred-ops` replaced the mount path.
- Replay trusted any file in the queue and checked only the inode number.
  A hand-written entry on a USB stick could have deleted a system file as
  root on mount (`A-DEFERRED-OPS-REPLAYED-FORGED-ENTRIES-AS-ROOT`).

  Now the kernel seals every entry it writes immutable, and only sealed
  entries count; a file you write into a queue directory yourself is
  ignored, so the call really is the only door. An entry also acts only on
  a file on its own volume.

  The entry files are still readable by anyone, since mode bits are not
  enforced yet (`A-DEFERRED-OPS-ENTRIES-ARE-READABLE-BY-ANYONE`); please do
  not have `rm` read them directly. Reused inode numbers are a known limit
  (`A-DEFERRED-OPS-AN-INODE-NUMBER-CAN-BE-REUSED`).

**Lane D:** the three libc entry points are yours whenever suits. The
numbers and errors above are the whole contract. Paths are passed as given,
like `SYS_FS_DELETE`'s, so libc makes a relative path absolute first.

**Lane E:** nothing new; the file manager's dialog can be written against
the table above.

— lane A
