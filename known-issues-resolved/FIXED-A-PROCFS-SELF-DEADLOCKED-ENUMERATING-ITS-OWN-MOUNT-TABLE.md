### FIXED-A-PROCFS-SELF-DEADLOCKED-ENUMERATING-ITS-OWN-MOUNT-TABLE

**Status:** fixed 2026-08-23 (lane A). Recorded because the *shape* recurs and
the detection story is the interesting part.

**Symptom.** `ls /proc` and `cat /proc/fsstats` halted the kernel outright —
lockdep reported a self-deadlock, `sync.rs` panicked, the boot died. Found by
`fs::sysdiag`'s self-test the first time it ever ran at boot (it had been a
kshell-only suite until the de-fanging programme wired it in), and the panic
surfaced two subsystems away from the cause: sysdiag → `Vfs::readdir("/proc")`
→ procfs → `Vfs::mounts_full()` → `fs.lock()` on procfs.

**Cause.** The VFS holds a mount's per-mount lock for the whole duration of a
filesystem operation. Two procfs content generators walked the mount table and
locked *every* filesystem in it:

- `gen_mounts` (`/proc/mounts`, `/proc/<pid>/mounts`) → `Vfs::mounts_full()`,
  which locked each mount just to call `fs_type()`.
- `gen_fsstats` (`/proc/fsstats`) → `Vfs::debug_stats()` per mount.

One of those mounts is the procfs serving the read. `sysfs`'s `mount_count`
node had the same latent bug via `Vfs::mounts()`.

`readdir` is the worst reachable path because it sizes every root file by
*generating* it — so a bare `ls /proc` ran every generator with the lock held.

design-decisions §43 makes the per-mount lock safe to re-enter for *stacked*
filesystems, where the lower layer is a different lock. It cannot help a
filesystem that enumerates the table it is itself in.

**Fix.**
1. `MountPoint` now caches `fs_type: String` at mount time, so `Vfs::mounts()`
   and `Vfs::mounts_full()` lock no filesystem at all — only the global VFS
   lock. This removes the hazard by construction for every present and future
   caller, and is strictly faster. It fixes the sysfs node too, for free.
2. `Vfs::debug_stats_nonblocking()` added for the case that genuinely needs a
   per-fs lock; `/proc/fsstats` uses it and prints `(busy)` for the filesystem
   it is being read through. That is the only truthful answer — a filesystem
   cannot describe its internals mid-operation on itself.

**Why it hid for so long, and the lesson.** `procfs::self_test()` drives a
bare `ProcFs::new()` and calls its methods directly. The object under test was
never the *mounted* one, so no per-mount lock was ever held and no generator
ever re-entered. A virtual filesystem's suite must exercise it **through
`Vfs::`**, not through a detached instance; the direct calls test the
generators, not the filesystem. A four-call through-the-mount block now closes
`procfs::self_test()`.

**Not yet done (low priority).** `readdir` on a procfs directory generates the
full content of every file in it to report a size. Linux reports `0` for these
and readers read to EOF instead. Ours is O(entire /proc) on every `ls /proc`.
Changing it is a visible behaviour change (sizes in `ls`), so it is left as
tech debt rather than folded into a deadlock fix.
