### [E] System Restore showed five invented restore points, and its schedule added more -- 2026-09-27
**Status:** FIXED (lane E, 2026-09-27) for every program's settings and data. The system's own files are not covered, and say so: `open-questions.md` E-Q3.

**In short:** System Restore opened on five restore points nobody had taken
("Initial Setup", "After System Update v1.1" ...) under a banner saying they
were not real. Its weekly schedule, switched on with a record of automatic
points nobody had taken, added another invented one within a minute of
opening. Compare made up files, packages and settings in proportion to the
days between two points. Create refused -- and then added the point to the
list anyway. It now keeps real restore points of the folder every program
keeps its settings and data in, restores them for real, and compares what
they hold.

**What was wrong, one by one:** the tree was built in `new`; sizes were fixed
estimates per component ("~2 GB" of system files); `check_schedule` added a
point to the list and nothing to any disk; `compare_snapshots` generated
`/system/lib/module_N.so` and "core-libs 1.2.0 -> 1.3.0"; `begin_create`
showed "Cannot create" and then called `create_snapshot`; the create form's
component boxes could not be changed and its "branch from the selection"
described files that cannot be taken as they were in another point; the
Schedule view had no control at all.

**Now:** `apps/systemrestore/src/points.rs` -- where things are
(`Locations`), which components can be kept (`SnapshotComponent::source`),
the saved list (`to_text`/`parse`/`load`/`save`, refused whole when damaged
and never written over), and the work on its own thread (`Worker`: take,
restore after keeping the folder first, delete then free the space), all on
`apps/snapstore`. The window: an overlay of real progress that cannot be
abandoned half-way, a close that waits for the work, a restore dialog that
says what will happen and to close other programs first, the current point
marked, a Schedule view whose controls work and which says it runs only while
the window is open, a retention policy that removes only scheduled points,
and a Compare view of the files two points hold. §1217 says why.

**Not done:** folders the user chooses (the design allows it; it needs a
folder picker), and the system half (E-Q3).
