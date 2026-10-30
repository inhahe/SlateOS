## A-LINUX-DRIVER-PROCESS-READS-FILES-UNCONFINED (lane A, 2026-10-29) — OPEN

**Status:** OPEN

**In short:** a Linux driver process that loads firmware must be given the
right to read files, and that right is not limited to its firmware: with
it, the process can read every file the user it runs as can -- and driver
processes run as root. A driver for a WiFi card, say, is code its maker
wrote and nobody here reviewed; if it, or its device, were turned against
the system, it could read anything on disk. Nothing ships like this yet:
only the layer's own test process is given the right, and e1000e needs no
firmware.

**Where.** A driver process reads firmware through the system's ordinary
file calls with its own authority (design-decisions 1590): `SYS_FS_OPEN`
needs any capability of type `File` with `READ`
(`require_cap_type` in `kernel/src/syscall/handlers.rs`), which says
nothing about which files, and the kernel starts driver processes with no
identity of their own (`identity: None` in `kernel/src/linuxdrv.rs`'s
`start`), which leaves them root.

**The proper fix.** Either, and both are worth having:
- **Run each driver process as an identity of its own, unprivileged**, so
  that ordinary permissions confine it: `/lib/firmware` readable by it,
  nothing private. Needs a driver user (or one per driver) agreed with
  lane B, who owns the user database, and every driver process's
  capabilities named rather than inherited -- they already are.
- **A file capability scoped to a directory**, so that a process given
  `/lib/firmware` to read can read only beneath it: a capability-based
  answer independent of users. Needs the kernel's file capabilities to
  carry a path or a directory handle, and `SYS_FS_OPEN` to check it
  (the `openat2`-like pinned calls already resolve beneath a directory).

When the first driver that needs firmware runs on a real system (WiFi,
roadmap 2.1), it must not be started with the unconfined right.
