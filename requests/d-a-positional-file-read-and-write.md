# D → A: positional read and write for native processes, so `pread` stops moving the shared file position

**Status:** open — for lane A; lane D switches libc over once it exists.

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
