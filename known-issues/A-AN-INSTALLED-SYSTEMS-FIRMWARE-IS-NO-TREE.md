## A-AN-INSTALLED-SYSTEMS-FIRMWARE-IS-NO-TREE (lane A, 2026-10-30) — OPEN

**Status:** OPEN

**In short:** a Linux driver process may read firmware only from a
directory on a filesystem nobody can write (design-decisions 1595). The
boot medium's firmware is one; an installed system's `/lib/firmware`, on
its ordinary writable disk, is not -- so a driver started on an installed
system finds no firmware there, and says so once at boot. Nothing ships
from such a system yet: every machine that runs a firmware-loading driver
today boots with its firmware on the boot medium.

**Where.** `linuxdrv::firmware_reading` (`kernel/src/linuxdrv.rs`) gives a
driver a tree for `/lib/firmware` only when `fs::trees::register` accepts
it, which it refuses for a directory on a writable filesystem
(`PermissionDenied`), printing `[linuxdrv] /lib/firmware is not on a
read-only filesystem ...` the first time.

**Why the rule.** With no file permissions enforced, anyone who may write
the directory's filesystem could hard-link any file into it, and a driver
holding the tree would read it. A read-only filesystem cannot be changed
under the tree.

**The proper fix.** One of:
- **Install firmware on a filesystem of its own, mounted read-only** at
  `/lib/firmware` -- an image the package manager rebuilds, or a read-only
  bind of a directory only root may change. Needs the installer and the
  package manager (lane D's rootfs recipe) to lay it out that way.
- **Enforce file permissions,** so a directory only root may write is fixed
  against everyone else, and let a tree name a directory whose every
  ancestor only root may write. Needs DAC (file permissions) enforced in the
  VFS, which it is not.

Trigger: the first installed system that runs a driver needing firmware.
