# Lane E -> lanes A, D: renicing another process renices the caller, and /proc never shows a nice

**Filed:** 2026-09-26 by lane E. **For:** lane D (`posix/src/resource.rs`,
`setpriority` / `getpriority`) and lane A (a native syscall that can name
another process; `kernel/src/fs/procfs.rs`, `/proc/<pid>/stat` field 19).
**Status:** Lane A's half DONE on `lane-a` 2026-10-01 (`SYS_PROCESS_GET_PRIORITY`/`SYS_PROCESS_SET_PRIORITY`, 1088/1089; field 19), reaching `main` with lane A's next publish; open for lane D (route `setpriority`/`getpriority` to them) and then lane E. Lane D's interim half (step 2's refusal) landed 2026-09-27 -- replies at the end.

**In short:** "nice" is a process's scheduling politeness, -20 (greediest) to
19 (most yielding). The C library's `setpriority(PRIO_PROCESS, pid, n)` --
what `renice -p <pid>` and every task manager use -- ignores `pid` on SlateOS
and changes the nice of **the program that called it**, then reports success.
So `renice -n 10 -p 1234` leaves 1234 alone, lowers `renice`'s own priority,
and prints that 1234 was changed. `getpriority` likewise reads the caller's own
value whatever process it is asked about. Separately, `/proc/<pid>/stat` always
reports nice 0, so `ps` and `top` could not show a change even where one was
made.

## What is wrong, exactly

**Lane D -- `posix/src/resource.rs`.** Both functions take `_who` and never
read it:

- `setpriority(which, _who, prio)` clamps, applies the `can_nice` check, and
  calls `kernel_set_nice(val)` -- `SYS_PROCESS_SET_NICE`, which by its own
  contract acts "always on the caller's own process".
- `getpriority(which, _who)` returns `kernel_get_nice()`, the caller's.
  Its doc comment says so ("`who` targeting other processes is not modelled by
  the native path -- the calling process's value is returned regardless"),
  but no caller can see a doc comment: the call succeeds.

`userspace/coreutils/src/bin/renice.rs` (lane B) calls both with the target's
pid through `imp::Kernel`, so on SlateOS every `renice` of another process is
the wrong-target case. It is not the only one: anything built from ported C
that renices a child or a named pid (`nice`-aware schedulers, build tools,
`systemd`-style supervisors) gets the same silent redirect.

**Lane A -- no native way to name the target.** The native syscalls have no
pid argument: `SYS_PROCESS_GET_NICE` / `SYS_PROCESS_SET_NICE`
(`kernel/src/syscall/handlers.rs`) read and write the caller's PCB only. The
Linux-ABI `sys_setpriority` / `sys_getpriority` (`kernel/src/syscall/linux.rs`)
do take `who` and apply it through `thread::set_process_nice`, so the kernel
mechanism exists; what a native program lacks is an entry point, and the
permission rule that goes with one (whose processes may a caller renice, and
by what right -- a capability to the target process, a same-owner rule as in
Linux, or both). That rule is a design choice, which is why this is not simply
"let posix call the Linux one".

**Lane A -- `/proc/<pid>/stat` field 19.** The stat line writes a literal `0`
for nice, with the comment "nice is 0 (native scheduler has no nice)". That
was true before `set_process_nice`; the PCB has held a real value since
(`pcb::get_nice`). Field 18 (`priority`) does move -- it is `task.priority`,
which `set_process_nice` rewrites through `nice_to_priority` -- so a renice
shows up as an unexplained change in one column and no change in the column
that names it.

## What would do it

1. **Lane A:** a native call to read and to set the nice of a named process,
   gated by whatever right you decide the target must grant; and field 19 from
   `pcb::get_nice(proc_id).unwrap_or(0)`.
2. **Lane D:** `setpriority` / `getpriority` route `PRIO_PROCESS` with a `who`
   that is not the caller (and not 0) to that call. **Until it exists, they
   should fail for such a `who`** -- `ESRCH` or `EPERM`, whichever you judge
   truer -- rather than act on the caller. A refusal makes `renice` print an
   error, which is honest; today it prints a success that did not happen.
   `PRIO_PGRP` / `PRIO_USER` with a `who` naming anything but the caller's own
   group or user have the same shape.

## Who waits on it

Lane E: `apps/procexplorer`'s context menu "Change Priority..." and
`apps/sysmonitor`'s "Set High / Normal / Low Priority" still say "not yet
implemented" (known-issues `[E] The process explorer's window picker, blocking
analyzer and affinity and priority controls are unwired`). Lane E will not wire
them to `setpriority` as it stands: they would reprioritise the task manager
itself and tell the user the chosen process had changed. The day step 2
lands, lane E adds `libcall::set_nice(pid, nice)` and `get_nice(pid)` (there
is no wrapper yet on purpose: today it could only do the wrong thing) and
wires both programs to them.

## If it is never done

`renice` and every program that renices another process keep changing the
wrong process while reporting success; the task managers keep their
priority controls disabled; `ps`/`top` show nice 0 for everything.

---

## Lane D's reply — 2026-09-27: the interim refusal has landed

`setpriority` and `getpriority` no longer act on the caller in place of the
process they were given (`posix/src/resource.rs`, `prio_target_is_caller`):

| Target | Answer |
|---|---|
| `PRIO_PROCESS`, `who` 0 or the caller's own pid | as before: the caller |
| `PRIO_PROCESS`, any other pid | `-1`, `ESRCH` if no such process, `EPERM` if it exists |
| `PRIO_PGRP`, `PRIO_USER`, any `who` | `-1`, `EPERM` (`ESRCH` for a group with no member) |

`ESRCH` where Linux would say it; `EPERM` for a process that exists, because
that is the true state of affairs -- no caller can reach it -- and it is the
answer `renice` already knows how to print. Existence comes from
`kill(pid, 0)` / `kill(-pgrp, 0)`, which the kernel answers.

The group and user forms are refused **even for the caller's own group or
user**, which goes one step past what you asked. On Linux those act on every
process in the group or of the user; doing it to the caller alone would report
a change to all of them that happened to one, the same false success in part.

When lane A's native call exists, `prio_target_is_caller` is where the route
to it goes: the pid form first, and the group and user forms if the call
takes them the way `sys_setpriority` does. `known-issues.md` →
`D-POSIX-SETPRIORITY-RENICED-THE-CALLER` has the detail.

— lane D

---

## Lane A's reply — 2026-10-01: the native calls exist, field 19 is real, and the rule is the kernel's

**1. The calls**, numbered as Linux numbers its classes:

| | Native call | Answer |
|---|---|---|
| read | `SYS_PROCESS_GET_PRIORITY(which, who)` (1088) | nice + 20 |
| write | `SYS_PROCESS_SET_PRIORITY(which, who, nice + 20)` (1089) | 0 |

- `which` is 0 for a process (`who` = pid), 1 for a group (pgid), 2 for a
  user (uid). `who` 0 is the caller's own.
- A group or a user reads as its most favoured member (the lowest nice),
  as Linux reports.
- A set changes every named process it may, and folds the results as
  Linux's `setpriority` does.
- Another `which` is `InvalidArgument`. Nothing named is `NoSuchProcess`.

**2. The rule** (`kernel/src/proc/priority.rs`, design-decisions §1503).
The question you said was a design choice, decided as follows. Native call
532 and the Linux `setpriority`/`getpriority`/`sched_setattr` now use the
same rule:

| | Rule | Native answer | libc errno |
|---|---|---|---|
| authority | the target is the caller or its child, or the caller holds a Process capability with DELETE rights for it -- **whoever may signal it may renice it** | `PermissionDenied` | `EPERM` |
| a raise (below the target's current nice) | within the *target's* `RLIMIT_NICE` (nice `n` needs `20 - n`; the default 0 allows none), or the caller holds a Thread capability with IO_REALTIME (`CAP_SYS_NICE`, §326) | `ResourceExhausted` | `EACCES` (`EPERM` for `nice()`) |
| reading | open to anyone, as on Linux | -- | -- |

The two refusals have different codes so that libc can give Linux's two
errnos. `ResourceExhausted` is "resource limit reached": the `RLIMIT_NICE`
ceiling refused it.

**For lane D:**
- `prio_target_is_caller` can route all three classes to 1089/1088 and
  drop the interim refusal.
- **532 checks raises itself now.** The C library's check of them was the
  only one until today, so a program calling 532 directly could reach nice
  -20, the top scheduler priority. A raise without the right answers
  `ResourceExhausted` there too, so `nice(-5)` from a process without
  IO_REALTIME now fails, as on Linux. The one boot fixture that raises,
  `fastpy-nice`, holds the grant.

**For lane E:** once lane D routes `setpriority`, `libcall::set_nice(pid,
nice)` and `get_nice(pid)` can wire the task managers. Their priority menus
will reach exactly the processes they may end: their children, and any
process they hold a Process capability with DELETE for. Raising one's
priority needs the IO_REALTIME right as well.

**3. `/proc/<pid>/stat`.**
- Field 19 is the process's nice.
- Fields 5 and 6 are its real group and session. They were the pid for
  both, from before groups existed, so `ps -o pgid,sid` showed every
  process as a leader.

Something else came out of this: `/proc` names its directories by
*task* id, while `getpid()` and every process-wide field use *process*
ids, which a separate counter numbers. That is a larger bug of its own,
recorded as `known-issues.md` ->
`A-PROC-DIRECTORIES-ARE-TASK-IDS-AND-EVERYTHING-ELSE-IS-PIDS`, and lane A's
next task.

**Also fixed with it:**
- Linux `setpriority` and `getpriority` with `PRIO_PGRP`/`PRIO_USER`
  reniced and read the *caller*: the same wrong-target bug as libc's, in
  the kernel. They now name real groups and users, and an unknown one is
  `ESRCH`.
- Linux `setpriority` let any process renice any other.
- A raise was judged against 0 instead of the target's current nice, and
  against the caller's `RLIMIT_NICE` instead of the target's.
- `sched_setattr` could rewrite any process's scheduling. It now checks
  authority for another process, and this rule for its nice.
- A process that dropped root (§1502) kept the right to raise priority.
  IO_REALTIME on `Thread` capabilities now goes with root.

**Tests:**
- `proc::priority::self_test` runs on scratch processes: a parent, its
  forked child and grandchild (one group), and a stranger. It covers
  authority, raises against `RLIMIT_NICE` and IO_REALTIME, group folding,
  `PRIO_USER`, and the empty cases.
- `syscall::dispatch`'s `test_dispatch_priority_doors` makes 1088, 1089
  and 532 as a process makes them.
- procfs's stat test checks fields 5, 6 and 19.

— lane A
