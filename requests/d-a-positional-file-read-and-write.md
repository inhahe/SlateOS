# D → A: positional read and write for native processes, so `pread` stops moving the shared file position

**Status:** DONE on `lane-a` 2026-09-27 (`f388798a1`): `SYS_FS_PREAD` 1080, `SYS_FS_PWRITE` 1081; reaches `main` with lane A's next publish. Reply at the end.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-26

## In short

The native ABI has no positional read or write, so libc's `pread`, `pwrite`,
`preadv`, `pwritev`, `preadv2` and `pwritev2` — and kernel AIO's file
reads and writes, which run through them — save the descriptor's position,
seek, transfer and seek back. For the length of the call the shared position
is wrong: another thread reading, writing or `lseek`ing the same descriptor
meanwhile uses the moved position, and two `pread`s on one descriptor at once
can each put the position back under the other. Linux's `pread` never touches
the position; programs that share a descriptor between threads and read it
with `pread` (databases, image and archive readers, any thread pool over one
file) rely on exactly that.

`known-issues.md` → `B-D-PREAD-MOVES-THE-SHARED-FILE-POSITION`.

## What exists already

The Linux-ABI table has it: `sys_pread64` / `sys_pwrite64`
(`kernel/src/syscall/linux.rs`) do a positional transfer through
`pread_file_to_user` and its write counterpart, on the file handle, without
touching the position. Native processes cannot reach them — their file
transfers are `SYS_FS_READ`/`SYS_FS_WRITE` at the handle's position, and
`SYS_FS_SEEK`.

## The ask

Two native calls on a file handle, sharing those helpers:

| call | arguments | answer |
|---|---|---|
| `SYS_FS_PREAD` | `handle, buf, len, offset` | bytes read, or the error |
| `SYS_FS_PWRITE` | `handle, buf, len, offset` | bytes written, or the error |

The position is neither read nor moved. Numbers are yours to choose.

One decision is yours, and it is worth making explicitly: Linux's `pwrite` on
a descriptor opened `O_APPEND` **appends**, whatever the offset — pwrite(2)
lists it under BUGS, and glibc programs live with it. The Linux-ABI
`sys_pwrite64` comment says the offset wins, as POSIX has it. Either is
defensible; libc will pass `O_APPEND` descriptors through unchanged, so the
kernel's answer is the one programs see.

## What changes on lane D's side

`pread`/`pwrite` and `file.rs`'s `at_position` (under the `readv` family)
switch to the new calls; the seek-transfer-restore goes away, and with it the
race. Nothing else in libc depends on the emulation.

I have not touched `kernel/**`.

---

## Reply, lane A — 2026-10-01: built on 2026-09-27, stamped late

`f388798a1` adds the two calls, sharing the Linux table's helpers. Neither
reads nor moves the handle's position.

| call | number | arguments | answer |
|---|---|---|---|
| `SYS_FS_PREAD` | 1080 | `handle, buf, len, offset` | bytes read |
| `SYS_FS_PWRITE` | 1081 | `handle, buf, len, offset` | bytes written |

An offset above `i64::MAX` is `InvalidArgument`.

**On an `O_APPEND` handle the offset wins**, as POSIX has it and as this
kernel's Linux `pwrite64` already did, rather than Linux's append-anyway,
which its own man page files under BUGS. It is the one decision you flagged.
It is recorded as design-decisions §969, so it can be revisited. If a ported program turns out to depend on Linux's
behaviour, the switch is one line in the shared helper.

The stamp is late because lane A's publishes stalled for some days. The calls
reach `main` with the next one, and then libc can switch over.

— lane A
