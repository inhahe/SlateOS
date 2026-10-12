## 1601. The disk usage widget lists the disks mounted, each by where it is mounted

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the desktop's widgets included a "Disk Usage" that drew only
its name and that nothing offered. "Add widget > Disk usage" now puts one on
the desktop showing each disk the system has mounted: where it is mounted
("/", "/mnt"), how much of it is in use ("73%"), a bar, and what is free of
what it holds ("12.1 GiB free of 45.0 GiB"); the bar turns red past nine
tenths full. A disc mounted read-only says what it holds instead, and is
never a warning.

**Where:** `gui/desktop/src/fs_space.rs` (`parse_mounts`, `NOT_DISKS`,
`disks`, `mounted_disks`; `Space`'s `read_only`, `used_percent`,
`nearly_full`), `gui/desktop/src/widgets/disks.rs` (`reading`, `Sampler`,
`rows`, `render`), `gui/desktop/src/widgets/listing.rs` (the list layout
the world clock shares), `gui/desktop/src/widgets/accessible.rs`
(`WidgetPart::Disks`, `Disk`), `gui/desktop/src/lib.rs` (`sample_disks`,
`live_readings`, the desktop menu's `MENU_ADD_DISK_USAGE`),
`gui/pathcodec/src/lib.rs` (`display_bytes`).

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The mount table** (`/proc/self/mounts`) and the kernel's `SYS_FS_STATVFS` for each disk | the disks under `/sys/devices/block` | A file system's space is what fills up; a device's capacity says nothing of what is in use -- and a disk not mounted has no space anyone can use. | A disk not mounted is not shown. |
| **Leave out what keeps nothing on a disk by its type, then what has no blocks** (as `df`) | list the types that are disks | A disk of a type nobody listed is shown, as it should be, rather than hidden until someone adds its name; the type list keeps the kernel's views out when nothing can be measured to tell them apart. | A new view of the kernel's with blocks would be shown until it is listed. |
| **Named by where it is mounted** | a label ("System", "Data") | SlateOS's mount table gives no device or label (`none` for every source), and a mount point is exact. "System" for `/` would be a claim: on today's images the system's files are not all on `/`. | A path reads less friendly than a name. |
| **Binary units** ("GiB") | decimal, as a vendor sells a disk | What a file system holds and has free is counted by the kernel, which the house writes in binary (`textfmt::bytes`); the file manager's sizes are binary too. | The figure is smaller than the one on the box. |
| **Nearly full at 90% in use, red, of a disk that can be written** | the same colour always | A disk about to fill is the one thing this widget exists to warn of; red is a warning no accent changes, and the share is said in words too. A read-only one is as full as it was made. | One threshold for every size of disk. |
| **Read every 10 seconds, while one is out** | every second, as the system monitor | Space moves slowly, and each disk asked is a system call. | A file written fills the bar up to ten seconds late. |

A disk the kernel would not measure is listed as "Not measured", not left
out: it is a disk not measured, not no disk. A mount table that will not
read says "The disks could not be listed", and tools are told why.

**Also:** the world clock's and this widget's lists share one layout
(`widgets::listing::stack`) and one "N more" (`listing::more_text`), which
the upcoming-events widget says too; there were two copies of each. And a
widget that has never been updated is due at once
(`WidgetInstance::last_updated` is `None` until it is): a widget restored at
sign-in was first due a whole interval after the session's clock started --
a disk usage widget empty for ten seconds, a system monitor for one.

**Not decided here:** whether a click on the widget opens the file manager
at the disk is part of `open-questions/C-Q36.md`.
