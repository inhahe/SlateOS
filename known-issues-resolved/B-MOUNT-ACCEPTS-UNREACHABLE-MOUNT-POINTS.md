### B-MOUNT-ACCEPTS-UNREACHABLE-MOUNT-POINTS. `Vfs::mount` succeeds when the mount point's parent does not exist, producing a filesystem nothing can reach — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/fs/vfs.rs`, `kernel/src/fs/overlay.rs`)

**Symptom.** The overlay filesystem self-test failed on every boot, so the boot
test reported `Boot test FAILED (BOOT_OK reached but a self-test failed)`:

```
[overlay]   commit: OK (applied 1 changes)
[vfs] Mounted overlay filesystem at '/mnt/ovl-cow-test' (rw)
WARNING: Overlay filesystem self-test failed: NotFound
```

Every one of the twelve preceding overlay sub-tests passed, including
`read lower: OK` reading the very file that then failed. The failure was the
first VFS-routed read after the mount.

**Why it looked like an overlay bug and was not.** The log line says the mount
succeeded, so the investigation naturally went to the overlay engine and its
VFS adapter — `normalize_rel`, `layer_join`, `OverlayFs::stat`/`metadata`, the
page-cache route in `read_file_routed`. All correct. The mount really had
succeeded; it was simply unreachable.

`Vfs::resolve_inner` walks every non-final path component and requires each to
exist in its containing filesystem, where "exists" includes being a mount point
(`resolve_mount`'s longest-prefix match maps it to the mounted fs). Resolving
`/mnt/ovl-cow-test/file_a.txt` therefore fails on the **first** component:
nothing creates `/mnt` at boot, `/mnt` is not itself a mount point, so
`lstat("mnt")` against the root memfs returns `NotFound` and the walk aborts
before the mount table is ever consulted. `/proc`, `/dev` and `/sys` work
because their parent is `/`.

**Root cause.** `Vfs::mount_with_options` validated only that the path was
absolute and not already mounted. It never checked reachability, so it happily
registered a mount that consumed an `fs_id`, printed a success line and
appeared in `/proc/mounts` while being addressable by nothing.

**Fix.** `mount_with_options` now stats the mount point's parent and refuses
the mount unless it is an existing directory (`NotADirectory`, or the stat's
own error). Only the *parent* is required, not the mount point itself: Linux
requires the mount point directory to exist, but our boot sequence mounts
`/proc`, `/dev` and `/sys` over a root memfs that has no such directories, and
longest-prefix matching makes the mount point itself reachable regardless.
Requiring the parent is the weakest condition that makes "registered" and
"reachable" mean the same thing. The check runs *before* `VFS.lock()` is taken,
since `stat` re-enters the VFS.

Overlay self-test 13 now creates `/mnt` before mounting.

**Also fixed: the test hid which step failed.** Test 13 used bare `?` on four
fallible calls, so the only diagnostic was
`Overlay filesystem self-test failed: NotFound` — naming neither the operation
nor the path. Each step now reports its name and error and unwinds the mount
and the scratch tree, which is what turned this from a guessing exercise into a
one-boot diagnosis.

**Bug class.** Same shape as the `pthread_join`-returns-`Ok(0)` defect fixed the
same day: an operation that cannot do what was asked reports success, and the
damage surfaces later somewhere unrelated. Worth auditing other registration
APIs that take a path and validate only its syntax.
