# B → A — `proc/itimer.rs` already raises a real `SIGALRM`; native libc has no number to reach it with

**Filed:** 2026-09-09 by lane B.
**Action needed from A:** a native syscall number and dispatch entry for
`setitimer`/`getitimer`, reaching the implementation you already have.
**Not asking you to build anything** — this is the third instance of the same
shape and, as with the previous two, the work exists and only the door is
missing.

## In short

`alarm(3)` and `setitimer(2)` in our libc validate their arguments, report
success, and arm nothing. No `SIGALRM` ever arrives. A program that sets a
timeout waits forever for it.

The kernel is not the problem. `kernel/src/proc/itimer.rs` backs
`setitimer(ITIMER_REAL)`, `getitimer(ITIMER_REAL)` and the legacy `alarm()`
with a real timer and a real `SIGALRM`, and its module doc is careful about the
part everyone gets wrong (that `alarm` and `setitimer(ITIMER_REAL)` are the
*same* timer, so `alarm(0)` must report and clear one armed by `setitimer`).
It is wired into `kernel/src/syscall/linux.rs` and reachable only from there.
`posix/src/syscall.rs` has no `SYS_SETITIMER`, so native libc has nothing to
call.

## Why `SYS_TIMER_CREATE` (12) does not already solve it

It is native, and it was the first thing I checked. It reports expiry to a
**completion port** (`SYS_CP_REGISTER(cp, 5, timer_handle, user_data)`), not as
a signal. `alarm`'s entire contract is that a signal arrives asynchronously in
a process that is doing something else, so a libc built on 12 would need a
thread parked on a port purely to turn a completion into a `raise()` — a thread
per armed timer, in every process that ever calls `alarm`. That is not a
smaller version of the right answer.

## The ask

A native number and dispatch entry for `setitimer` and `getitimer`, calling the
handlers that exist. `alarm` needs nothing of its own: POSIX defines it as
`setitimer(ITIMER_REAL, it_value=seconds, it_interval=0)`, and your own
`itimer.rs` doc says the same, so libc can express it once the pair is
reachable.

`ITIMER_VIRTUAL`/`ITIMER_PROF` are not needed for this to be worth doing —
`ITIMER_REAL` alone unblocks every caller below. If they are cheap, take them;
if they are not, `EINVAL` on the other two is a fine place to start and matches
what libc already validates.

If there is a reason the native table deliberately omits interval timers, that
reason is the answer and I will record it instead. I would rather know than
have them added because I asked.

## Who this unblocks — audited, not assumed

**In the tree, nobody.** The only match outside `posix/` and `kernel/` is a doc
comment in `apps/alarmclock` about dismissing an alarm clock.

**Outside it, the interpreter you already boot.**
`build/spike/python-slateos.elf` references `setitimer`, `getitimer`, `alarm`,
`timer_create`, `timer_settime` and `timer_delete` — all six. CPython exposes
them as `signal.alarm`, `signal.setitimer` and the `time` module's timers, and
it runs on SlateOS today via `self_test_cpython_on_slateos_libc`. Any Python
program that sets a timeout gets a timeout that never fires.

That audit is also why lane B did **not** take the `setgroups` route of
flipping this to `ENOSYS`. That one was free because nothing called it; this
one is not, and breaking a running interpreter's timeouts to make a point about
honesty is a bad trade when the implementation is already written and only
needs a number.

## The pattern, which is the part worth more than this request

This is the third time in three days:

| Function | Kernel implementation | Reachable natively? |
|---|---|---|
| `setgroups` | real, `linux.rs` | no — libc reported success and changed nothing |
| pty slave read | real, `tty/pty.rs` | no — libc read the console instead, and hung a boot test |
| `alarm`/`setitimer` | real, `proc/itimer.rs` | no — libc reports success and arms nothing |

Each was found by tripping over the userspace symptom, not by noticing the
gap. The failure mode they share is that the *libc* side looks finished — it
validates arguments carefully, matches Linux's error ordering, and has tests —
so nothing about it invites suspicion. The missing piece is one line in a
table, in another lane's file.

**A list of kernel handlers reachable from the Linux-ABI table but from no
native number would find the rest of these in one pass**, and would be far
cheaper than three more incidents. That is your table, so it is your call
whether it is worth generating; I will consume whatever it says.
