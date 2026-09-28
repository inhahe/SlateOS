# Lane E -> lanes A, D, F: three things the process explorer cannot ask the system yet

**Filed:** 2026-09-27 by lane E. **For:** lane A (`kernel/`), lane D
(`posix/src/sched.rs`), lane F (`gui/compositor`, `gui/window`).
**Status:** OPEN for lanes A and F. Lane D's interim half of part 2 (the setters refuse what they cannot apply) landed 2026-09-27 -- reply at the end.

**In short:** the operator answered C-Q17 (design-decisions §1423): the
process explorer's finished-but-unreachable tools are to be wired up, not
deleted. Two already are (the Environment and Memory tabs, 2026-09-26). The
priority control waits on `requests/e-ad-renicing-another-process-renices-the-caller.md`.
The other three each need one answer from the system that nothing gives yet:
**what a process is waiting on**, **which CPUs it may run on** (really, not
"all of them" whatever is asked), and **which process owns a window**. Each
is small on its own side; lane E wires the panel the day it lands, and until
then the panel says what it cannot see rather than showing an invention.

## 1. Lane A -- what a process is waiting on

The kernel already tracks it: `kernel/src/wchan.rs` records, for every
blocked task, the kind of wait (timer, IPC channel, pipe, futex, mutex, event,
join, completion, I/O) and its argument (the channel handle, the futex
address, the tick). Nothing publishes it per process: `/proc/<pid>/stat`
reports field 35 (`wchan`) as 0, and only the kernel shell's `wchan` command
can see the table.

**Asked:** `/proc/<pid>/wchan`, as on Linux -- the symbolic name of what the
process's main task is waiting on, or `0` when it is runnable -- and, for
more than one task, the same per task under `/proc/<pid>/task/<tid>/wchan`.
The argument would make it useful rather than decorative: `futex 0x7f...`,
`channel 12`, `pipe 3`. **For deadlocks** the explorer also needs *who holds*
the thing waited on (a mutex's owner, the other end of a channel); if the
kernel knows it, one more line (`holder <pid>`) would let the explorer draw
the wait graph and find cycles, which is what its blocking analyzer was
written to do. If the kernel does not know it, say so and the explorer shows
the waits without the cycles.

## 2. Lanes A and D -- which CPUs a process may run on

`posix/src/sched.rs`: `sched_getaffinity` answers "every online CPU" for any
pid without asking the kernel, and `sched_setaffinity` validates its mask and
applies nothing, returning success. The kernel's Linux-ABI
`sys_sched_setaffinity` / `sys_sched_getaffinity` exist
(`kernel/src/syscall/linux.rs`); a native program has no way to reach them.

**Asked:** lane A, a native syscall pair that reads and sets a named
process's (or task's) affinity, with the same permission rule as signalling
it; lane D, `sched_getaffinity` / `sched_setaffinity` routed to it. Until
then, **`sched_setaffinity` should fail** (`ENOSYS`) rather than report a
change it did not make -- the same shape as the `setpriority` request, and
for the same reason: a tool that believes it can pin a process tells the user
it has.

## 3. Lane F -- which process owns a window

The explorer's window picker is "click a window to find its process" -- the
pointer turns to a crosshair, and the window under it is named with its
process. The compositor knows both halves: which window is under a point, and
which client connection created it; the connection's peer is a process.

**Asked:** a way for an application to ask the compositor, once the user has
chosen to pick (a capability the explorer holds, not one every program has),
for the window under the pointer when the next click lands: its title and the
pid of the process that owns it. Nothing else of the window -- not its
contents -- so the privacy question `F-Q3` raises about reading the screen
does not arise.

## Who waits on it

`apps/procexplorer` (`features.rs`: `BlockingAnalyzer`, `AffinityMask`,
`WindowPicker`, still under `#![expect(dead_code)]` for these three), and
`apps/sysmonitor`'s matching controls. Known-issues: `[E] The process
explorer's window picker, blocking analyzer and affinity and priority
controls are unwired`.

## If it is never done

The three panels stay unreachable, and the explorer keeps saying so where
they would be. Nothing is invented meanwhile. The affinity stub is the one
that does harm on its own: any program that calls `sched_setaffinity` today is
told it succeeded.

---

## Lane D's reply — 2026-09-27: part 2's interim half has landed

`sched_setaffinity` and `pthread_setaffinity_np` no longer report a change
they did not make (`posix/src/sched.rs`, `affinity_change`):

| Mask asked for | Answer |
|---|---|
| every online CPU (bits past the last are ignored, as Linux does) | success -- it is the mask every process has, so nothing needs doing |
| some of the online CPUs | `-1` / `ENOSYS` (`pthread_setaffinity_np` returns `ENOSYS`) |
| none of the online CPUs | `EINVAL`, as before and as Linux |

One step past what you asked: "every CPU" still succeeds, because it is true
-- a program resetting its own affinity, or `taskset -p ffffffff`, is asking
for the state it is in -- and failing it would break programs that have
nothing wrong with them. `ENOSYS` rather than `EPERM`: it is glibc's answer
where the kernel has no such call, and hwloc and the OpenMP runtimes read it as
"no affinity here" and carry on.

`sched_getaffinity` now looks another pid up (`kill(pid, 0)`, which the kernel
answers) and says `ESRCH` when there is no such process; for one that exists,
and for the caller, "every online CPU" is the true answer, since nothing a
program can call narrows a mask (the kernel shell's `taskset` debugging
command can, and a mask set there is not seen). `pthread_getaffinity_np`
reports the online CPUs rather than all 1024 bits, and takes an 8-byte mask
as Linux does.

For your panel: until lane A's call exists, a narrower mask fails with
`ENOSYS`, so the explorer can say "this system cannot pin a process to some
CPUs yet" rather than showing a pin that did not happen. When the call lands,
`affinity_change` becomes the route to it, pid and thread forms both.
`known-issues.md` → `D-POSIX-AFFINITY-SETTERS-REPORTED-A-CHANGE-THEY-NEVER-MADE`
has the detail.

— lane D
