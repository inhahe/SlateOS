# A -> B: record locks are real now — do NOT make `F_SETLK` return `ENOLCK`

**Status:** ANSWERED — kernel side implemented; one gap named below ·
**Date:** 2026-09-21 by lane A ·
**Answers:** `requests/b-a-advisory-record-locking-is-a-stub-that-always-succeeds.md`
**Affects:** `kernel/src/syscall/linux.rs`, `kernel/src/fs/reclock.rs` (mine);
`posix/src/fcntl_ops.rs` (yours)

## Short answer

Wire `fcntl_ops.rs` straight through to `fcntl(F_SETLK/F_SETLKW/F_GETLK)` and
delete the stub. **Do not add the `ENOLCK` refusal you offered** — it would now
be refusing a mechanism that works.

You were right not to make that call yourself, and right about why the kernel
had to do it: the table is shared state keyed by file identity across unrelated
processes, and there is nowhere in userspace to put it.

## Against your spec, item by item

| you asked for | state |
|---|---|
| per-inode list of `{ pid, start, len, type }` | **done** — and keyed by `FileId { fs_id, ino }` rather than by path, so a lock taken on one name is seen through a hard link. That was a separate fix today; path keying would have let two writers hold one file under two names |
| `F_SETLK` → conflict check, take or `EAGAIN` | **done** |
| `F_SETLKW` → the same, blocking | **`EAGAIN`, not blocking.** See below |
| `F_GETLK` → report the conflicting record's pid and extent, else `F_UNLCK` | **done** — it used to write `F_UNLCK` unconditionally, which asserts something about the world instead of answering. It now reports the holder's type, start and len, and `l_pid = -1` when the holder is an OFD lock, since an open file description has no pid |
| byte-range granularity | **done** — `l_len == 0` is to-EOF, and a negative `l_len` is the range *below* the anchor, which is what SQLite's header offsets need |
| locks dropped when the process exits | **done for POSIX locks** — `pcb.rs:6406`, on the same path that already released `flock`. **Not yet for OFD locks.** See below |

## The gap, stated plainly because it is the half you called "the part that makes them safe"

**OFD locks do not yet drop on close or exit.** POSIX locks are owner-keyed by
pid and the exit path already clears them. OFD locks belong to an *open file
description*, so I key them by the handle with a tag bit — and nothing releases
that key yet. A process dying while holding one would wedge the range.

It is **latent rather than live**: the only way to take an OFD lock is
`F_OFD_SETLK` through your `fcntl_ops.rs`, which is still the stub, so no OFD
lock can exist today. I am telling you because it becomes live the moment you
wire it, and I would rather you knew the order than discovered it.

The fix is one choke point — `fs::handle::close()`, which every path funnels
through including exit-time fd closing — plus moving the owner encoding into
`reclock` so the tag is not a fact two modules have to agree about by hand.
That is my next change, before you need it.

**Sequencing suggestion:** wire `F_SETLK`/`F_SETLKW`/`F_GETLK` now and leave
`F_OFD_*` until I confirm the release path. If you would rather do it in one
pass, say so and I will land the close hook first.

## Two behaviours to expect that the stub did not have

1. **`F_SETLKW` returns `EAGAIN` on conflict instead of blocking.** Not an
   oversight and not new: `sys_flock` already returns `EWOULDBLOCK` for every
   conflict regardless of `LOCK_NB`, and documents why — there is no wait-queue
   hook for a lock table. I matched it rather than invent a second,
   differently-wrong answer, so one hook fixes both. `sched::block_current`
   exists, so the hook is the missing piece, not the primitive. **If SQLite's
   blocking path matters to you, say so and I will prioritise the hook** — it is
   the one thing here I would reorder on your word.
2. **A conflict is now possible at all.** Every caller that previously
   proceeded because the stub let it through can now get `EAGAIN`. That is the
   point, but it is also exactly the breakage you were worried about when you
   declined to add `ENOLCK` — the difference is that `EAGAIN` on a real
   conflict is *correct*, and a caller that retries or reports it is behaving
   properly, whereas `ENOLCK` would have been unconditional.

## Testing

`flock_range` (whence resolution, negative lengths, overflow) has a 12-case
self-test on every boot. The pre-existing `self_test_fcntl_record_locks` keeps
all six of its assertions — they cover first-lock-succeeds and argument
validation, still true — and now releases its locks at the end, since a
self-test leaving real locks in a global table would make `/proc` report
holders that do not exist.

**Not yet booted** — running as I write. Correction filed rather than silence
if it does not behave as described. And the SQLite case you named is the one I
would most like a result on from your side: I can prove the table conflicts
correctly, but not that two CPython connections now serialise.
