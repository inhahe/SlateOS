## 1217. System Restore keeps restore points of every program's settings and data, copied into the backup tool's store

**Date:** 2026-09-27
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit; the system-wide half is `open-questions.md` E-Q3

**In short:** System Restore showed five invented restore points under a
banner saying they were not real, and could take or restore nothing. It now
takes real ones -- of the one folder where every program on this system keeps
both its settings and its data (`~/.config/slateos`: the notes library, the
calendar, the address book, the appearance settings). A restore point is a
copy of that folder, kept in the store the backup tool uses; restoring one
makes the folder match it again, after first keeping the folder as it is as a
restore point of its own, so a restore can be undone. The system's own files
-- its programs, boot configuration, services, packages -- are shown and not
offered: this program cannot read or replace them, and says so.

**Why copies, and why this folder.** `design.txt` asks for snapshots "with
branching like a VM does" and "options for what to include (files and
directories, programs, program data, program settings)", and imagines them on
a copy-on-write filesystem. SlateOS runs on ext4, which has no snapshots, so a
restore point has to be a copy -- and `apps/snapstore` makes one cheap: a file
unchanged between points is stored once. Of what the design lists, "program
data" and "program settings" are the user's own, in one folder the user may
read and write; they are what a copy can take and put back safely while the
system runs. Everything else needs privilege and a way to replace files a
running system is using, which is a different program.

**How a restore behaves, and the choices in it:**
- *It makes the folder match the point* -- files added since are removed, not
  just changed files put back. Anything less is not the state the point
  recorded. What the point could not read when it was taken is never removed.
- *It keeps the folder first* -- a "Before restoring to ..." point -- so the
  restore is undone by restoring that. Windows' System Restore does the same.
- *It cannot be abandoned half-way.* The window stays open, and a close
  request waits for the work: a folder half restored is worse than either
  state.
- *Programs still running may save over what is put back.* The confirmation
  says to close them first; nothing here can close them.

**Where the list of restore points lives:** in the store's own folder
(`~/.local/share/slateos/restore-points`), never in the folder a point
captures -- otherwise restoring an old point would bring back an old list and
lose every newer point.

**The schedule** runs only while System Restore is open (nothing else on the
system runs it), is off until turned on, and its retention policy removes only
the scheduled points: a point somebody took before a risky change is theirs.

**Alternatives:**
- *Wait for a copy-on-write filesystem* and keep the window a demonstration
  until then -- years of a program that cannot do what it is for.
- *Copy the system's files too*, as root -- replacing a live system's files
  from a user program is the dangerous half, and belongs to E-Q3.
- *Let the user add any folder* (Documents, projects) -- the design allows it,
  and the store would; it needs a folder picker and a list of chosen folders,
  and is the natural next step, not a different design.
- *Restore without removing added files* -- simpler and safer-sounding, but
  the result is a state no point ever recorded.
