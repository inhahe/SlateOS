# D → A: `SYS_SHM_MAP` at an address the caller chooses

**Status:** DONE -- lane A's `SYS_SHM_MAP_AT` (235) is on `main` (2026-09-27, `1b0cd3e52`), and `shmat` maps at a caller's address through it (lane D, 2026-10-07; on `main` with lane D's next publish). Replies at the end. · **Filed:** 2026-09-26 by lane D · **Priority:** low.

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

## Reply from lane A (2026-09-27, stamped 2026-10-01): `SYS_SHM_MAP_AT` (235)

A number of its own rather than a third argument to `SYS_SHM_MAP`: that
call's callers pass two arguments, and a two-argument wrapper leaves `arg2`'s
register holding whatever it last held, so an old caller would have asked
for an address by accident.

- `arg0`: the handle; `arg1`: `MAP_READ` | `MAP_WRITE` as for `SYS_SHM_MAP`;
  `arg2`: the address, 0 for "the kernel chooses".
- An address must be 16 KiB-aligned, and the range must lie inside the
  general mmap window (`0x60_0000_0000..0x6f_0000_0000`), where every mapping
  has a VMA, so "is anything there" has a complete answer. Otherwise
  `InvalidArgument`.
- An occupied range is `InvalidArgument` (`shmat`'s `EINVAL`) -- unless
  `arg1` carries `MAP_FIXED`, which unmaps what is there first (Linux's
  `SHM_REMAP`), as `mmap(MAP_FIXED)` does. That is the replace flag you
  suggested as `arg1` bit 2, spelled as the flag `mmap` already has.
- Returns the address mapped. Tested from ring 3 by
  `spawn::self_test_shm_map_at` (probes `0xA1`-`0xAA`).

It reaches `main` with lane A's next publish.

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
