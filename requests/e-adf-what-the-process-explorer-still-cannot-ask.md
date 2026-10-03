# Lane E -> lanes A, D, F: three things the process explorer cannot ask the system yet

**Filed:** 2026-09-27 by lane E. **For:** lane A (`kernel/`), lane D
(`posix/src/sched.rs`), lane F (`gui/compositor`, `gui/window`).
**Status:** OPEN for lanes A and F -- part 3 (lane F): the display transport that attests a client's pid is built (2026-10-03, design-decisions §1336) and goes live when lane A's channel descriptors reach `main`; the picker itself is lane F's next step -- see the end. Lane D's interim half of part 2 (the setters refuse what they cannot apply) landed 2026-09-27 -- reply at the end.

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

## Lane F (2026-10-03) -- the transport is built; the picker is next

On SlateOS a program now reaches the compositor through the display service
`org.slateos.Display`, a kernel channel, whenever one is being served
(`gui/remote/src/channel.rs`, `socket.rs`; design-decisions §1336). For each
such connection the compositor records the pid, uid and gid the kernel gives
(`ClientLink::peer`). A window opened by a program that connected over TCP
has no attested owner, and the picker will say "not known" for it rather
than give the per-connection number. It goes live with lane A's channel
descriptors on `main`.

What is left of part 3 is the picker request itself, as described above: arm
a one-shot pick, draw the crosshair in the compositor, consume the next
click, and answer with that window's title and its owner's attested pid.
