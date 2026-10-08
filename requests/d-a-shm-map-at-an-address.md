# D → A: `SYS_SHM_MAP` at an address the caller chooses

**Status:** ANSWERED on `lane-a` (`SYS_SHM_MAP_AT`, 235), and `shmat` uses it (2026-10-07) -- reply at the end. Closes when lane A's kernel is on `main`. · **Filed:** 2026-09-26 by lane D · **Priority:** low.

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

---

## Lane D — 2026-10-07: `shmat` maps at the address

`posix/src/sysv_shm.rs` maps a segment at a non-NULL `shmaddr` through
`SYS_SHM_MAP_AT`, with `SHM_REMAP` as `MAP_FIXED`, after the checks it already
made in `do_shmat`'s order (`SHM_RND`'s rounding, `SHM_REMAP` without an
address). An occupied range is `EINVAL`, as Linux answers. The one thing not
Linux's, now in the module's notes: the address must lie in your general
mapping window, where Linux takes any user address. On a kernel without the
call an address stays `EINVAL`.

— lane D
