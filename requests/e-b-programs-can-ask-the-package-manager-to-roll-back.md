# Lane E -> lane B: a program can ask the package manager for its generations and to go back to one

**Filed:** 2026-10-09 by lane E. **For:** lane B (`userspace/pkg`).
**Status:** OPEN.

**In short:** the operator decided (E-Q3, design-decisions §1237) that a bad
system update is undone by going back a package generation, not by System
Restore copying system files. `pkg` already does that from the command line
(`pkg generations`, `pkg rollback [GENERATION]`). The desktop has no way to
offer it, because there is nothing a program can call: only `pkg`'s printed
output, which is written for a person and changes with its wording. Nothing
is broken while this waits. System Restore says that an update is undone with
`pkg rollback`, and the desktop shows no generations.

## What lane E needs

1. **The generations, as data.** For each one: its number, when it was made,
   what made it (install, remove or upgrade, and of what), and whether it is
   the current one. Either a library the desktop links (`pkg`'s database
   reader as a crate), or a stable machine-readable output
   (`pkg generations --json`, one object per generation), whichever you
   prefer to keep stable.
2. **Going back, without a terminal.** A rollback that asks nothing
   interactively and reports its outcome as data (or as documented exit
   codes): done, no such generation, not permitted, failed partway (and in
   what state it left the system).
3. **Who may do it.** A rollback changes the whole system, so a desktop
   program presumably cannot do it by itself. Please say which permission or
   which privileged path a desktop program goes through. If that path is not
   yours (a service of lane D's, or a permission prompt of lane A's), say
   whose, and lane E will ask there.

## Where lane E will use it

- System Restore (`apps/systemrestore`): an "Undo the last update" entry
  beside the restore points.
- Settings' system pages (`apps/settings`), listing the generations next to
  the filesystem snapshots it already shows from `/proc/snapshots`.
