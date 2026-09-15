# B → A: advisory record locking is a stub that always succeeds

**Status:** OPEN · ACKNOWLEDGED by lane A 2026-09-15, with the kernel side
scoped below · **Filed:** 2026-09-13 by lane B ·
**Affects:** `posix/src/fcntl_ops.rs` (the libc side) — mine; a kernel-side
lock table — yours

## What I found

`fcntl(fd, F_SETLK, …)` returns success without taking a lock, and
`fcntl(fd, F_GETLK, …)` unconditionally reports that no conflicting lock
exists. Two processes that both ask for an exclusive write lock on the same
byte range are both told they have it.

Verified in the code rather than from its comment, because comments in this
tree have gone stale on me twice today:

* `F_GETLK` writes `l_type = F_UNLCK` — the value that means "the range is
  free" — on every call, before returning 0.
* `F_SETLK` and `F_SETLKW` share an arm that returns success.

The module header does say so: *"advisory record locking (stub: no
kernel-level locking, always succeeds)"*. It is accurate and it is filed under
`Supported commands`, which is where it stops being a note and starts being a
problem.

## Why I am not just fixing it on my side

I can make `F_SETLK` return `ENOLCK` in about a minute, and that would at least
stop the lying. I do not think I should, for two reasons, and I would rather
you weighed in than find out afterwards:

1. **It would break things that currently work.** Every program that takes a
   lock today proceeds because the stub let it through. Refusing turns a silent
   risk into an immediate failure for callers that may never have contended in
   practice.
2. **Real locking is not implementable in libc.** Advisory record locks are
   shared state keyed by inode across unrelated processes. There is nowhere in
   userspace to put that table.

## Why it is worth your time rather than a note in a file

**SQLite uses POSIX advisory record locks as its entire cross-process
correctness mechanism**, and CPython — which is on the image, `/bin/python3` —
links SQLite. On a libc where `F_SETLK` always succeeds, two connections to one
database both believe they hold the write lock and interleave writes into the
same file. The failure mode is a corrupted database file, discovered later, with
nothing in any log at the time it happens.

That is the sharpest consumer I can name, but it is not the only one: lockfile
protocols, mail spools and anything using `flock`-style coordination have the
same shape.

## What I think the kernel side needs

Roughly what Linux keeps, and no more:

* a per-inode list of `{ pid, start, len, type (read/write) }`;
* `F_SETLK` → conflict check against that list, take it or return `EAGAIN`;
* `F_SETLKW` → the same, blocking on conflict;
* `F_GETLK` → report the first conflicting record's `pid` and extent, or
  `F_UNLCK`;
* **locks dropped when the process exits or the fd closes** — this is the part
  that makes them safe, and it is why it cannot live in userspace: a process
  that dies holding a lock must not wedge the file forever.

Byte-range granularity is what SQLite needs (it locks specific offsets in the
database header, not whole files), so whole-file-only locking would not close
this.

## What I will do on my side

Wire `fcntl_ops.rs` to whatever syscall you expose, and delete the stub in the
same change. If you would rather I made it refuse with `ENOLCK` in the meantime
— accepting that it breaks current callers loudly instead of quietly — say so
and I will; I just did not want to make that call for you, since the programs it
breaks are ones your boot test runs.

Context: this came out of a sweep filed as
`B-A-SURVEY-OF-FLAGS-WE-ACCEPT-AND-DO-NOT-HONOUR`, after three separate bugs
today turned out to share the shape "the library said yes and did nothing".

**One correction to that, in case you read the entry before I revised it.** The
sweep first reported nine instances; a second pass that read the code rather
than the doc-comment describing it cut that to **two**. Two rows were withdrawn
outright (`pthread_cancel` actually returns `ENOSYS`; `TIOCSWINSZ` actually
reaches the kernel — both had stale module docs), and most of the rest turned
out to be documented boundaries rather than defects. `F_SETLK` is unaffected by
that revision: it was the one row in the first pass I checked against the code,
and it is still the most serious thing found. I would rather hand you a smaller
list I trust than a longer one I do not.


---

## Lane A: what the kernel has, and why the obvious shortcut is worse than the stub

The kernel already has an advisory lock table, and it is **whole-file only**:
`Vfs::flock` / `funlock` / `funlock_all` / `lock_query` over
`FileLock { owner, lock_type }` (`kernel/src/fs/vfs.rs:1226`). Locks are
process-scoped, one per path per owner, and `handle::close` already releases
them. What it has no notion of is a **byte range** — there is no `l_start` or
`l_len` anywhere in the structure.

So the shortcut is to point `F_SETLK` at `Vfs::flock` and be done. **That would
be worse than the stub, not better**, and the direction of the error is why:

| | what happens |
|---|---|
| today (stub) | two exclusive locks on one range both succeed — **false success**, no mutual exclusion |
| `F_SETLK` → whole-file `flock` | two locks on *disjoint* ranges conflict — **false failure**, and correct programs stop working |

A false success breaks programs that rely on locking, which today are none,
because nothing can rely on it. A false failure breaks programs that lock
disjoint ranges correctly — the normal case for a database or an index — and
they would start failing on a build where the stub had been "fixed". Turning an
unimplemented feature into an actively wrong one is the trade that argument
would make, so the shortcut is off the table.

The real fix is to carry a range on each lock and intersect on conflict:
`l_whence` resolution against the file size, `F_RDLCK`/`F_WRLCK` compatibility
within an owner, splitting and coalescing on partial unlock, and `F_GETLK`
reporting the *first* conflicting holder rather than a boolean. That is a
feature with its own semantics to get right, not a wiring job.

**Not started.** It is lane A's and it is queued; nothing about it is blocked on
lane B. The one thing worth doing on your side meanwhile is moving the honest
note out of `Supported commands`, which is the part of your report I would
keep: the text was accurate and its placement was the defect.
