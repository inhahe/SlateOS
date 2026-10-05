# B → A, D: `unshare` and `nsenter` wait on `unshare(2)` and `setns(2)`

**Filed:** 2026-10-01 by lane B. **Addressed to:** lane A (namespaces, the
container runtime) and lane D (the C library). **Status:** OPEN, and not
urgent: nothing is asked for now. This records a dependency so that it is
seen when the container runtime is designed.

## In short

`unshare` (run a command in fresh namespaces -- its own view of the mounts,
the process list, the host name...) and `nsenter` (run a command inside
another process's namespaces) both refuse on SlateOS, because the C library's
`unshare()` and `setns()` check their arguments and then return `ENOSYS`. They
used to run the command anyway, unisolated, which was worse
(`TD-B-UNSHARE-RAN-THE-COMMAND-UNISOLATED-AND-SAID-IT-HAD-NOT`,
`TD-B-NSENTER-RAN-THE-COMMAND-IN-THE-WRONG-NAMESPACE`).

Under design-decisions §1049 a command that does not work is kept only while
what it waits for is planned. These two were judged today and kept, because
lane A owns namespaces and carries "Container runtime / Docker equivalent" in
its backlog, and design.txt says Docker "needs container primitives
(namespaces, cgroups equivalent). Plan for these in your kernel design." This
file is the link between the two, so the runtime's design can count these
tools among its callers.

## What the two tools call, so the interface can be checked against them

- **`unshare`** (util-linux): `unshare(2)` with `CLONE_NEWNS`, `CLONE_NEWUTS`,
  `CLONE_NEWIPC`, `CLONE_NEWNET`, `CLONE_NEWPID`, `CLONE_NEWUSER`,
  `CLONE_NEWCGROUP` and `CLONE_NEWTIME`; for `--map-root-user`, writes to
  `/proc/self/uid_map`, `/proc/self/gid_map` and `/proc/self/setgroups`; for
  `--fork`, a child that is PID 1 of the new PID namespace; and
  `mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL)` to make the new mount
  namespace's mounts private.
- **`nsenter`** (util-linux): `/proc/<pid>/ns/{mnt,uts,ipc,net,pid,user,cgroup,time}`
  that open to namespace handles, and `setns(fd, nstype)` on them.

Any container tool ported later -- `runc`, `podman`, `bubblewrap` -- calls
the same set, which is the argument for providing the Linux-ABI calls over
whatever native form the runtime takes. The kernel's existing per-process
mount namespaces (`fs::mount_ns`) and its namespace bookkeeping
(`fs::prociso`) may be where they start.

## What lane B does then

Ports util-linux 2.39.3's `unshare.c` and `nsenter.c`, as `lsns` was, measured
against WSL's, and the refusals become real calls.
