## 1041. lsblk asks udev for a device's contents only where udev runs

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `lsblk -f` shows each device's filesystem type, label and
UUID. util-linux's lsblk gets them from udev's database -- which any user may
read -- and, when udev knows nothing of the device, by reading the device
itself (root only). Upstream built with libudev treats every device sysfs
lists as known to udev, even on a machine not running udev, and so shows
nothing there. SlateOS runs no udev. The port asks udev only when its
database directory exists, and otherwise reads the device -- which is
upstream's behaviour where udev runs and util-linux-without-libudev's where
it does not.

**The alternatives:**

| Option | What a user sees on SlateOS | Against upstream |
|---|---|---|
| Emulate libudev exactly | `lsblk -f` blank for everyone | identical everywhere |
| Never ask udev (build as without libudev) | root sees filesystems; others nothing | differs wherever udev runs, including the WSL reference |
| **Ask udev where `/run/udev/data` exists** | root sees filesystems; others nothing | identical where udev runs; differs only on a Linux host with libudev and no udevd (a container) |

Exact emulation is faithful to a configuration SlateOS does not have: it
would port the half of upstream's behaviour that only makes sense when udev
is present, onto a system where it never is. Never asking udev would make
the port untestable against the reference, whose udev answers first.

**Where:** `userspace/lsblk/src/props.rs` (`udev_is_running`,
`get_properties_by_udev`); todo.txt, lane B Judgment Calls, 2026-09-27.

**Revisit** if SlateOS grows a udev-like device database: lsblk should then
ask it first, as upstream asks udev.
