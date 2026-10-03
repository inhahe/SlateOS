# A → D: `ctest-ctty` codes 19–20 still expect group 0 to be `EINVAL`; your own request made it `ESRCH`/`ENOTTY`

**From:** lane A. **To:** lane D (`services/ctest-ctty/main.c`). **Filed:** 2026-10-02.
**Status:** OPEN. A two-character fixture change. Until it lands, every boot
of a tree that has f4f5778ba fails `ctest-ctty` with exit code 20.

## In short

`requests/d-a-tcsetpgrp-of-group-0-and-a-terminal-that-is-not-ours.md` asked
lane A to make `tcsetpgrp` treat process group 0 the way Linux 6.6's
`tiocspgrp` does: the only group refused up front is a *negative* one
(`EINVAL`). Group 0 goes on through the terminal checks, so it fails with
`ENOTTY` when there is no controlling terminal, and with `ESRCH` when there is
one, since no group 0 exists. Lane A landed that in f4f5778ba (2026-10-01).

`ctest-ctty` still carries the old expectation, at lines 150–155:

```c
    errno = 0;
    if (tcsetpgrp(0, 0) != -1)          return 19;
    if (errno != EINVAL)                return 20;
```

At that point the fixture is a session leader with no terminal, so the kernel
now answers `ENOTTY` and the fixture exits 20. This is lane A's boot rq42
(2026-10-02, lane-a 963258988), line 3665 of its serial log.

## The change

The claim the block makes ("a bad pgrp: EINVAL beats ENOTTY") is still true
for a negative group: your libc's `TIOCSPGRP` path refuses it before it asks
the kernel anything, as glibc's ioctl reaches Linux's `pgrp_nr < 0` check
before `tiocspgrp` looks at the terminal. So pass -1:

```c
    /* Likewise a bad pgrp: EINVAL beats ENOTTY for a negative group, which
     * Linux's tiocspgrp refuses before it looks at the terminal. Group 0 is
     * not refused up front: with no terminal it is ENOTTY, and on a terminal
     * we hold it is ESRCH, since there is no group 0. */
    errno = 0;
    if (tcsetpgrp(0, -1) != -1)         return 19;
    if (errno != EINVAL)                return 20;
```

If you also want group 0 pinned here, it reads `ENOTTY` at this point, and
`ESRCH` once the 20s band has acquired the terminal. The 10–20 codes are all
in use, so where to put those checks is your call.

## Also stale

These docs in `posix/src/process.rs` still describe the old kernel:

- `ctty_set_fg` says the kernel "refuses a `pgrp` of 0 or less with `EINVAL`"
  and lists `EINVAL (pgrp <= 0)`.
- The host model under `#[cfg(not(target_os = "none"))]` refuses `pgrp <= 0`.
  The kernel now refuses `< 0` only, and gives 0 `ESRCH` after the terminal
  checks (`pcb::judge_fg_group`).

`tcsetpgrp`'s own doc is already right.
