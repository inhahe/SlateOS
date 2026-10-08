### A-CONTAINER-CHILDREN-ESCAPE-THEIR-CONTAINER -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN -- fixed on lane-a-wip 2026-10-08, awaiting a boot on main.

**In short:** a container's first program was confined to the container (its
own root directory, its volumes, its hostname, its network). Every program
*that* program started was not. A shell in a container ran each command on
the host's file system, under the host's hostname, on the host's network.

**Where.** The container layer confines a process by tables keyed on its pid
(`ipc::namespace`: `PROCESS_ROOT`, `PROCESS_MOUNTS`, `PROCESS_ROOT_RO`,
`PROCESS_HOSTNAME`) and by its task's `net_ns` (`sched`). `fork`
(`proc::fork`) and spawn (`proc::spawn`) passed on only the namespace id
(`PROCESS_NS`, the bind/hide rules), and ignored a failure to do even that. A
new task never inherited `net_ns`: `sched::spawn_inner` inherited the creator's
cgroup but started every task in the root network namespace. Threads shared
their process's pid-keyed jail, but not its network.

**Fix (lane-a-wip).** `ipc::namespace::inherit(parent, child)` copies all of
it. `fork` and spawn call it and fail closed: no child rather than an
unconfined one. `sched::spawn_inner` gives a new task its creator's
`net_ns` as well as its cgroup, read with a blocking lock (the `try_lock` it
used could fall back to the root group under contention). Test:
`namespace::self_test`'s `test_inherit`: a child gets its parent's
namespace, root, volume, read-only root and hostname, and resolves a path as
its parent does.

**Reproduce (main).** `container run --rootfs <dir> <image> /bin/sh -c 'cat
/etc/hostname'`: the shell's `cat`, a child, reads the host's file.
