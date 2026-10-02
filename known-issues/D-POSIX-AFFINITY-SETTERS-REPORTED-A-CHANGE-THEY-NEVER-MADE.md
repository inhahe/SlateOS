## D-POSIX-AFFINITY-SETTERS-REPORTED-A-CHANGE-THEY-NEVER-MADE — `sched_setaffinity` and `pthread_setaffinity_np` said a process was pinned to some CPUs, and it was not (lane D, 2026-09-27) — **Status: FIXED 2026-09-27 (they refuse what they cannot do); pinning waits on lane A**

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

**Still open:** a native call that reads and sets a named process's or
thread's mask, with the rule for who may -- lane A's, in
`requests/e-adf-what-the-process-explorer-still-cannot-ask.md`. When it
exists, `affinity_change` is where the route to it goes.
