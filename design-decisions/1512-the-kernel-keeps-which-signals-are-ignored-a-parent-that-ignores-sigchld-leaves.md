## 1512. The kernel keeps which signals are ignored, a parent that ignores SIGCHLD leaves no zombies, and a spawned child starts in its parent's job

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program can say "ignore this signal" (`SIG_IGN`). Until now
only the C library knew, so the setting was lost the moment the program
started another one: `nohup cmd` ignores the hang-up signal and becomes
`cmd`, and `cmd` died when its terminal closed anyway. A parent that ignores
`SIGCHLD` should also never be left holding dead children ("zombies"), and
only the kernel can arrange that. The kernel now keeps the ignored set for
every process, passes it on across `exec`, `fork` and spawn, drops ignored
signals when they are sent, and reaps a child at exit when its parent will
never wait for it. Separately, a program started with `posix_spawn` now
starts where POSIX puts it -- in its parent's process group and session,
with its parent's blocked signals -- and the four `posix_spawn` attributes
for those (group, session, signal mask, default signals) are honoured
before the child runs. Lane D asked for both
(`requests/d-a-ignored-signals-and-spawn-attributes-need-a-kernel-record.md`).

This revisits §115's Decision 3 ("the kernel deliberately cannot see a
native-ABI `SIG_IGN`"), which was mine: it named this as the proper fix and
gave its trigger -- a native program needing POSIX-exact ignore semantics.
`nohup` is one, and a set the kernel must carry across `exec` is something
no libc table can be.

**What changed:**
- **`proc::signal`** keeps an ignored set and `SIGCHLD`'s `SA_NOCLDWAIT`
  beside the pending set and the blocked mask. It is the one record of
  "ignored" for both ABIs: the native libc reports it
  (`SYS_SIGNAL_SET_IGNORED`, 1098; read back with `SYS_SIGNAL_GET_IGNORED`,
  1099), the Linux shim's `rt_sigaction` records it. `exec` keeps the set and
  clears `SA_NOCLDWAIT`; `fork` copies both; a spawned child gets its parent's
  set less `POSIX_SPAWN_SETSIGDEF`'s signals.
- **At send** (`classify_post_info`) an ignored signal is discarded, before
  any trampoline or default action is considered -- unless it is blocked, when
  it stays pending (Linux's `sig_ignored`) and is discarded at delivery if it
  is still ignored then (`take_deliverable_info`). `SIGCONT` still continues
  a stopped process. A *blocked* fatal signal to a process with no trampoline
  now waits for its unblocking, as a blocked signal must; it used to kill at
  once.
- **SIGCHLD:** when a process's last thread leaves, `pcb::remove_thread`
  decides from the parent's record, under the process table's lock, whether
  the parent will collect it (`pcb::ExitNotice`). If not, every `wait` treats
  the zombie as gone -- the parent's last wait ends in `ECHILD` -- and the
  exiting thread releases it once the exit's other work is done
  (`pcb::release_autoreaped`). `SIG_IGN` sends no `SIGCHLD`; `SA_NOCLDWAIT`
  still does, as on Linux.
- **The Linux sigaction table** reads an action it has no entry for off the
  set (`SIG_IGN` or `SIG_DFL`), and an exec drops its entries rather than
  keeping the `SIG_IGN` ones -- a second copy that went stale when a native
  image in between stopped ignoring a signal.
- **Terminal job control** asks the set, so a native shell that ignores
  `SIGTTOU` (as bash does) is let through, and the limitation
  `TD-KERNEL-NATIVE-ABI-SIG_IGN-IS-INVISIBLE-TO-THE-KERNEL` recorded is gone.
- **`/proc/<pid>/status`** prints Linux's `Sig*` lines and `stat`'s fields
  31-34 are real. "Caught" stays the libc's for a native process, so its
  `SigCgt` is empty -- the one disposition the kernel still cannot see.
- **Spawn:** a child with a parent starts in the parent's process group and
  session (it led its own until now, so it had no controlling terminal and a
  `^C` to the foreground group missed it), with the parent's blocked mask and
  ignored set. `SpawnEx2Args` gains six fields -- `pgid_mode`, `pgid`,
  `sigmask_set`, `sigmask`, `sigdefault`, `setsid` -- applied before the
  child's first instruction, in glibc's order: a new session, then the group.
  What cannot be had is `NotPermitted` (`EPERM`) and leaves no process.
- **Two faults found on the way, both in the teardown autoreaping would
  share:** `pcb::try_reap` -- the path every waited-for process takes --
  never released the references its file mappings held, nor its session's
  claim on a terminal; only `pcb::destroy` did. And on a multi-CPU machine a
  thread killed while another CPU ran it left that CPU on the process's page
  tables after they were freed: the switch away compared the *records* of the
  two tasks' address spaces, and the dead thread's had been cleared. Both
  switch paths now compare the live CR3, and a process's address space is
  freed only once every such thread is off its CPU (deferred, and drained by
  the boot thread's idle loop).

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. One kernel record of "ignored" for both ABIs (chosen)** | a signal ignored under one ABI stays ignored after an exec into the other | one answer to "is it ignored?" for every consumer: send, delivery, job control, SIGCHLD | the native libc must report every change to or from `SIG_IGN` |
| B. A native per-signal sigaction table in the kernel, beside the Linux one | the kernel would hold handlers too | `sigaction` could be one syscall | duplicates the libc's handler table, the bug shape §113 and §114 removed; handlers mean nothing across `exec` |
| C. Keep `SIG_IGN` in each ABI's own place and translate at exec | nothing new for Linux programs | smaller change | two records of one fact, and every consumer must ask both, by ABI |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| The native call carries the whole set | one signal per call | the libc's table is the authority for its own changes; one call per change is what it has to make anyway, and it is idempotent |
| A pending signal is discarded only when *newly* ignored by the whole-set call | discard every pending signal in the set | an unchanged action is not "set to SIG_IGN"; a blocked signal kept because it was ignored stays for whoever unblocks it. The Linux shim, which changes one at a time, discards as Linux does |
| `SIGKILL`/`SIGSTOP` in the set is `InvalidArgument` | clear them silently, as `SYS_SIGNAL_MASK` does | asking to ignore them is a caller bug; `sigaction` itself refuses it with `EINVAL` |
| The autoreap decision is taken under the process table's lock | read the parent's disposition before taking it | no wait can see the zombie between publication and decision, as Linux's `tasklist_lock` guarantees |
| The exiting thread releases its own process | a reaper task; or the parent | nothing else is sure to run: the parent asked never to wait, and the kernel has no reaper task. The thread is already off the process's page tables (`sched::detach_address_space`) |
| An autoreaped child's CPU time is not added to the parent's children's time | add it | POSIX counts waited-for children in `RUSAGE_CHILDREN`; Linux adds none it reaps this way |
| A spawned child inherits its parent's group and session by default | keep it leading its own | POSIX `posix_spawn`; no caller in the tree relied on the old behaviour (`login_tty` calls `setsid` itself) |
| A value with no meaning in the six fields is refused (`pgid` without its mode, a mode of 2) | ignore it | `struct_size`'s rule: a field the kernel will not read must be zero, or a request it cannot honour passes silently |
| Address spaces whose killed threads may still run wait in a queue drained by the idle loop | wait in the reaper | `on_thread_exit` can run in an exception handler, where nothing may wait |

**Revisit** if a native program needs `SA_NOCLDSTOP` or another flag only
the kernel can act on, or if killed threads are ever made to exit
themselves (as Linux's do), which would make the address-space deferral
unnecessary.
