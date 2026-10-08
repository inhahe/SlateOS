### A-ONLY-THE-UTS-NAMESPACE-HAS-LINUX-CALLS -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN -- the remaining work of roadmap 5.5's "The Linux namespace
calls"; the first kind landed on lane-a-wip 2026-10-08.

**In short:** a Linux program can now give itself a private host name
(`unshare --uts`, `clone(CLONE_NEWUTS)`), enter another process's
(`nsenter --uts`, `setns`), and name one by opening `/proc/<pid>/ns/uts`.
None of the other six kinds of namespace can be made or entered that way
yet: a private mount table, network, process list, user ids, IPC objects,
cgroup view or clock. A program that asks for one is refused, never told it
has one it has not got.

**What exists** (design-decisions 1554): `kernel/src/utsns.rs` (the UTS
namespaces), `kernel/src/nsfs.rs` (handles on namespaces, Linux's nsfs),
`/proc/<pid>/ns/uts`, `setns` by handle and by pidfd, `NS_GET_NSTYPE`, and
the same for native programs as `SYS_NAMESPACE_*` (1161-1166). Ring-3
tests: `spawn::self_test_linux_uts_namespaces` (`build/nstest.c`, checked
against Linux 6.6 as root) and `spawn::self_test_native_namespaces`
(`build/nsnative.c`).

**What is missing, and how each is answered now:**

| Missing | Answer today | Where it would go |
|---|---|---|
| `CLONE_NEWNS`, `NEWIPC`, `NEWNET`, `NEWPID`, `NEWUSER`, `NEWCGROUP`, `NEWTIME` | `EINVAL` from `clone`, `clone3`, `unshare` and a pidfd's `setns` (`EPERM` first for a caller without the privilege, as Linux orders it; `clone(CLONE_NEWNS)` `ENOSYS`, as before); natively `NotSupported` | a kind in `nsfs::NsKind` each, over what the kernel already has: `fs::mount_ns` and `ipc::namespace` (mounts), `netns` (network), `pidns` (process ids), `userns` (id maps) |
| `/proc/<pid>/ns/{mnt,net,pid,user,ipc,cgroup,time,pid_for_children,time_for_children}` | absent from the directory; opening one `ENOENT` | the same kinds |
| `/proc/<pid>/task/<tid>/ns`, `/proc/thread-self/ns` | absent | trivial once wanted: a thread's namespaces are its process's here |
| `NS_GET_USERNS` | `EPERM` | a handle on the owning user namespace, once user namespaces have handles |
| `/proc/self/{uid_map,gid_map,setgroups}` for `unshare --map-root-user` | absent | the user namespace kind |

**Divergence kept on purpose:** namespaces are a process's here and a
thread's on Linux, so `unshare` or `setns` in one thread moves all its
threads (design-decisions 1554, decision 1).

**Who waits:** util-linux's `unshare` and `nsenter` (lane B,
`requests/b-ad-unshare-and-nsenter-wait-on-unshare-and-setns.md`), through
lane D's libc `unshare()`/`setns()`, which can use `SYS_NAMESPACE_*` for
`--uts` now (`requests/a-d-the-namespace-calls-have-native-numbers.md`);
and any later runc, podman or bubblewrap.
