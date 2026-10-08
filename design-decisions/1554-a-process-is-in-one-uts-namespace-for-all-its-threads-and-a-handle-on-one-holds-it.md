## 1554. A process is in one UTS namespace for all its threads, and a handle on one holds it

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a UTS namespace (a private copy of the computer's host name and
domain name, the names `uname` reports) can now be made with `unshare` or
`clone`, entered with `setns`, and named by opening `/proc/<pid>/ns/uts`.
That is what `unshare --uts`, `nsenter --uts` and every container runtime use
to give a container its own host name. On Linux each *thread* has its own
namespaces; here a whole *process* does, so `unshare` or `setns` in one thread
moves every thread of its process. Making or entering one needs root's
authority (an effective user id of 0) or the native right to manage
namespaces. The system's own names are the root namespace's; a container's
`--hostname` is now its own namespace too, rather than a per-process override
of `uname`.

**What exists now** (`kernel/src/utsns.rs`, `kernel/src/nsfs.rs`):

- Every process records the UTS namespace it is in (`pcb::Process::uts_ns`),
  inherited by fork and spawn. A namespace lives while a process is in it or a
  handle holds it, and goes with the last (`utsns::retain`/`release`).
- The **root** namespace's names are the system's (`fs::nameservice`), so the
  readers that never see a process -- sysfs, the kernel shell, network
  settings -- keep reading the same names, and setting them in the root
  namespace is what `sethostname` always did. Any other namespace holds its
  own copy, made from its creator's.
- `unshare(CLONE_NEWUTS)`, `clone`/`clone3` with `CLONE_NEWUTS`, `setns` with a
  namespace's descriptor or a pidfd, and `/proc/<pid>/ns/uts` as Linux has it:
  a link whose text is `uts:[N]`, which opens to a descriptor that `fstat`
  describes as nsfs's inode `N` on a device of its own
  (`fs::vfs::reserve_dev`), and which answers `NS_GET_NSTYPE`. The other
  namespace kinds are refused (`EINVAL` from `clone` and `setns`, `EPERM`
  from `unshare` as before) rather than accepted and not made: until this
  change `clone(2)` took the bits and made nothing.
- Containers: each has its own UTS namespace, named by `--hostname`, which
  every process it runs joins; replaced `ipc::namespace`'s per-process
  hostname override, which changed what `uname` said but not what a
  `sethostname` inside the container changed (the system's name).
- Native programs: `SYS_NAMESPACE_UNSHARE`, `_OPEN` (a `/proc/<pid>/ns`
  path to a handle), `_ENTER`, `_ENTER_PROCESS`, `_CLOSE` and `_INFO`
  (1161-1166), for lane D's libc `unshare()`, `setns()` and opens of
  `/proc/<pid>/ns/*` -- the native process keeps its descriptors in
  userspace, so it cannot be handed a kernel descriptor as a Linux process
  is.

**Decision 1 -- membership is per process, not per thread.**

| | Per process (chosen) | Per thread (Linux) |
|---|---|---|
| A thread that calls `unshare(CLONE_NEWUTS)` | moves its whole process | moves alone; its siblings keep the old namespace |
| What the rest of this kernel keys on | the process: net namespaces are per task but set at spawn, mount views and the path namespace (`ipc::namespace`) are per process, `/proc/<pid>` is the process | the thread |
| Programs that notice | a program that unshares in one thread and expects its other threads to keep the old names: rare -- `unshare(1)`, `nsenter(1)`, runc, podman and bubblewrap unshare in a single-threaded process or a fresh child | none |
| Cost | one field on the process | a namespace set per task, copied at every `clone(CLONE_THREAD)`, and every reader asking "which thread?" |

Linux itself refuses the per-thread change where it makes no sense --
`unshare(CLONE_NEWUSER)`, and `setns` into a user or mount namespace, are
`EINVAL` in a multithreaded process -- and the tools that use namespaces are
written for a single thread. A thread-level namespace would be a second notion of "where a
task is" beside the process-level ones the kernel already has. Revisit if a
ported program is found that relies on threads in different UTS namespaces.

**Decision 2 -- who may.** Root's authority, as Linux asks `CAP_SYS_ADMIN`:
for a Linux program, an effective user id of 0 (`proc::setid`) or a
`Namespace` capability with `WRITE` -- the native authority to make and
attach namespaces that `SYS_NS_CREATE` and `SYS_NS_ATTACH` already ask for;
for a native program, that capability alone, as every native call takes its
authority from capabilities and none from a user id. Without user
namespaces there is no unprivileged way in, as on a Linux with
`kernel.unprivileged_userns_clone=0`. The checks run in Linux's order --
`CLONE_NEWUSER` refused first, then the privilege, then a kind not built --
so an unprivileged program is told `EPERM` whatever it asked for.

**Decision 3 -- a handle is an ordinary descriptor-backed object.** Opening
`/proc/<pid>/ns/uts` takes a hold on the namespace, recorded in the
process's `ipc_handles` as `(ResourceType::Namespace, nsfs::encode(kind,
id))`: exit gives it back, fork's child takes its own, `SCM_RIGHTS` carries
one, exactly as for pipes and sockets. One hold per process per namespace,
shared by every descriptor the process has on it, as for every such object
here. The record reuses `ResourceType::Namespace` rather than adding a wire
type, as `Service` is both the right to register a name and the record of a
registered listener: the right lives in the capability table, the record in
`ipc_handles`, and neither is mistaken for the other. A new type would have
grown the capability ABI that lane D mirrors for nothing a program can see.

**Not yet:** the other kinds of namespace (mount, IPC, network, PID, user,
cgroup, time) -- the roadmap's 5.5 item "The Linux namespace calls" --,
`/proc/<pid>/task/<tid>/ns`, `/proc/thread-self/ns`, and `NS_GET_USERNS`,
which answers `EPERM` until a user namespace can be named by a handle
(`known-issues/A-ONLY-THE-UTS-NAMESPACE-HAS-LINUX-CALLS.md`).
