## 43. VFS dispatch holds a *per-mount* lock, not the global VFS lock — enabling stacked filesystems (overlay) and removing the I/O-under-global-lock anti-pattern

**Date:** 2026-06-30

**Decided by:** Claude (autonomous). This was a structural fix forced by
implementing the §42-deferred overlay-backed CoW rootfs (container increment 5):
VFS-mounting the overlay deadlocked, and the proper fix is a foundational change
to VFS lock granularity that benefits the whole filesystem layer. No operator
fork — there is one correct design (don't hold a global lock across filesystem
I/O), and CLAUDE.md mandates the proper fix over a hack.

**Context.** The overlay engine (`fs::overlay`) reads/writes its lower and upper
layers through ordinary VFS paths. To give containers real copy-on-write rootfs,
increment 5 wraps a live overlay in an `OverlayFs` adapter implementing the
`FileSystem` trait and mounts it into the path tree. But the VFS held its single
global `Mutex<VfsInner>` **across every filesystem method call** (e.g.
`read_file_routed` did `let vfs = VFS.lock(); … return mp.fs.read_file(relative)`
*inside* the locked scope). When `Vfs::read_file("/mnt/ovl/x")` called
`OverlayFs::read_file`, that re-entered `Vfs::read_file("<lower>/x")` to fetch the
backing bytes → re-acquire the same non-reentrant spinlock → **hard deadlock**
(observed: boot hung immediately after mounting the overlay). Holding a global
lock across I/O is also independently an anti-pattern CLAUDE.md calls out (it
serializes *all* filesystem I/O system-wide on one mutex).

**The decision.** Change `MountPoint.fs` from `Box<dyn FileSystem>` to
`Arc<Mutex<Box<dyn FileSystem>>>` — i.e. give **each mount its own lock**. A new
`resolve_mount(path)` helper takes the global `VFS` lock only long enough to do
the longest-prefix mount-table lookup, clones the `Arc`, copies the stable
`fs_id`/`MountOptions`/relative path, and **drops the global lock**. Every one of
the ~50 dispatch sites then locks the returned *per-mount* handle to run the
actual operation. Because a stacked filesystem's lower layers live on *different*
mounts (different `Arc`s, different locks), an overlay method can freely re-enter
the VFS: it briefly re-takes the global lock to resolve the lower mount, then
locks that mount's *own* lock — never the overlay's — so there is no reentrancy
on any single lock.

Consequences/details:
- **Atomicity is now per-mount, not global.** Operations that needed two steps
  to be atomic w.r.t. one filesystem (RENAME_NOREPLACE's exists-check + rename;
  the cache-identity capture + remove/rename/truncate) now hold that mount's lock
  across both steps — same guarantee, scoped to the mount. Cross-mount checks
  (same-mount rename, hard-link, `RENAME_EXCHANGE`) now compare handles with
  `Arc::ptr_eq` instead of comparing mount-path strings.
- **Iteration sites** (`sync`, `mount_info`, `trim_device`, `debug_stats`,
  `mounts`/`mounts_full`) snapshot the `Arc` handles (or clone the matching one)
  under a brief global lock, then call the filesystem lock-free — so even a
  stacked filesystem's `statvfs`/`sync` cannot deadlock during a full-VFS scan.
- **Page-cache fill** (`read_at_routed`/`read_file_routed`) drops the per-mount
  guard *before* calling `page_cache::read_through`; the fill closure
  (`fill_file_page`) re-resolves and locks freshly, so the cache lock and the
  per-mount lock never nest and a file's own lock isn't held across its fill.
- `cache_identity` was re-signatured to take the already-locked
  `&mut Box<dyn FileSystem>` + `fs_id` (it used to take `&mut MountPoint`).
- `mount`/`mount_with_options` wrap the incoming `Box` in `Arc::new(Mutex::new(…))`;
  callers are unchanged (still pass a `Box<dyn FileSystem>`).

**Why `Arc<Mutex<Box<dyn>>>` and not alternatives.**
- *Convert all `FileSystem` methods to `&self` + interior mutability* — would let
  a bare `Arc<dyn FileSystem>` work, but is a far larger, riskier change touching
  every filesystem impl, and the per-fs `Mutex` we need anyway preserves the
  "one operation at a time per filesystem" assumption every impl was written
  against.
- *A reentrant/recursive global lock* — rejected: reentrant locks hide bugs, our
  spinlock has no stable thread identity to key on, and it would not fix the
  I/O-under-one-global-lock serialization problem.
- *Make the overlay bypass the VFS for its layers* — impossible in general: a
  layer path can span arbitrary mounts and needs full VFS resolution.

**Result.** Overlay self-test 13 ("VFS mount adapter — CoW routing") passes:
reading through the mounted overlay returns the merged (lower) view, writing
copies up into the upper layer, and the lower layer is never mutated — all via
ordinary `Vfs::read_file`/`write_file` on the mount path. Full kernel boots
clean (BOOT_OK); no new clippy warnings (baseline unchanged). This unblocks the
§42-deferred overlay-backed CoW container rootfs (next: mount an `OverlayFs` at
each container's rootfs and point the chroot jail at it).

**Where it bites.** `kernel/src/fs/vfs.rs` (`MountPoint.fs` type, `resolve_mount`
helper, `cache_identity` signature, and ~50 dispatch sites converted from
global-lock-held to per-mount-lock); `kernel/src/fs/overlay.rs` (`OverlayFs`
adapter + self-test 13). Tech-debt TD32's "VFS-mount overlay" half is now
unblocked (the lock barrier that made it deadlock is gone).
