## 49. `container diff` is overlay-only (Docker semantics), and the container records its `OverlayId` rather than re-deriving it from the overlay name

**Date:** 2026-07-01
**Decided by:** Claude (autonomous) — resolves open-questions Q16, within the
operator-approved Docker/container-runtime port (Q15). Q16's OPEN entry
recommended this option (A); no operator input was solicited because it is the
only *proper* (non-band-aid) implementation and stays inside the approved scope.

Docker's `docker diff <ctr>` lists filesystem changes of a container relative to
its image: `A`dded / `C`hanged entries live in the writable upper layer, `D`eleted
entries are whiteouts. This is *defined* only for an overlay rootfs. Our runtime
has two rootfs kinds — overlay-backed (`oci run`, real lower/upper/whiteouts) and
plain bind-rootfs (`container create` + `rootfs <dir>`, a chroot to a host dir
with no base to diff against).

**The decision (Q16 option A).** Implement `diff` only for overlay-backed
containers; plain bind-rootfs returns `InvalidArgument` ("no overlay rootfs").
`container::diff(id)` resolves the container's overlay, walks the upper via an
**iterative work-stack** (`Vfs::readdir`, bounded kernel stack — not recursion),
classifies each entry with `overlay::which_layer` (`Both`→Changed, `Upper`→Added),
appends `overlay::whiteouts` as Deleted, and returns the list sorted by path,
each formatted `"/{rel}"`. Rejected: option B (point-in-time baseline captured at
first `start()`) because it is not Docker's semantics and puts a full rootfs walk
+ per-container manifest on the start hot path; option C (both) because two
meanings of "diff" under one command is confusing.

**Sub-decision — store the `OverlayId` on the container, don't re-derive it.**
`diff` needs to recover the overlay from a container id. Overlays are created as
`oci-{image_name}`, so it *could* be looked up by reconstructing that name — but
that breaks under rename and couples the container to the overlay's naming
convention. Instead `Container` gained an `overlay_id: Option<OverlayId>` field,
set on the `oci run` path via `set_overlay_id` (Created-state-only, mirroring
`set_rootfs_mount`). Robust identifier > reconstructed name (matches the
CLAUDE.md "store stable identifiers, not derived references" rule).

**Blocking `container wait` (increment 62, same series).** Independently, the
old `container wait` busy-polled `wait_status` in a `yield_now()` loop — the
CLAUDE.md-forbidden busy-wait. Replaced with event-driven `container::wait(id)`:
register `set_wait_task(init_pid, task_id)`, re-check terminal state (lost-wakeup
guard), then `block_current()`; the init process's `remove_thread` exit hook wakes
the task. Returns `WaitOutcome::{Exited(code), Removed}`. This is the same
join mechanism `sys_wait4` uses and the proven basis for a future real
`container exec` (Q17).

**Where it bites.** `kernel/src/container.rs` (`overlay_id` field +
`set_overlay_id` + `WaitOutcome`/`wait` + `DiffEntry`/`DiffKind`/`diff`, boot
self-tests 19k2/19k3); `kernel/src/fs/overlay.rs` (`upper_path`, `whiteouts`
accessors); `kernel/src/kshell.rs` (`container diff` arm, `container wait` rewrite,
`oci run` → `set_overlay_id`, `docker` passthrough + help/usage).
