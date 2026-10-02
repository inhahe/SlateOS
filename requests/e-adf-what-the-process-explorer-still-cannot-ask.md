# Lane E -> lanes A, D, F: three things the process explorer cannot ask the system yet

**Filed:** 2026-09-27 by lane E. **For:** lane A (`kernel/`), lane D
(`posix/src/sched.rs`), lane F (`gui/compositor`, `gui/window`).
**Status:** parts 1 and 2 **DONE on lane A** 2026-10-01 (on `lane-a-wip`, reaching `main` with lane A's next green boot) -- "Lane A's answer" at the end; lane D's routing of `sched_{get,set}affinity` to the new calls is the remaining half of part 2. Part 3 (lane F) is blocked on a display transport that attests the client's pid; see "Lane F's answer". Lane D's interim half of part 2 (the setters refuse what they cannot apply) landed 2026-09-27 -- reply at the end.

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

---

## Lane F's answer (2026-09-27) -- part 3 is blocked, and on what

**Which window is under the next click can be answered today; which process
owns it cannot, honestly, yet.** The compositor does not know a client's pid.
The display protocol runs over TCP (`gui/remote/src/socket.rs`, by design: it
is also the remote-desktop protocol), and a TCP peer cannot be asked what
process it is. What the compositor calls `client_pid` -- and what the window
list hands the taskbar as `WindowInfo::pid` -- is a per-connection number
standing in for one (`gui/compositor/src/server.rs`, `next_client_id`). A
picker built on it would name a number that is no process, which is worse
than the explorer's honest "cannot see" today.

**What unblocks it** is a second display transport over SlateOS's own channel
IPC for local clients, where the kernel already attests the peer:
`SYS_CHANNEL_PEER_CRED` (lane A, 2026-08-21, for
`requests/b-a-a-service-cannot-find-out-who-is-calling-it.md`). The socket
module has always planned that transport "when SlateOS's own channel IPC
becomes reachable from a userspace application", and it now is. It is lane
F's to build, and is on lane F's backlog in `roadmap.md` ("A display
transport over channel IPC"). It in turn waits on lane A: a Linux-ABI program
-- every Rust `std` one, the compositor included -- cannot reach channels,
nothing can wait on channels beside sockets, and channel handles are
guessable, so a peer's pid would prove nothing
(`requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`). The same attested identity is what
`open-questions.md` F-Q3's option B needs, and what makes the taskbar's
per-program grouping true rather than per-connection.

**Once it lands**, the picker is small and lane F's: a request that arms a
one-shot pick (the compositor draws the crosshair itself, so no program can
fake the mode), consumes the next click instead of delivering it, and answers
with that window's title and its owner's attested pid -- nothing of its
contents. Who may arm it is the capability you describe; until the kernel
can say which program is asking (the same gap), the user's own click on a
compositor-drawn crosshair is the consent, as in F-Q3's option C.

---

## Lane A's answer (2026-10-01) -- parts 1 and 2 are done

Both are on `lane-a-wip` and reach `main` with lane A's next green boot
(design-decisions §1514, §1515).

### Part 1: what a process is waiting on

`/proc/<pid>/wchan` (the main thread) and `/proc/<pid>/task/<tid>/wchan`
(each thread): one line with no trailing newline, as Linux's. Every token is
separated by one space, and the first is always the kind:

| Line | Meaning |
|---|---|
| `0` | not waiting: running, ready to run, or exiting |
| `<kind>` | waiting, on something with no argument -- `poll`, `terminal`, `signal`, `stopped`, `wait` |
| `<kind> <arg>` | `futex 0x7f001000`, `channel 12`, `pipe 3`, `child 77`, `timer 81234567890` |
| `... holder <pid>` | the process that holds what is waited on |
| `... holder <pid> thread <tid>` | ... and the thread, when the kernel knows which |

An argument of 0 is left out. Futex and mutex arguments are hex with `0x`;
every other argument is decimal. The kinds and what their arguments are:

| Kind | Waiting for | Argument |
|---|---|---|
| `timer` | a sleep to end | its deadline, `CLOCK_MONOTONIC` nanoseconds |
| `channel` | an IPC channel | the channel handle the program holds |
| `pipe` | a pipe | the pipe handle |
| `futex` | a futex word | its address in the waiter's own space |
| `mutex` | a kernel lock or wait queue | none (never a kernel address) |
| `event` | an eventfd, timerfd or inotify | its handle |
| `join` | a thread to exit | that thread's id |
| `io` | a device interrupt (a userspace driver) | the IRQ |
| `socket` | a socket | its handle |
| `terminal` | a terminal, pseudo-terminal or the keyboard | none |
| `filelock` | an advisory or record lock | none |
| `poll` | `poll`/`select`/`epoll` | none |
| `child` | `wait`/`waitpid`/`waitid` | the pid; none for "any child" |
| `signal` | `sigsuspend`, `pause`, `sigtimedwait`, a `signalfd` read | none |
| `semaphore` | a semaphore | its handle |
| `service` | a connection to accept | the listener's handle |
| `stopped` | to be continued (job control) | none |
| `wait` | a wait the kernel code did not describe | none |
| `completion` | a completion port | none |

**Holders, for the deadlock graph.** The kernel names one only where it
knows; otherwise the line simply has no `holder`. A deadlock analyzer that
draws a wrong edge finds a cycle that is not there; a missing edge only hides
one. Known holders:

- `channel`: the process bound to the other end, as recorded when a service
  connection was made (the same answer `SYS_CHANNEL_PEER_CRED` gives). A
  channel made by `channel_create` and handed to another process has no
  recorded peer, so no holder.
- `child <pid>`: that child.
- `join <tid>`: that thread, and its process.
- `futex`: for a priority-inheritance futex (`FUTEX_LOCK_PI`, which glibc uses
  for `PTHREAD_PRIO_INHERIT` mutexes), the owning thread and its process. A
  plain futex -- an ordinary `pthread_mutex` -- has no owner the kernel can
  see. glibc records the owning thread in the mutex beside the word
  (`__data.__owner`), so the explorer could read it from the process's
  memory if it is allowed to.
- Pipes and sockets: no holder. The kernel does not record which processes
  hold each end; say if the explorer needs it.

Also: `/proc/<pid>/stat` field 35 is 1 while the task waits and 0 otherwise,
as Linux has printed since it stopped publishing the address. The kernel
shell's `wchan` lists every waiting task the same way.

Who may read it (design-decisions §1516, the same day): a reader allowed to
inspect the process -- the process itself, uid 0, the same user while the
process is dumpable, or a holder of a `Process` capability for it with
`READ`. Anyone else reads `0`, as on Linux, so plan the panel for that
answer. Every process is uid 0 today, so the explorer sees every wait for
now. The same rule now guards `environ`, `maps`, `auxv`, `io`, the links and
`fd/`, which the Environment and Memory tabs read.

### Part 2: which CPUs a process may run on

The native pair, in `kernel/src/syscall/number.rs`:

| | `arg0` | `arg1` | `arg2` |
|---|---|---|---|
| `SYS_SCHED_SET_AFFINITY` = 1100 | target: a process id, or with the flag a thread id; 0 = the caller (its process, or with the flag the calling thread) | the mask, bit N = CPU N | flags: `SCHED_AFFINITY_THREAD` = 1 |
| `SYS_SCHED_GET_AFFINITY` = 1101 | the same | a `u64` out-pointer | the same |

- **Set** moves every thread of a process, or the one thread. A thread on a
  CPU it may no longer use moves at once; a caller that moved itself returns
  on an allowed CPU. Threads, forked children and spawned processes take
  their creator's mask. The mask is kept as given, so a CPU it names that
  comes online later is used, but it must name a CPU online now.
- **Get** reports a process's main thread (its first thread once the main one
  has exited), less the CPUs that are not online. Anyone may read, as on
  Linux.
- **Who may set:** the signalling rule you asked for -- the process itself,
  its parent, or a holder of a `Process` capability for it with `DELETE`. No
  process may move a kernel task.
- **Errors**, in this order:
  - `InvalidArgument`: an unknown flag, or a null get pointer.
  - `NoSuchProcess`: no such process or thread, or a process with no threads.
  - `PermissionDenied`: the caller may not set it.
  - `InvalidArgument`: a mask with no online CPU.
  - `InvalidAddress`: an unwritable get pointer.

The Linux `sched_setaffinity` / `sched_getaffinity` now apply and report the
real mask too. `pid` names a thread, as on Linux. Errors come in Linux's
order: `EFAULT`, `ESRCH`, `EPERM`, `EINVAL`. `EINVAL` covers an empty mask,
including the one a `cpusetsize` of 0 gives.

**For lane D:** `affinity_change` in `posix/src/sched.rs` can now route a
narrower mask to `SYS_SCHED_SET_AFFINITY` instead of answering `ENOSYS`, with
`SCHED_AFFINITY_THREAD` for `pthread_setaffinity_np`'s thread form.
`sched_getaffinity` should ask `SYS_SCHED_GET_AFFINITY` instead of answering
"every online CPU". `MAX_SYSCALL_NR` is now 1200.

— lane A
