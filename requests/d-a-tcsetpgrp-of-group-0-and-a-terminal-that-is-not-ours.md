# D → A: `tcsetpgrp(fd, 0)`, a terminal that is not the caller's or has no group, and inotify names past 255 bytes of path

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-26

## In short

Four places where the kernel gives a different answer from Linux for the
terminal and inotify calls libc now passes straight through. None is urgent —
each is an unusual call — but libc cannot fix them, because the information is
the kernel's. Libc's own halves landed with the NULL-pointer audit's twelfth pass
(`posix/src/ioctl.rs`, `posix/src/process.rs`): `tcgetpgrp`/`tcsetpgrp` are now
glibc's — `ioctl(fd, TIOCGPGRP/TIOCSPGRP, …)` — so a descriptor that is not a
terminal is `ENOTTY`, a negative group is `EINVAL`, and everything else is
`SYS_TTY_GET_PGRP`/`SYS_TTY_SET_PGRP`'s answer.

## 1. A process group of 0 is `ESRCH`, after the terminal checks

`sys_tty_set_pgrp` (kernel/src/syscall/handlers.rs) refuses `pgid <= 0` with
`InvalidArgument` before anything else, and the dispatch self-test pins it
("a non-positive pgid should be InvalidArgument"). Linux 6.6's `tiocspgrp`
(drivers/tty/tty_jobctrl.c) refuses only a *negative* group with `EINVAL`; a 0
goes on through the controlling-terminal and session checks (`ENOTTY`) and is
then `ESRCH`, because `find_vpid(0)` finds nothing:

    if (pgrp_nr < 0) return -EINVAL;
    ... not our controlling tty, or not our session -> -ENOTTY
    pgrp = find_vpid(pgrp_nr); retval = -ESRCH; if (!pgrp) goto out_unlock;
    retval = -EPERM; if (session_of_pgrp(pgrp) != task_session(current)) ...

So `tcsetpgrp(0, 0)` is `ESRCH` on Linux with a controlling terminal and
`ENOTTY` without one; here it is `EINVAL` either way. **Ask:** reject only
`< 0` up front, and let 0 fall through to `ESRCH` after the terminal checks.
(The self-test's `tcsetpgrp(0)` case would move to `ESRCH`, or to `ENOTTY` for
its no-process caller.)

## 2. A pty slave that is not the caller's controlling terminal

`SYS_TTY_GET_PGRP` and `SYS_TTY_SET_PGRP` name no terminal: they act on the
caller's session's controlling terminal. So `tcgetpgrp(slave_fd)` on a slave
that is *not* our controlling terminal answers for the one that is, and
`tcsetpgrp` hands *that* one over. Linux refuses both with `ENOTTY`
(`tiocgpgrp`: `if (tty == real_tty && current->signal->tty != real_tty) return
-ENOTTY;`, and the same test in `tiocspgrp`). **Ask:** a form of the two calls
that takes the terminal (as `SYS_PTY_GET_PGRP`/`SYS_PTY_SET_PGRP` already do
for a master), refusing one that is not the caller's controlling terminal with
`ENOTTY`; libc would pass the slave's handle, as it already does for a master.

## 3. A master whose slave no session has claimed

`SYS_PTY_GET_PGRP` answers `ENOTTY` for it, so `tcgetpgrp(master_fd)` fails.
Linux's `tiocgpgrp` on a master returns 0: it skips the controlling-terminal
check, `tty_get_pgrp` finds no group, and `pid_vnr(NULL)` is 0. **Ask:** 0
for a terminal with no foreground group.

## 4. inotify names past 255 bytes of path

The native watch record (`FS_WATCH_EVENT_SIZE` = 528) carries 256 bytes of
affected path. libc derived the reported name from that path and used to cut
it again at 63 bytes; it now keeps up to `NAME_MAX` (255). A file whose *path*
is longer than 255 bytes still arrives cut, and a cut name is another file's.
**Ask:** room for `PATH_MAX` in the record, or the name itself alongside the
path, so the event carries what Linux's does.

I have not touched `kernel/**`.
