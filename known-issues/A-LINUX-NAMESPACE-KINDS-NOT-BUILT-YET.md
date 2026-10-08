### A-LINUX-NAMESPACE-KINDS-NOT-BUILT-YET -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN -- the remaining work of roadmap 5.5's "The Linux namespace
calls"; the UTS and mount kinds landed on lane-a-wip 2026-10-08.

**In short:** a Linux program can now give itself a private host name
(`unshare --uts`) and a private set of mounts (`unshare --mount`), enter
another process's (`nsenter --uts --mount`, `setns`), and name either by
opening `/proc/<pid>/ns/uts` or `/proc/<pid>/ns/mnt`. None of the other five
kinds of namespace can be made or entered that way yet: a private network,
process list, user ids, IPC objects, cgroup view or clock. A program that
asks for one is refused, never told it has one it has not got.

**What exists** (design-decisions 1554, 1555): `kernel/src/utsns.rs` and
`kernel/src/fs/mntns.rs` (the namespaces), `kernel/src/nsfs.rs` (handles on
them, Linux's nsfs), `/proc/<pid>/ns/{mnt,uts}`, `setns` by handle and by
pidfd, `NS_GET_NSTYPE`, and the same for native programs as
`SYS_NAMESPACE_*` (1161-1166). Ring-3 tests:
`spawn::self_test_linux_uts_namespaces` (`build/nstest.c`),
`spawn::self_test_linux_mount_namespaces` (`build/mntnstest.c`), both
checked against Linux 6.6 as root, and `spawn::self_test_native_namespaces`
(`build/nsnative.c`).

**What is missing, and how each is answered now:**

| Missing | Answer today | Where it would go |
|---|---|---|
| `CLONE_NEWIPC`, `NEWNET`, `NEWPID`, `NEWUSER`, `NEWCGROUP`, `NEWTIME` | `EINVAL` from `clone`, `clone3`, `unshare` and a pidfd's `setns` (`EPERM` first for a caller without the privilege, as Linux orders it; `NEWUSER` `EINVAL` before anything, as Linux without user namespaces); natively `NotSupported` | a kind in `nsfs::NsKind` each, over what the kernel already has: `netns` (network), `pidns` (process ids), `userns` (id maps); IPC and time have nothing yet to isolate |
| `/proc/<pid>/ns/{net,pid,user,ipc,cgroup,time,pid_for_children,time_for_children}` | absent from the directory; opening one `ENOENT` | the same kinds |
| `/proc/<pid>/task/<tid>/ns`, `/proc/thread-self/ns` | absent | trivial once wanted: a thread's namespaces are its process's here |
| `NS_GET_USERNS` | `EPERM` | a handle on the owning user namespace, once user namespaces have handles |
| `/proc/self/{uid_map,gid_map,setgroups}` for `unshare --map-root-user` | absent | the user namespace kind |
| What a mount namespace still lacks | see known-issues A-MOUNT-HAS-NO-BIND-OR-MOVE | bind and move mounts, propagation, `pivot_root(2)` |

**Divergence kept on purpose:** namespaces are a process's here and a
thread's on Linux, so `unshare` or `setns` in one thread moves all its
threads (design-decisions 1554, decision 1).

**Who waits:** util-linux's `unshare` and `nsenter` (lane B,
`requests/b-ad-unshare-and-nsenter-wait-on-unshare-and-setns.md`), through
lane D's libc `unshare()`/`setns()`, which can use `SYS_NAMESPACE_*` for
`--uts` and `--mount` now (`requests/a-d-the-namespace-calls-have-native-numbers.md`);
and any later runc, podman or bubblewrap.
