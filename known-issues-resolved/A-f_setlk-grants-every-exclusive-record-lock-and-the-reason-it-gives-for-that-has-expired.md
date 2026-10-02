### [A] `F_SETLK` grants every exclusive record lock, and the reason it gives for that has expired -- 2026-09-21
**Status:** FIXED 2026-10-01. POSIX locks real and released on exit
(2026-09-21); OFD locks released at a description's final close; `F_SETLKW`
waits, refusing a deadlock; native programs reach the table
(`SYS_FS_RECORD_LOCK`). See the 2026-10-01 addendum at the end of this entry.

**In short:** a program can ask the kernel for exclusive use of part of a file
-- the mechanism databases use to stop two copies of themselves writing the
same page. The kernel says yes to everyone. Two programs both asking for
exclusive use of the same bytes are both told they have it.

**This is not news; the interesting part is the justification.** Lane B filed
it on 2026-09-13 (`requests/b-a-advisory-record-locking-is-a-stub-that-always-
succeeds.md`) about their libc side. The kernel side carries a written reason
for granting unconditionally, at `syscall/linux.rs:895`:

> *In our kernel only one process can hold a Linux fd table at a time (no
> cross-process visibility yet), so no other holder can conflict. `F_SETLK` /
> `F_SETLKW` always grant; `F_GETLK` always reports `F_UNLCK`.*

If that were true the behaviour would be correct -- there would be no second
holder to conflict with. **It is not true.** `pcb::linux_fd_install_stdio` is
documented as *"called exactly once, immediately after `set_abi_mode` flips the
process to Linux ABI in `spawn_process` / `exec_process`"* -- that is once **per
process**, and the assignment at `pcb.rs:6855` is unguarded. Nothing anywhere
limits the number of processes holding one; the only mention of the invariant in
the entire tree is the comment asserting it. So every Linux-ABI process has its
own fd table, and two of them can hold the same exclusive lock.

**What I have and have not shown.** I have shown the *justification* is false,
by reading the assignment site and finding no guard. I have **not** shown a
program is currently corrupted by it -- that needs two Linux processes actually
contending for one range, and I have not demonstrated that happens today. The
distinction matters and I have got it wrong three times in one day (`sealing`,
`secpolicy`, and `acl`, where a genuine call path had no actor who could enter
it). So: the reasoning is void, the mechanism is wrong, the exploitation is
unmeasured. What makes it worth fixing regardless is that the comment names
**sqlite WAL locking and Postgres backend startup** as things that "proceed
without modification" -- those are precisely the callers for which proceeding
is the failure, because they proceed into a second writer.

**The fix is a wiring job, not a design job, and the part that should exist
already does.** `kernel/src/fs/reclock.rs` is a complete POSIX record-lock
table: byte ranges, owners, read/write lock types, conflict detection, `set` /
`unlock` / `query` / `list`, its own self-test, and since today identity keying
so a lock taken under one name is seen under a hard link. It has **zero callers
outside its own module** -- I checked, because a module nothing calls is how
this kind of gap usually looks. `fcntl_flock_apply` at `linux.rs:5391` is the
function that should call it.

Two things to get right when wiring it:

| issue | detail |
|---|---|
| the owner identity | POSIX locks are owned by a *process*, OFD locks by an *open file description*. `fcntl_flock_apply` already knows which it is (`is_ofd`), and `reclock::set` takes an `owner: u64`, so both map cleanly -- but they must not share an owner space, or an OFD lock and a POSIX lock from one process would wrongly conflict |
| `reclock` takes paths as `&str` | `set(path: &str, ...)`. Paths here are bytes and may legally contain any byte except `/` and NUL, so a `&str` API cannot express every lockable file (CLAUDE.md item 7). Unreachable today because nothing calls it; wiring it to a syscall is exactly what makes it reachable, so the signature should change to `impl AsRef<Path>` in the same change rather than after |

**`F_SETLKW` resolved by precedent, not by invention.** POSIX says `F_SETLKW`
*blocks* until the lock is available, and nothing here can block on a lock
table. That looked like a fork needing the operator until I read what `flock(2)`
already does in this tree: `sys_flock` returns `EWOULDBLOCK` for every conflict,
strips `LOCK_NB` without honouring it, and **says so in its own doc** -- *"real
Linux blocks (sleeps) on a contended lock when `LOCK_NB` is absent. Our IPC
layer doesn't yet expose a wait queue hook for the VFS lock table, so we return
EWOULDBLOCK for every conflict."*

So `F_SETLKW` should return `EAGAIN` on conflict, carry the same stated
limitation, and name the same missing wait-queue hook. That is consistent with
the neighbouring syscall rather than a second, differently-wrong answer -- and
when the hook lands, both are fixed in one place. `sched::block_current` exists
(`linux.rs:4581`), so the hook is the missing piece, not the primitive.

**The implementation, in the order it has to happen:**

| step | detail |
|---|---|
| 1. `reclock` path API | `&str` -> `impl AsRef<Path>` **before** it is reachable, not after |
| 2. resolve `l_whence` | `SEEK_SET` is `l_start`; `SEEK_CUR` needs the descriptor's offset; `SEEK_END` needs the file size. The handler currently parses the range into `_l_start` / `_l_len` -- underscore-prefixed, deliberately discarded -- so this is where the stub actually lives |
| 3. `l_len` edge cases | `0` means *to EOF*, and a **negative** length means the range *below* `l_start`. Both are legal POSIX and both are easy to get silently wrong on a data-integrity path |
| 4. owner mapping | POSIX -> pid; OFD -> `entry.raw_handle`. Separate owner spaces, or one process's POSIX and OFD locks would conflict with each other |
| 5. `F_GETLK` | `reclock::query`, writing the holder's `l_type` and `l_pid` back |
| 6. `F_UNLCK` | `reclock::unlock` |
**Addendum, 2026-09-21 -- wired, and the gap I made doing it.** `F_SETLK`,
`F_GETLK` and `F_UNLCK` now go through `reclock`. `F_GETLK` is a lookup rather
than an unconditional `F_UNLCK` claim. POSIX locks are released on process exit
at `pcb.rs:6406`, which a previous session had already wired.

**OFD locks are not released by anything.** POSIX locks are owner-keyed by pid,
so the exit path clears them. OFD locks belong to an *open file description*, so
I key them by handle with bit 63 set -- and nothing clears that key. A holder
that dies wedges the range until reboot, which is precisely the property lane B
called "the part that makes them safe".

Latent, not live: `F_OFD_SETLK` can only reach the kernel through
`posix/src/fcntl_ops.rs`, which is still a stub, so no OFD lock can exist yet.
It goes live the moment lane B wires it, which is why they were told the order
rather than left to find it (`requests/a-b-record-locks-are-real-now-*`).

**The fix, in the right place:** `fs::handle::close()` already releases advisory
locks on the *final* close of an open file description --
`funlock_resolved(p, handle)` at `handle.rs:660`, after the `OPEN_FILES` guard
is dropped. An OFD record lock ends at exactly that moment and for exactly that
reason, so the release belongs on the next line. The owner encoding should move
into `reclock` as a `release_ofd(handle)` function first: the tag bit is
currently a fact `linux.rs` knows and `handle.rs` would have to agree about by
hand, and an owner space belongs to the module that owns it.
**F_GETLK** needs the same treatment: it currently reports `F_UNLCK`
unconditionally, which is a *claim about the world* rather than a lookup, and
`reclock::query` is the lookup it should do.

**Addendum, 2026-10-01 -- every part of this entry is done (design-decisions §1505).**
- **OFD release:** `reclock::release_ofd` on a description's final close, as
  proposed above. A memfd's OFD locks now have their own owner tag and are
  released by `memfd::close`. They had shared a number space with file
  handles, so memfd 5's locks and file handle 5's were one owner.
- **`F_SETLKW` waits** (`reclock::set_wait`), and is woken by whatever frees
  its range. A wait that could never end is `EDEADLK`. A signal ends it with
  a restart, so `SA_RESTART` restarts it, as on Linux.
- **Native programs** reach the table through `SYS_FS_RECORD_LOCK` (1093).
  The work for both ABIs is one module, `syscall::record_lock`
  (`requests/d-a-native-programs-cannot-reach-the-record-lock-table.md`).
- **Found and fixed in the same change:**
  - Locks were keyed by re-resolving the handle's path through the caller's
    namespace, applying a jailed process's jail twice.
  - A lock on a stale handle went to the empty path.
  - A memfd's locks were looked up in the file-handle table under its id.
  - `F_GETLK` zeroed `l_pid` where Linux leaves the fields alone, and kept
    the caller's `l_whence` beside an absolute range.
  - The access mode (`EBADF`) and a full table (`ENOLCK`) went unchecked or
    unmapped.
  - POSIX's close rule (closing any descriptor drops the process's locks on
    the file) was not applied on any close path.
