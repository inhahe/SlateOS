# B → A, D: chattr's ioctls do not reach the kernel from a SlateOS program

**From:** Lane B. **To:** Lane A (the kernel), lane D (`posix/`, the C
library). **Filed:** 2026-10-17. **Status:** OPEN.

## In short

`chattr` and `lsattr` are e2fsprogs 1.47.0's now, ported (lane B,
`userspace/chattr`), and agree with Ubuntu's on every case of
`scripts/chattr-diff.sh`. But they are SlateOS programs, linked against our C
library, and on SlateOS they cannot read or set a single attribute:
`lsattr /etc/passwd` says `Operation not supported While reading flags on
/etc/passwd`, and `chattr +i` refuses every file. The immutable and
append-only marks lane A built (design-decisions §1524) are there, enforced,
and out of reach of the tools that set them.

Found by the Opus review of lane B's batch `31ce92405..d89be2f7a`.

## Why

| Where | What it does with `FS_IOC_GETFLAGS` and the rest |
|---|---|
| `posix/src/ioctl.rs`, `ioctl()` (lane D) | answers the terminal requests (`TCGETS`, `TIOCGWINSZ`, `FIONREAD`, ...) and `ENOTTY` for everything else |
| `kernel/src/syscall/linux.rs`, `file_flags_ioctl` (lane A) | answers `FS_IOC_GETFLAGS`/`SETFLAGS` -- but only in the Linux-ABI dispatch, for a program built for Linux |
| the kernel, any ABI | nothing for `FS_IOC_GETVERSION`/`SETVERSION` (`chattr -v`, `lsattr -v`) or `FS_IOC_FSGETXATTR`/`FSSETXATTR` (`chattr -p`, `lsattr -p`) |

So a native program's `ioctl (fd, FS_IOC_GETFLAGS, &flags)` never leaves the
C library. `chattr-diff.sh` cannot see this: it runs both sides under WSL,
whose glibc reaches Linux.

## What lane B needs

1. **Lane A:** a native way to read and set a file's flags by descriptor --
   the same rules `file_flags_ioctl` applies (the flags kept, `EOPNOTSUPP`
   for the rest, `EPERM` for `i`/`a` without the right) -- as a syscall, or
   by routing a native `ioctl` with these requests to the same code. And a
   word on the generation and the project: answered (ext4 keeps both), or
   refused with `ENOTTY`, as Linux does on a file system that has neither --
   which is what `chattr -v` gets today, and is an honest answer.
2. **Lane D:** `ioctl()` in `posix/src/ioctl.rs` forwarding
   `FS_IOC_GETFLAGS` (`0x80086601`), `FS_IOC_SETFLAGS` (`0x40086602`) -- and,
   if lane A answers them, `FS_IOC_GETVERSION` (`0x80087601`),
   `FS_IOC_SETVERSION` (`0x40087602`), `FS_IOC_FSGETXATTR` (`0x801c581f`),
   `FS_IOC_FSSETXATTR` (`0x401c5820`) -- to lane A's call, with the argument
   as Linux has it: a pointer to an `int`, or to a 28-byte `struct fsxattr`.

Nothing in lane B changes when they land: `chattr` calls `ioctl` through
`libcall::inode`, as e2fsprogs calls it.

## Until then

`chattr` and `lsattr` say `Operation not supported` for every file on
SlateOS, which is true, and is what e2fsprogs says on a Linux file system
without the attributes. Their docs and the roadmap say so.

— lane B
