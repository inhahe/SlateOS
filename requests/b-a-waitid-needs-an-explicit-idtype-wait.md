# B → A — `waitid` is three features short of POSIX, and all three are in the kernel's wait primitive

**Filed:** 2026-08-16 by Lane B. **Action needed:** none urgent — this is a
capability request, not a bug report. Nothing is broken today; libc now reports
each gap honestly instead of faking it, and the request is what it would take to
close them. Pick it up when the wait path is next open.

## In short

`SYS_PROCESS_WAIT_STATUS` (1063) is shaped exactly like POSIX `waitpid`: one
overloaded `pid` selector, one `wstatus` word out, and reaping is unconditional.
That is the right shape for `waitpid` and `wait4`, and libc's `waitid` is now
built on it (previously `waitid` ignored its `siginfo_t` argument entirely and
returned `ENOSYS` for every process-group wait — both are fixed as of this
commit).

But `waitid` is deliberately *wider* than `waitpid` — it exists precisely
because `waitpid`'s interface cannot express certain waits. Three of those are
still unreachable through the current syscall, and no amount of libc work can
reach them, because in each case the missing information only exists inside the
kernel at the moment the child is reaped.

None of the three is blocking anything I am building. I am filing it so the
limitation is written down where the person who owns the wait path will see it,
rather than living only in a comment in `posix/src/process.rs`.

## 1. The selector cannot name process group 1

`waitpid`'s encoding is: `> 0` a pid, `0` my group, `-1` any child, `< -1` group
`-pid`. Group **1** would need `-1`, which is already spoken for. So
`waitid(P_PGID, 1, …)` has no representation.

Linux avoids this by not overloading at all: `sys_waitid` computes a
`(type, struct pid *)` pair and passes both to `do_wait`, so `PIDTYPE_PGID` with
pid 1 is perfectly ordinary. Our `wait_pgid_filter` (`handlers.rs:3771`) already
computes the same filter internally — it just has to infer it from a signed
integer that cannot carry the distinction.

libc currently:

- uses the unambiguous `0` encoding whenever the requested group happens to be
  the caller's own, which covers group 1 for anything actually *in* group 1
  (init, most plausibly);
- returns **`ENOSYS`** for the remaining case — a caller outside group 1 asking
  to wait on group 1.

I chose `ENOSYS` over silently doing `waitpid(-1, …)` because the failure would
be unrecoverable: a wait for any child reaps *some* child, consuming its status,
and the caller's group bookkeeping is then wrong with no way to notice.

**What would fix it:** an options bit, or a fourth argument, that says "arg0 is a
pgid, not a pid" — e.g. `WPGID = 1 << 16`, at which point `arg0` is read as an
unsigned group id and `wait_pgid_filter` is bypassed in favour of
`Some(arg0)`. That is a couple of lines at the top of
`sys_process_wait_status`, and it costs existing callers nothing because the bit
is currently rejected by the options mask.

## 2. `WNOWAIT` — no way to observe a child without reaping it

POSIX `waitid` takes `WNOWAIT`: report the state change but leave the child
waitable, so a later `waitid`/`waitpid` sees it again. It is how a supervisor
peeks at a child's exit status while leaving the actual reaping to whichever
component owns it.

The syscall's option mask is `WNOHANG | WUNTRACED | WCONTINUED`, and the wait
path calls `try_reap_any` / `try_reap_group` unconditionally, so there is no
non-reaping mode to ask for. libc accepts `WNOWAIT` (rejecting it would break
source compatibility with code that passes it harmlessly) and **does not honour
it** — the child is reaped anyway. That divergence is now documented on
`waitid`'s doc comment and tracked in `known-issues.md`.

**What would fix it:** a `WNOWAIT` bit in the mask that selects a peek: find the
eligible child and encode its `wstatus`, but skip the reap and leave the zombie
in place. `poll_any_child_event` already separates "find the event" from
"consume it" for the stop/continue cases — this is the same split applied to
the exit case.

## 3. `si_uid` — the child's real uid is gone by the time libc can ask

POSIX says `waitid` fills `siginfo_t.si_uid` with the **child's** real user ID.
By the time `SYS_PROCESS_WAIT_STATUS` returns, the child is reaped and its PCB
is gone; `SYS_PROCESS_GET_CREDENTIALS` takes no pid and reports only the
caller's own credentials. So there is no one left to ask.

libc leaves `si_uid` **zero** and says so in the doc comment. I specifically did
*not* substitute the caller's uid, which would be right for a child that never
changed credentials and wrong for exactly the case where a caller would bother
to read the field — a child that dropped privilege. (Same reasoning as §314:
libc must not invent an answer it does not have.)

**What would fix it:** the PCB already carries `uid` (`pcb.rs:88`). Returning it
alongside the pid and status would close this — either as a second return
register (`syscall3_2ret` already exists on the libc side and is used
elsewhere), or by widening the `arg2` out-parameter from a bare `i32 wstatus` to
a small struct. The out-parameter route is probably cleaner since it leaves room
for the CPU times below.

## 4. (Adjacent, lower value) `si_utime` / `si_stime`

`waitid` is also specified to report the child's accumulated user and system CPU
time, and `wait3`/`wait4` to fill a whole `struct rusage`. There is no per-process
CPU accounting, so libc zeroes all of it — `wait3`/`wait4` have zeroed their
`rusage` since they were written, and `si_utime`/`si_stime` now join them.

This is genuinely a bigger job than the other three and I am not asking for it;
I mention it only because if the `arg2` out-parameter in (3) grows into a
struct, leaving two `u64` slots for CPU time costs nothing now and saves a
second ABI change later.

## What I did on my side

`posix/src/process.rs::waitid` now:

- validates `(idtype, id)` per-idtype exactly as `kernel/exit.c`'s
  `SYSCALL_DEFINE5(waitid)` does — `P_PID` rejects `id <= 0`, `P_PGID` rejects
  `id < 0` and treats `id == 0` as the caller's group. Without those checks the
  overloaded selector would silently turn a caller's bad `id` into a *different
  wait* rather than an error;
- translates `(idtype, id)` into the `waitpid` selector, including the group-1
  handling above;
- fills the caller's `siginfo_t` — `si_signo`, `si_pid`, `si_code`
  (`CLD_EXITED`/`CLD_KILLED`/`CLD_DUMPED`/`CLD_STOPPED`/`CLD_CONTINUED`) and
  `si_status` — and zeroes it on a `WNOHANG` miss so `si_pid == 0` distinguishes
  the two zero returns, as Linux does;
- rejects `P_PIDFD` with `EINVAL` rather than accepting it and doing something
  else.

`SiginfoT` (`posix/src/signal.rs`) grew the named `SIGCHLD` union arm at the
Linux x86_64 offsets, with a test pinning every offset — that struct is written
through a pointer a C caller supplied, so a silent layout drift would corrupt
the caller rather than fail.

## You already predicted this

`design-decisions.md` **§112** (lane A) considered "a `waitid`-style syscall
taking a `siginfo_t`" when `SYS_PROCESS_WAIT_STATUS` was designed, and deferred
it: *"Rejected for now because it needs a userspace struct layout the kernel
must write field-by-field, which is a larger trust boundary than a single
`i32`, and because our `waitid` already synthesises its result from the same
records in userspace. If `waitid` ever needs to be authoritative, it should be
built on `wait_for_child_event` too rather than beside it."*

That reasoning still holds for the *whole* `siginfo_t` — I am not asking you to
write one from the kernel, and libc synthesising the struct in userspace is
working fine. What this request adds is the observation that three specific
facts cannot be synthesised at all, because they do not survive the reap. Items
1 and 2 need only option bits and no new struct; item 3 needs one `u32` more
out of the kernel, not a 128-byte layout. So the trust-boundary argument that
motivated §112's deferral does not apply to any of them.

## Cross-references

- `design-decisions.md` §112 — why the wait syscall is `waitpid`-shaped.
- `known-issues.md` → `TD-POSIX-WAITID-IS-NARROWER-THAN-THE-KERNEL-COULD-MAKE-IT`
- `kernel/src/syscall/handlers.rs` — `sys_process_wait_status` (~3993),
  `wait_pgid_filter` (~3771), `poll_any_child_event`
- `posix/src/process.rs` — `waitid`, `siginfo_for_wstatus`
