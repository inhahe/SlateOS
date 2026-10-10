# A drive's recycle-bin limits wait until the bin is opened

**Status:** OPEN (lane E, found 2026-10-09)

**In short:** each drive's recycle bin now has limits -- an age, a size, a
number of items (design-decisions §1238, §1240) -- and they are applied when
the file manager's Recycle Bin is opened. §1238 also says they are applied
"when a drive appears", so that a stick that was away for a month is pruned
the moment it is back. Nothing does that yet: a stick's bin is held to its
limits only when someone opens the Recycle Bin while the stick is in. Nothing
is lost by the wait -- items are kept longer than the limit says, never
shorter -- but a stick that is never looked at in the file manager keeps
everything.

**Where:** `apps/recyclebin/src/bins.rs` -- `Bins::prune_all` is what to call;
`apps/explorer/src/main.rs` -- `open_recycle_bin` is its one caller.

**Proper fix:** the recycle-bin service §1238 gives to lane D, which watches
mounts, calls `prune_all` (or its own equivalent over the same on-disk format)
for each drive as it is mounted, and periodically while it stays mounted. Until
it exists, a program that hears of a new drive could call it; none in lane E
does (the desktop announces devices to the shell, not to applications). When
the service lands, lane E's part is to stop pruning in the file manager, or to
keep it as a second chance -- whichever the service's design says.
