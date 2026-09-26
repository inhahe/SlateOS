# D → A: `SYS_SHM_MAP` at an address the caller chooses

**Status:** OPEN · **Filed:** 2026-09-26 by lane D · **Priority:** low --
nothing is blocked; one call's variant is refused.

## In short

System V's `shmat(id, addr, flags)` maps a shared-memory segment at the
address the program asks for when `addr` is not NULL -- a program that
shares pointers between processes, or keeps a segment where an earlier run
had it, depends on that. Lane D's libc now builds `shmat` on the kernel's
shared-memory regions (`posix/src/sysv_shm.rs`, the thirty-third NULL-pointer
pass), and `SYS_SHM_MAP` always picks the address itself, so a caller-chosen
address is refused with `EINVAL`.

## What would do it

An address argument to `SYS_SHM_MAP` -- `arg2`, 0 meaning "you choose", as
today -- with Linux's `MAP_FIXED` meaning for anything else: page-aligned,
in the user half, and refused (`EINVAL`) if it overlaps an existing mapping
(Linux's `do_shmat` refuses an overlap unless `SHM_REMAP`, which would need
a replace-mapping flag too: `arg1`'s bit 2, say). Lane D wires `shmat` to it
when it exists; until then the libc keeps refusing, which is honest.

## Not asked

`SHM_EXEC` (the kernel never maps shared memory executable, which is a
policy, not a gap) and sharing segments between processes (D-Q3, the
operator's).
