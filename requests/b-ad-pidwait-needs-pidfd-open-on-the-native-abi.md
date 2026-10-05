# B → A, D: `pidwait` needs `pidfd_open` on the native ABI

**Status:** OPEN — for lane A (the native syscall table) and lane D (`posix/`).

**From:** lane B. **Date:** 2026-10-02.

## In short

procps-ng's `pidwait` -- "wait until the processes matching this pattern have
exited" -- is ported (`userspace/coreutils/src/pgrep.rs`; `pgrep` and `pkill`
are the same program under other names) and measured against procps-ng 4.0.4
by `scripts/pgrep-diff.sh`. It is **not built** for SlateOS, because the one
call it waits with, `pidfd_open`, answers `ENOSYS` to every native program
(design-decisions §1006: a command that cannot do its job is not shipped). The
kernel already implements the call -- for Linux-ABI programs. This asks for the
native route to it.

`known-issues/B-THE-NATIVE-LIBC-AND-THE-LINUX-ABI-DISAGREE-ABOUT-WHAT-EXISTS.md`
lists `pidfd_open` among the operations the kernel implements and the native
table has no number for, calls it "the easy yes" of its step 2, and says that
step is worth taking "only if a port actually needs one of these". This is
that port.

## What `pidwait` does, exactly

```c
int epollfd = epoll_create(1);
...
int pidfd = pidfd_open(pid, 0);             /* one per matching process */
if (pidfd == -1) {
    if (errno == ENOSYS)
        errx(EXIT_FAILURE, "pidfd_open() not implemented in Linux < 5.3");
    if (errno != ESRCH)                      /* already gone is fine */
        warn("opening pid %ld failed", pid);
    continue;
}
ev.events = EPOLLIN | EPOLLET;
ev.data.fd = pidfd;
epoll_ctl(epollfd, EPOLL_CTL_ADD, pidfd, &ev);
...
while (wait_count < poll_count)               /* until every pidfd has fired */
    wait_count += epoll_wait(epollfd, events, 32, -1);
```

So the native side needs three things to hold together:

| | Today, native | Needed |
|---|---|---|
| `pidfd_open(pid, 0)` | `ENOSYS` after argument checks (`posix/src/process.rs`) | a pidfd, or `ESRCH` for no such process |
| that pidfd in `epoll_ctl(ADD, EPOLLIN \| EPOLLET)` | (never reached) | accepted |
| `epoll_wait` | (never reached) | reports the pidfd readable once its process has exited -- the Linux side's `poll` already does: `kernel/src/syscall/linux.rs`, "poll on a PidFd whose target is gone -> POLLIN" |

Lane A: a native syscall number reaching `sys_pidfd_open`'s machinery (the
target lookup, the `PidFd` handle, its readiness). Lane D: libc's `pidfd_open`
calling it, and the native `epoll` accepting the descriptor and reporting its
`POLLIN`.

## How to know it works

`scripts/pgrep-diff.sh` already runs `pidwait` on Linux against the real one
(its `live` world starts a real process as PID 2 and waits for it). On
SlateOS, with the call in place:

```sh
sleep 3 & pidwait -e sleep; echo $?
# waiting for sleep (pid N)
# 0          -- after about three seconds
```

Lane B then adds `userspace/coreutils/src/bin/pidwait.rs` (three lines; the
module is the whole program) and updates the entry above.

## If never done

Nothing breaks: `pidwait` simply does not exist on SlateOS, which is true and
says so (`command not found`). `pgrep` and `pkill` are unaffected.
