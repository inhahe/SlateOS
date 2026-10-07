## D-POSIX-AFFINITY-SETTERS-REPORTED-A-CHANGE-THEY-NEVER-MADE — `sched_setaffinity` and `pthread_setaffinity_np` said a process was pinned to some CPUs, and it was not (lane D, 2026-09-27) — **Status: FIXED 2026-09-27 (they refuse what they cannot do); pinning: both halves are written -- lane A's `SYS_SCHED_SET_AFFINITY`/`GET_AFFINITY` and `SYS_CPU_CURRENT` (1100/1101, 1092, on `lane-a`) and lane D's route to them (2026-10-07). It closes when lane A's kernel is on `main` and `services/ctest-affinity` passes in a lane D boot.**

**In short:** `taskset -c 0 -p 1234` asks the C library to keep process 1234 on
CPU 0. The library checked the request and said it had worked, and nothing
changed: 1234 went on running on every CPU. Nothing in the system can confine
a program to some of its CPUs yet -- so the library now says that ("function
not implemented"), except for the one request it can honour truthfully:
"every CPU", which is what every program already has.

| Call | Was | Now |
|---|---|---|
| `sched_setaffinity`, `pthread_setaffinity_np`: a mask of every online CPU | success | success -- it is the mask in force |
| the same, a narrower mask | success, nothing applied | `-1` / `ENOSYS` |
| the same, no online CPU in the mask | `EINVAL` | `EINVAL` |
| `sched_getaffinity(pid)`, a pid that is not there | every CPU | `-1` / `ESRCH` |
| `pthread_getaffinity_np` | all 1024 bits, CPUs that do not exist included; any mask shorter than 128 bytes refused | the online CPUs, zeroes after; 8 bytes accepted, as Linux takes |
| `pthread_setaffinity_np`, a mask shorter than 128 bytes | `EINVAL` | read as Linux reads one: zero-extended |

`ENOSYS` is the answer glibc gives where the kernel has no such call; hwloc and
the OpenMP runtimes take it to mean "affinity is not available here" and carry
on, rather than failing.

**Where:** `posix/src/sched.rs` (`affinity_change`, `affinity_target_exists`,
`read_affinity_mask`, `fill_affinity`), and `posix/src/pthread.rs`'s two calls
over them. pthread.rs's own copy of `cpu_set_t` went in the same change: the
two had to be checked against each other in `abi_layout.rs` because a duplicate
type is what drifts.

**Why nothing can pin:** the scheduler has a per-task CPU mask
(`kernel/src/sched/mod.rs`, `Task::cpu_affinity`, `set_cpu_affinity`), but
it is set only inside the kernel -- its own tasks at spawn, and the kernel
shell's `taskset` debugging command. The native ABI has no call that sets it,
and the Linux ABI's `sys_sched_setaffinity` (`kernel/src/syscall/linux.rs`)
checks its arguments and applies nothing. (A mask set from the kernel shell is
therefore invisible to `sched_getaffinity`, which answers "every CPU".)

**Lane A's half** (on `lane-a`): `SYS_SCHED_SET_AFFINITY` and
`SYS_SCHED_GET_AFFINITY` read and set a process's mask -- every thread it has
and the ones it starts later -- or, with `SCHED_AFFINITY_THREAD`, one
thread's, with signalling's rule for who may; `SYS_CPU_CURRENT` says which CPU
the calling thread is on. Since 2026-10-01 a process's id is its first
thread's, one id space as Linux's.

**Lane D's half, done 2026-10-07:** `posix/src/sched.rs`
(`native_set_affinity`, `native_get_affinity`, `cpu_current`) and
`posix/src/pthread.rs`. The C calls name a *thread*, as Linux's do:
`sched_setaffinity(0, ...)` is the calling thread, `sched_setaffinity(pid,
...)` the thread with that id -- a process's first -- and
`pthread_setaffinity_np` the thread given, so the thread form of lane A's
calls is the one they use. The errnos are Linux's: no CPU the machine has
`EINVAL`, no such thread `ESRCH`, another's `EPERM` (not the shared mapping's
`EACCES`). `sched_getcpu` and `getcpu` ask `SYS_CPU_CURRENT`, and stop asking
on a kernel without it, where they still answer CPU 0. A kernel without the
affinity calls gets the 2026-09-27 behaviour above, unchanged.

The kernel's mask is one `u64`: CPUs above 63 in a caller's mask are CPUs the
machine has not got, which Linux ignores as well.

`services/ctest-affinity` checks what a program sees: pinned to each CPU in
turn, a thread reads back that CPU and `sched_getcpu` says it is there; a new
thread inherits its creator's mask; `pthread_setaffinity_np` pins another
thread; and a mask with no CPU, no such process and pid 1 are refused. It
passes on Linux as an ordinary user.
