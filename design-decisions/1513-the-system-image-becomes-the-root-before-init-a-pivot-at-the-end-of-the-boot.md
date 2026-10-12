## 1513. The system image becomes the root before init: a pivot at the end of the boot

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** the disk image that holds the installed system -- its programs,
its accounts, its settings -- was mounted at `/mnt`, while `/` was a
filesystem the kernel built in memory. So no program found anything where it
looks: `/bin/sh` (which `popen`, `system` and every `#!/bin/sh` script need),
`/etc` (accounts, the list of services to start), `/usr/share/zoneinfo`,
`/home`. Nothing installed on the image could be started at boot. Now, at the
end of the boot and just before the first user program (init) starts, the
kernel makes the image the root, as Linux does when it switches from its boot
filesystem to the real one. Lanes B and D asked for this
(`requests/d-a-nothing-on-the-system-image-can-be-started-at-boot.md`,
`requests/d-ab-the-booted-system-has-no-bin-sh.md`), both recommending "the
image is the root".

**What changed:**
- **`Vfs::pivot_root(new_root, put_old)`**, Linux's `pivot_root(2)`: the
  mount at `new_root` becomes `/` with every mount beneath it, the old root
  moves to `put_old`, and every other mount -- `/tmp`, `/proc`, `/dev`,
  `/sys` -- keeps its path over the new root. Mounts keep their identity, so
  a file held open stays open; advisory locks taken by path move with their
  files. Everything is planned and checked before anything changes.
- **The boot, step 24:** if an ext4 image is mounted at `/mnt`, it becomes
  `/`; the in-memory root goes to `/.bootfs` and is unmounted unless a file
  on it is still held. A boot with no image keeps the in-memory root, as
  before.
- **The kernel's boot files** -- `/bin/hello`, `/bin/ticker` and the default
  `/etc/startup.conf` -- go onto the image only where it has none, so an
  image that provides its own service list and programs is used as it
  stands. The image recipe (lane D's) is asked to provide them.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Pivot at the end of the boot, before init (chosen)** | init and everything it starts see the image at `/`; the boot's self-tests run as before | no self-test or fixture changes; the end state both lanes asked for | the boot test's own fixtures still see the image at `/mnt`, so a fixture that needs `/bin/sh` while the battery runs still has none |
| B. Mount the image at `/` from the start of the boot | everything, the battery included, sees the final layout | one layout from power-on | 190 paths in the kernel and every lane-D fixture name `/mnt`; self-tests that write at `/` would write onto the image; needs `/mnt` kept as a second name for the image first |
| C. Links from today's root into `/mnt` (`/bin`, `/etc`, `/usr`) | standard paths resolve | small | two roots stay visible: `realpath`, `/proc/self/exe` and the mount table say `/mnt/...`, and `df /` reports the in-memory root (lane B's objection) |
| D. Init runs the image's services `chroot`ed to `/mnt` | services see the image as `/` | no kernel change | they lose `/dev`, `/proc` and `/tmp`, which belong to the in-memory root (lane D's objection) |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| The old root goes to `/.bootfs`, then is unmounted | free it in place, as Linux's `switch_root` deletes the initramfs | an unmount refuses while a file on it is held, so a held file keeps working rather than losing its filesystem; a directory name with nothing in it is the whole cost when nothing is held |
| `put_old` must be a direct child of `/` | anywhere | after the pivot only `/`'s own children are sure to resolve, whatever the new root contains |
| The pivot rewrites the mount table and advisory locks, not other path-keyed records (working directories, watches) | rewrite them all | it runs before init, when no process keeps any; a pivot of a running system would need them |
| The kernel's default service list and programs are written onto the image only where it has none | write them always, as onto the in-memory root | an installed system's image is its own; the boot test's image is attached with `snapshot=on`, so the writes it does make there never reach the file |

**Revisit** when the boot test's fixtures and self-tests use the standard
paths: then the pivot can move to the start of the boot (B), with `/mnt` kept
as a second name for the image for whatever still uses it.
