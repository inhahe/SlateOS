### B-SPAWN1. `posix_spawn`/`vfork` child loses the exec-failure errno under CoW-fork degradation — KNOWN LIMITATION (acceptable)

**Symptom:** When a glibc `posix_spawn(3)` (or `vfork`) child fails its
`execve` (e.g. the target is missing), the parent observes a child that exited
with status 127 rather than receiving the precise `errno` glibc's posix_spawn
normally reports.

**Root cause:** glibc's posix_spawn does `clone3({CLONE_VM|CLONE_VFORK|
CLONE_CLEAR_SIGHAND, ...})` expecting a **shared** address space: on exec
failure the child writes `errno` to a stack location the parent then reads. Our
processes are address-space isolated, so `linux_clone_inner`'s VFORK_SPAWN
branch degenerates the shared-VM vfork to a copy-on-write fork. The child runs
on its own copied stack, so a post-fork write to the (formerly shared) errno
slot is invisible to the parent — only the child's exit status survives. The
common success case is unaffected (the child execve's and never writes back).

**Proper fix (deferred):** Genuine `CLONE_VM` shared-address-space semantics for
the vfork window, or a kernel-mediated errno relay from the failing child's
exec path back to the parent's clone return. Deferred until a workload depends
on the precise errno; status-127 is the universally-understood "exec failed"
signal and is what shells display anyway.
