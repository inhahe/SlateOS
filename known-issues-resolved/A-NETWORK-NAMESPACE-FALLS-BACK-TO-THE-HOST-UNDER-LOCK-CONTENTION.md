### A-NETWORK-NAMESPACE-FALLS-BACK-TO-THE-HOST-UNDER-LOCK-CONTENTION -- 2026-10-08 -- FIXED 2026-10-09 (lane A)

**Status:** FIXED 2026-10-09 -- fixed on lane-a-wip 2026-10-08, on main since b083cfeca (lane A's publish of 2026-10-09, boot-tested green at 3d83e0264).

**In short:** a program in a container is meant to use the container's own
network (its address, routes and name server). When the kernel was busy on
another processor at the moment the program opened a connection, bound a
port or looked up a name, the kernel quietly used the host's network
instead. Nothing reported it; the program simply escaped its container's
network for that one operation.

**Where.** `sched::current_task_net_ns` read the calling task's network
namespace with `SCHED.try_lock()` and answered the root namespace -- the
host's -- whenever the scheduler lock was held elsewhere, which on a
multiprocessor machine is routine. Its callers: `SYS_TCP_CONNECT`,
`SYS_TCP_LISTEN` and `SYS_UDP_BIND` (`syscall::handlers`), name resolution
(`net::dns`), the kernel shell's container exec and the container
self-test. The same shape was fixed on 2026-10-08 for a new task's
placement (`sched::creator_placement`, known-issues
A-CONTAINER-CHILDREN-ESCAPE-THEIR-CONTAINER); this is its reader.

**Fix (lane-a-wip).** It blocks for the lock. Every caller is a system call
or the kernel shell, none of which holds the scheduler lock, so waiting is
safe; answering the wrong namespace never is.

**Left as is, and why.** `sched::current_task_cgroup` has the same
`try_lock` fallback to the root cgroup, documented as being for allocation
paths that may not block. It has no caller (`#[allow(dead_code)]`); whoever
wires it into the memory controller must decide what a contended read
answers -- the root group would let a container's allocations escape its
limit, as this did its network.

**Reproduce (main).** Hard to trigger on demand: a container process's
`SYS_TCP_CONNECT` while another CPU holds the scheduler lock connects from
the host's namespace.
