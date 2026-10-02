## 46. Container runtime increment 9 — volume (bind) mounts layered on the chroot jail

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). Same §40 pre-approval of the
container-runtime port; this records the implementation choices for the volume /
bind-mount mechanism, which is the first concrete piece of the broader
"mount-namespace / `pivot_root`" work that §42/§45 deferred as TD32's remaining
scope. It is an implementation choice (how mounts compose with the chroot jail),
not an operator policy fork, so it is resolved autonomously and recorded here.

**Context.** After increments 6–8 a container's init process is jailed to a
copy-on-write overlay rootfs and every absolute/relative/`*at` path is contained.
But a real container runtime must also let a container *share a host directory* —
Docker's `-v /host/dir:/data`. The existing chroot (`apply_root`) re-anchors
**every** guest path under the single rootfs prefix, with no way for a subtree to
point somewhere else. The pre-existing `ipc::namespace` Bind/Hide rules (step 1
of `resolve_path_for`) operate purely *within* the guest path space — a Bind
rewrites a guest path to another guest path that step 2 then prefixes with the
rootfs — so they cannot express "this guest subtree lives at an arbitrary host
location outside the rootfs." A new mechanism was needed.

**The decision.**
1. **Per-process volume table** (`PROCESS_MOUNTS: BTreeMap<pid, Vec<VolumeMount>>`
   in `namespace.rs`), each entry mapping a normalized guest prefix → an absolute
   host target. Keyed on the **global PID**, exactly like `PROCESS_ROOT`, so
   child threads inherit it and PID-reuse safety is handled by clearing it in
   `detach()` (alongside the chroot).
2. **Resolution composes volumes *over* the chroot, not as a step-1 Bind.** A new
   `apply_root_with_volumes()` runs in step 2 *after* `normalize_jailed` clamps
   `..` against the guest root `/`. The longest-matching volume prefix wins and
   the path is re-anchored under that volume's host target; otherwise the path
   falls through to the normal rootfs prefix. Putting volume matching **after**
   `..`-normalization is the security-critical choice: a guest cannot use
   `/data/../../etc` to climb out of a volume into the host, because the path is
   collapsed to `/etc` (clamped at the guest root) *before* any volume is
   considered, so it simply resolves under the rootfs. The empty-volume-list fast
   path keeps the common (no-volume) jailed process on the original `apply_root`.
3. **Reverse mapping** (`unjail_path_for`, used by `fchdir`/`*at`) also reverses
   volumes — a host path inside a volume target maps back to the volume's guest
   prefix (longest host-target match) — so `getcwd` inside a volume reports the
   guest path and a subsequent relative op is jailed exactly once (no double-jail,
   consistent with §45). Checked before the rootfs strip because a volume's
   contents live outside the rootfs subtree.
4. **Container plumbing.** `Container` gains a `volumes: Vec<(guest, host)>`
   field; `add_volume_mount(id, host_target, guest_prefix)` (Docker `-v` order,
   Created-state-only) records them; `add_process_task` installs each via
   `namespace::add_volume` after `set_root`; `remove_process_task`/`delete` clear
   them. Last-writer-wins on a repeated guest prefix (mirrors Docker re-mount).

**Alternatives considered.**
- *Implement volumes as step-1 Bind rules.* Rejected: step-1 rewrites stay in
  guest space and are then rootfs-prefixed, so they can't escape the jail to an
  arbitrary host path — exactly the thing a volume must do. They also wouldn't
  get `..`-clamped the same way, opening an escape.
- *A full mount-tree (longest-prefix mount table that subsumes the rootfs as the
  `/` mount).* The cleaner long-term model and the eventual `pivot_root` target,
  but a wholesale replacement of the just-stabilized chroot path (increments 7–8)
  carries real regression risk for no immediate functional gain. Volumes-over-
  chroot is additive, leaves the hardened chroot untouched, and delivers the
  user-visible `-v` feature now. The mount-tree refactor remains TD32's deferred
  scope.
- *Resolve volumes before `..`-clamping.* Rejected outright — it would let
  `/data/../../etc` escape a volume into the host. Normalization must come first.

**Limitations / deferred.** Volumes apply only to a *jailed* process (a volume on
an unjailed process is ignored — volumes are a container concept). No read-only
volume flag yet (Docker `-v ...:ro`); no `tmpfs`/named-volume types — these are
straightforward follow-ups on the same table. The `unjail` reverse mapping is
ambiguous only if a volume target is nested *inside* the rootfs subtree (it
prefers the volume), which does not occur for normal host-dir volumes.

**Where it bites.** `kernel/src/ipc/namespace.rs` (`PROCESS_MOUNTS`,
`add_volume`/`clear_mounts`/`volume_count`, `apply_root_with_volumes`,
`longest_volume_match`, `unjail_path_for` volume reversal, `detach` cleanup,
`test_volume_mounts`); `kernel/src/container.rs` (`volumes` field,
`add_volume_mount`, `add_process_task`/`remove_process_task`/`delete` wiring,
self-test 19).
