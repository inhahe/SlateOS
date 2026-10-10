## 1238. The home recycle bin is SlateOS's: one per drive, with each drive's own limits, and a service takes it over later

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended this option) · **Lane:** E

Answering E-Q4 with option **C, reached through B**. The operator's answer,
verbatim, from `operator-answers/2026-10-09-open-questions-answers.txt`:

> E-Q4: Claude's recommendation. By the way, I think the plan was to have other
> options for automatically deleting than just "the disk is getting full", such
> as automatically deleting files that have been there over a certain amount of
> time?

**In short:** the recycle bin that the file manager, the image viewer and Disk
Cleanup already use (`~/.recycle`, which keeps file names exactly) is the one
SlateOS builds on. Next, each drive gets a bin of its own, so deleting from a USB
stick no longer copies the file onto the system disk. Later, a background service
takes the bins over, and the kernel's delete call hands files to it, so a delete
typed at the command line reaches the same bin. The kernel's separate bin
(`/_TRASH`) is built on no further.

**The operator's question back.** Yes: the plan already has it, and lane A said
so in chat on 2026-10-09. `roadmap-detailed.md` ("Per-drive auto-delete
(retention) policy") gives each drive's bin its own limits, stored on that drive
so they travel with it: delete items older than a number of days, keep the bin
under a size, keep it under a number of items. A system-wide default applies to
a drive that has set none. The limits are applied when a drive is connected, from
the deletion times recorded with each item, so a stick that was away for a month
is pruned when it comes back. Running short of space remains one trigger among
these, not the only one.

**What it obliges.**

- Lane E (`apps/recyclebin`): a bin per drive, and each drive's limits (age,
  size, count), applied when the bin is opened and when a drive appears; Disk
  Cleanup and the file manager show each drive's bin.
- Lane A: nothing more on `/_TRASH`. Lane A asked to be told when the service
  exists, so that its delete call can hand files to it.
- Lane D: the recycle-bin service, when it is built. It takes the home bin's
  format with it, so nothing a user has deleted is stranded by the move.
