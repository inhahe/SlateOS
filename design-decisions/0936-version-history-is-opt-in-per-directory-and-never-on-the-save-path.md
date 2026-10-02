## 936. Version history is opt-in per directory, and never on the save path

**Date:** 2026-09-13 · **Decided by:** Operator · **Lane:** A

Answering A-Q10 with two of its options together: *on only where it is asked
for*, **and** *do the work after the save returns*.

**In short:** every save currently reads back the old contents and checksums
them so the last 16 versions can be recovered. That is measured at about half
the cost of saving a small file. From now on the history is **off by default**
and enabled per directory, and where it *is* enabled the read-back and checksum
happen after the write has returned to the caller.

**Why both and not either.** Opt-in alone makes saving fast by removing the
feature for almost everyone, and a user who assumed the history was there finds
it missing exactly when it mattered. Backgrounding alone keeps the feature for
everyone but pays its cost on every write in the system, including the vast
majority of files nobody will ever want a previous version of. Together, the
common path costs nothing at all, and the directories someone deliberately
turned it on for get the history without paying for it synchronously.

**The cost this accepts, stated plainly.** A crash in the window between the
save returning and the history entry being written loses that one version. That
window is real, not theoretical, and it is the price of the write not waiting.
It is bounded to a single version of a single file, and only in directories that
opted in.

**Consequence for the benchmark.** `performance-targets.md`'s filesystem row
carries a note that its comparison is unlike-for-unlike because ext4 does no
versioning. With the default off, the ordinary write path *is* now comparable,
so that note should be revisited rather than left standing.
