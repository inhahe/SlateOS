## §673 — a cross-route check must establish that the object held still, and a clause that could not be judged is voided, never passed

**Date:** 2026-09-01
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** The conformance harness compares a file's size as reported by two
different calls — listing the directory, and asking about the file directly —
and complains if they disagree. It complained about `/proc/heapinfo`, and it was
wrong: that file's contents are *generated* each time you ask, and they include
counters that the harness's own memory allocations were bumping between the two
calls, so the file genuinely was a different length by the time the second call
ran. Both answers were true when given. The check was assuming the file would
hold still and never checking that it did. Now it takes the listing twice, once
before and once after, and only compares when the two agree — and when they
don't, it says so out loud and counts the check as *unjudged* rather than
quietly passing it.

### The defect

`check_meta` asserted `entry.size == meta.size` for regular files. Both are
"the file's size", so they must agree — for an object that did not change
between the two readings. The harness cannot freeze a filesystem, and never
established that it had.

`gen_heapinfo` formats `slab_allocs`, `slab_frees`, `total_allocs` and friends.
Walking `/proc` allocates. So the string grows a digit somewhere between the
`readdir` that measured it and the `metadata` that measured it again, and the
harness reported a driver defect where there was only elapsed time.

This survived a boot in which the heap was completely clean (`2507` clauses
held, `1` broke), which is what ruled out its being collateral from the
allocator bug being chased at the same time.

### The decision

Bracket the stat between two listings — `d1`, stat, `d2` — and:

| Observation | Verdict | *What changes* |
|---|---|---|
| `d1 == d2`, stat agrees | **pass** | as before |
| `d1 == d2`, stat differs | **fail**, after one tight confirming re-read | a failure now means the size held still and the routes still disagreed |
| `d1 != d2` | **void** | the log prints a `VOID` line naming what moved; the closing line carries the count |

The third row is the substance. A voided clause is counted in its own field,
never in `passed`.

### Why not the alternatives

- **Report `st_size = 0` for generated files, as Linux does.** Rejected. It is a
  user-visible behaviour change made to satisfy a test, and it destroys real
  information: SlateOS's `/proc` sizes are currently honest, and a reader or a
  `cat` that preallocates benefits from that. Changing the subject under test
  because the test was wrong is the wrong direction of fix.
- **Declare procfs "volatile" in a per-backend table and skip the clause.**
  Rejected, though it was the first design. A declaration is a promise about
  the future written by whoever last thought about it; the bracket is a
  *measurement* taken on the boot in question. The declaration would also be
  coarse — it would switch the clause off for every file on procfs, including
  the ones that are perfectly stable and where the check has value.
- **Silently skip when the sizes disagree.** Rejected outright, and this is the
  general rule the entry is really about: a check that declines to run and says
  nothing reads as coverage in the log, which is strictly worse than no check.
  `A-GATES-SILENTLY-STOPPED-CHECKING` is the same failure in a different
  subsystem.
- **Accept the small residual and skip the tight re-read.** The bracket is exact
  only for a monotonic size. `slab_active` is allocs-minus-frees and can return
  to a previous value, which would let a genuine disagreement be reported on a
  boot where nothing was wrong. The re-read runs only where a failure was about
  to be printed, so it is free on the common path, and a harness whose failures
  are sometimes noise trains its reader to ignore all of them.

### If it is never revisited

Nothing degrades. The risk to watch is the opposite of the one just fixed: a
backend that begins churning every file's size would void its way to a clean
run. That is why the voided count is printed on the closing line rather than
only on the individual `VOID` lines — the number is the thing that makes such a
regression visible.
