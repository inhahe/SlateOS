## B-A-DIRSYNC-NEVER-CONVERGED-BECAUSE-COPIES-LOST-THEIR-TIMESTAMPS (lane A, 2026-08-22) — FIXED 2026-08-22

**In short:** Directory sync copied a file but stamped the copy "now" instead of
carrying over the original's modification time. Since sync decides what to copy
by comparing modification times, the copy never matched its source — so the next
sync copied it again, and so would every sync after that. A backup of an
unchanged tree would re-copy the entire tree, every time, forever.

**Found by** reading a *passing* test's output rather than a failure.
`dirsync::self_test`'s `test_compare_identical` logged `Compare: 0 new, 1
modified` under a line that said `compare identical: ok`, because the test
asserted only on the new-file count and carried the comment `// Modified check
depends on timestamps; they may differ.` — which is precisely the bug, written
down and excused.

**Mechanism.** `compare` treats a file as modified when its size *or* its
`modified_ns` differs (`dirsync.rs:190`). `copy_file` called
`Vfs::write_file(dst, &data)`, and a write sets `modified_ns` to the current
time. So immediately after a successful sync, every copied file differs from its
source in `modified_ns` and compares as modified. There is no fixed point.

**Fix.** `copy_file` now reads `Vfs::metadata(src)` *before* the write and
applies `Vfs::set_times(dst, accessed_ns, modified_ns)` after it. Reading before
matters for the degenerate src == dst case, where reading after would stamp the
file with the time the copy itself just created. A failure to read or apply the
times is non-fatal but logs a warning naming the consequence ("it will be
re-copied on every sync") rather than passing silently, because a silent
timestamp failure is indistinguishable from this bug.

`test_compare_identical` now syncs and then re-compares, asserting that zero
files remain modified and that the file compares *unchanged* — i.e. asserting
convergence directly, which is the property the excusing comment gave up on.

**Severity.** Real for any consumer: unbounded redundant I/O proportional to
tree size on every sync, and `compare` could never report a tree as in sync.
Nothing consumed dirsync at boot before this week.
