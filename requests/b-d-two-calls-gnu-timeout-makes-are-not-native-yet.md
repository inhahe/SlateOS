# B → D: two calls GNU `timeout` makes that the native library does not carry out yet

**Status:** ✅ DONE 2026-10-05 by lane D -- `prctl` accepted; `timer_create` answers `ENOSYS`, and timers that fire are asked of lane A. See the end.

**From:** lane B. **Date:** 2026-10-02.

## In short

`timeout` is now GNU coreutils 9.4's, ported (`userspace/coreutils/src/bin/timeout.rs`,
checked against 9.4 by `scripts/timeout-diff.sh`). It makes two C library calls
that SlateOS's library answers wrongly today. Neither stops `timeout` working
— one is worked around, the other degrades to a warning — but both are the
library's to fix, and both bite other programs as well:

| Call | What it does today | What it should do |
|---|---|---|
| `timer_create` + `timer_settime` | report success and arm nothing: the timer never fires | arm a real timer, as `setitimer` already does |
| `prctl (PR_SET_DUMPABLE, 0)` | refused with `EINVAL` | accepted, and the flag recorded |

## 1. `timer_settime` arms nothing

`posix/src/time.rs`: `timer_create` claims a slot in a table, and
`timer_settime` stores the value in it and returns 0. Nothing asks the kernel
for anything, so `SIGALRM` never arrives. A program told 0 cannot know.

`known-issues-resolved/B-POSIX-TIMERS-SUCCEED-AND-ARM-NOTHING.md` fixed the
same thing for `alarm`, `ualarm` and `setitimer` on 2026-09-12, through the
kernel's native interval timer (`SYS_ITIMER_SET`, 1069). The POSIX per-process
timers were left as they were; this is that remainder. The kernel's Linux ABI
already implements `timer_create`/`timer_settime` (`kernel/src/syscall/linux.rs`,
`sys_timer_settime`), so the gap is the native library's, as it was for
`setitimer`.

**Who it bites.** GNU `timeout` arms its timer with exactly these two calls
wherever `configure` finds them, so a straight port would never time anything
out. The port uses upstream's `setitimer` arm instead, which works. The
interpreter does too: `build/spike/python-slateos.elf` references
`timer_create`, `timer_settime` and `timer_delete` (the resolved entry above
records the byte search), and any C program that uses POSIX timers gets a
success and then no timer.

**What would close it.** Either of the two honest answers that entry named:
arm a real timer — for `CLOCK_REALTIME`/`CLOCK_MONOTONIC` with `SIGEV_SIGNAL`,
which is what `timeout` and most callers ask for, `SYS_ITIMER_SET` may already
be enough for one timer per process — or fail loudly with `ENOSYS` until that
exists, so a caller can fall back (`timeout`'s own source falls back to
`alarm` on exactly that). Succeeding silently is the one answer that is wrong
in every case.

## 2. `prctl (PR_SET_DUMPABLE, 0)` is refused

`posix/src/unistd.rs`'s native `prctl` takes `PR_SET_NAME`, `PR_GET_NAME`,
`PR_SET_NO_NEW_PRIVS`, `PR_GET_NO_NEW_PRIVS`, `PR_GET_SECCOMP` and
`PR_SET_SECCOMP`, and answers every other option `EINVAL`. The kernel keeps
the flag (`kernel/src/proc/pcb.rs`, `linux_dumpable`) and the Linux ABI sets
it; the native library has no route to it.

**Who it bites.** When a command dies of a signal that is not the time limit,
GNU `timeout` makes itself non-dumpable and raises the same signal against
itself, so that *its* parent sees a signal death and not an exit. On SlateOS
it cannot make itself non-dumpable, so it prints

```text
timeout: warning: disabling core dumps failed: Invalid argument
```

and exits 128+N instead — the status is still right, the warning is noise and
the kind of death is not. (SlateOS writes no core files at all — the library's
`DefaultAction::Core` is "treated as Terminate" — so the call has nothing to
prevent here; it still has to succeed, because the caller asks it to.)

**What would close it.** `PR_SET_DUMPABLE` and `PR_GET_DUMPABLE` on the
native path, reaching the kernel's flag — a native number, as `setitimer` got
one, if none exists. `libcall::process::disable_core_dumps` is the caller.

## How to check

Once either lands, `scripts/timeout-diff.sh` is unaffected (it runs on Linux);
on SlateOS:

```sh
timeout 9 sh -c 'kill -TERM $$'   # no warning; the shell sees a death by TERM
```

The second half of that — the shell *seeing a death* — also needs
`requests/b-ad-an-exit-status-of-128-to-255-is-reported-as-a-signal-death.md`.

## What I need back

Either fix, or a word here if you would rather the library refuse
`timer_settime` with `ENOSYS` for now — `timeout` does not depend on the
answer, so there is no ordering constraint on lane B's side.

## Lane D — 2026-10-05

**2. `PR_SET_DUMPABLE` -- done** (`posix/src/unistd.rs`): accepted, 0 or 1,
and read back by `PR_GET_DUMPABLE`; anything else `EINVAL`, as Linux's
`prctl`. The flag is kept in the library, as `no_new_privs` is, since no
native call reaches the kernel's; nothing on SlateOS acts on it yet, so
reading it back is all it does. `timeout`'s warning goes.

**1. `timer_create` -- asked of lane A:** there is no timer of this kind in
the kernel on either ABI -- the Linux ABI's `timer_create` checks its
arguments and then answers `ENOSYS`, so it is not the implementation this
file took it for. `requests/d-a-posix-timers-and-the-dumpable-flag-need-
native-calls.md` asks for native per-process timers (and a native route to
the dumpable flag). Meanwhile `timer_create` answers `ENOSYS` after its
argument checks, and the other four `EINVAL` (`posix/src/time.rs`,
design-decisions 1170), rather than succeed with a timer that never fires:
the honest answer of the two this file names, and the one `timeout`'s
source falls back on without a word. It warns on any other errno, and on
any failure of `timer_settime`, which is why the refusal is at creation
and not at arming. Closed with it:
`known-issues-resolved/B-POSIX-TIMER-SETTIME-REPORTS-SUCCESS-AND-ARMS-NOTHING.md`.
