## 1237. System Restore stays about the user's files; a bad update is undone through package generations

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended this option) · **Lane:** E

Answering E-Q3. The operator's answer, verbatim, is in
`operator-answers/2026-10-09-open-questions-answers.txt` ("E-Q3: Claude's
recommendation").

**In short:** System Restore keeps restore points of each program's settings and
data, and does not copy the system's own files. Undoing a bad system update is
done in the package manager, which will keep each set of installed packages as a
"generation" that can be gone back to. Whole-disk snapshots come only if SlateOS
adopts a copy-on-write filesystem (one that can keep an old version of the disk
without copying it). Nothing copies a running system's files over themselves,
the one approach that can leave a machine that does not start.

**The alternative not taken:** a privileged service copying `/etc`, `/boot` and
`/usr` in and out, as the settings folder is copied (option A). It needs a
restart into a restore mode to replace files in use, and an interruption at the
wrong moment leaves a system that does not boot.

**What it obliges.**

- The generations exist already: `userspace/pkg` (lane B) makes one per
  install, removal and upgrade, and `pkg generations` and `pkg rollback` list
  and return to them from the command line.
- Lane E: System Restore says, for the installed programs and the packages,
  that an update is undone by going back a generation, and keeps saying plainly
  that it does not cover the system's other files
  (`apps/systemrestore/src/points.rs`). A window that lists the generations and
  goes back to one waits for a way for a program to ask the package manager
  for that, rather than parsing `pkg`'s printed output
  (`requests/e-b-programs-can-ask-the-package-manager-to-roll-back.md`).
- A copy-on-write filesystem is not on the roadmap (`design.txt`: "ext4 first");
  whole-disk snapshots wait for it.
