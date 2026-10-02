## 42. Container runtime increments 3–4 — per-process filesystem root (chroot) and `oci run` launching the jailed entrypoint

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). Same §40 pre-approval of the
container-runtime port; this records the implementation choices for the rootfs/
jail and the `oci run` launch path that §41 explicitly deferred.

**Context.** After §41, a container could launch an init process, but that
process resolved every path against the **host** filesystem — `/bin/sh` was the
host's `/bin/sh`, not the container image's. Real container isolation needs the
init process jailed to the container's rootfs. The host already had two relevant
mechanisms: the per-process `ipc::namespace` Bind/Hide path-translation hook
(consulted first in `Vfs::resolve_follow`), and an `fs::overlay` module accessed
by ID (not VFS-mounted). `sys_chroot` was an EPERM-only Linux gate ladder — no
real per-task VFS root existed.

**The decision.**
1. **Implement chroot as a dedicated per-process *root*, not as a Bind rule.**
   A Bind rule `{ "/" → "/containers/x/rootfs" }` would re-anchor paths but
   **cannot clamp `..`**: a guest path `/../etc` would normalize (after the
   prefix is applied) to a host path *above* the rootfs — a jail escape. So
   `ipc::namespace` gains a `PROCESS_ROOT` map with `set_root`/`clear_root`/
   `get_root`. `resolve_path_for` applies Bind/Hide first (guest path space),
   then re-anchors under the root via `apply_root`, which **normalizes within
   the jail with `..` clamped at the root** (`normalize_jailed`: popping an
   empty stack stays at root, exactly like Linux chroot) before prefixing. This
   makes escape structurally impossible rather than relying on a later check.
2. **Key the jail on the global PID, not the task id.** VFS resolution looks the
   root up via `current_task_id() → owner_process()`, i.e. the PID; child threads
   share the process, so they inherit the jail for free. (Contrast §41's
   scheduler resources, which are keyed on the *task id*. The container binding
   path now sets *both* correctly: cgroup/net-ns by task, jail by PID.)
3. **`Container` gains a `root_path` field + `set_root_path` (Created-only).**
   `add_process_task` reads it and calls `set_root(pid, root)`;
   `remove_process_task` calls `clear_root(pid)`. `run()` therefore launches the
   init already jailed. Changing the root of a running container is rejected
   (it would not retroactively re-jail live processes).
4. **`oci run` launches the entrypoint jailed to the extracted rootfs.** It sets
   `root_path` to the extracted `lower` tree, reads `command[0]` from the host
   path inside that tree, and `container::run`s it with the image's argv+env.
   The manual `container start` stub is now only a fallback (no entrypoint /
   unreadable binary / spawn failure), preserving `container exec` usability.

**Alternatives considered.**
- *Bind-rule chroot* — rejected for the `..`-escape reason above; the clamp is
  the whole point.
- *A per-task root in the scheduler `Task` struct* (mirroring Linux `fs_struct`)
  — viable, but path resolution already routes through `ipc::namespace` per
  *process*, and threads should share one jail, so the PID-keyed map in the
  existing module is the lower-friction home and avoids a second resolution hook.
- *VFS-mount the overlay so the jail routes through copy-on-write* — deferred.
  The overlay is ID-addressed today; mounting it into the path tree is a larger
  change. For now the jail points at the extracted `lower` dir, so image writes
  land there directly (documented limitation in `known-issues.md`).
- *Relative-path jailing* — `apply_root` only jails absolute paths; relative
  paths are left for the (not-yet-jailed) per-process cwd layer. Documented as a
  limitation rather than silently half-jailing.

**Deferred (still open).** Overlay-backed CoW rootfs (VFS-mounted upper);
per-process cwd jailing; a mount-namespace field + `pivot_root` semantics on
`Container`; dynamic-linker/interpreter presence checks for the entrypoint; a
userspace `docker`/`podman` CLI (Python/fastpy per CLAUDE.md).

**Where it bites.** `kernel/src/ipc/namespace.rs` (`PROCESS_ROOT`, `set_root`/
`clear_root`/`get_root`, `apply_root`/`normalize_jailed`, `resolve_path_for`,
`detach`, self-test "Process filesystem root (chroot)");
`kernel/src/container.rs` (`root_path`, `set_root_path`, `add_process_task`/
`remove_process_task` jail wiring, self-test 18); `kernel/src/kshell.rs`
(`container rootfs` subcommand, `Rootfs:` in `container info`, the `oci run`
launch path).
