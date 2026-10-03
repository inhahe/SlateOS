## TD-A-A-A-PHASE-BREAKDOWN-BUILT-BY-SUBTRACTING-MINIMA-IS-NOT-A-DECOMPOSITION (lane A, 2026-09-11) — **method defect in `bench_vfs_write_breakdown`; it manufactured a 26 µs phantom**

**In short:** I built a tool that splits the cost of a file write into labelled steps, and
then chased a large "unaccounted" leftover it reported. The leftover was mostly an
artifact of how the tool does its arithmetic, not time the kernel actually spends. Three
separate mistakes stacked up, and the leftover is an *upper bound* on hidden cost rather
than a measurement of it. Nothing shipped wrong; the risk was spending a day optimising a
function that was never slow.

### 1. The arithmetic does not decompose

`remainder = resolved_only.min_ns - Σ(phase.min_ns)`.

Each phase is timed in its **own 200-iteration loop** and contributes its own *best*
iteration. The whole is also its own best iteration. But the minimum of a sum is
`>=` the sum of the minima, and equality needs every component to hit its personal best
*in the same iteration*. With nine components that essentially never happens, so the
residual silently absorbs all of the co-occurrence slack.

**Consequence: a large residual is consistent with no hidden cost at all.** It is an
upper bound on unmeasured work, and it was being read as a quantity. The honest forms are
to subtract *means* from the mean (slack still there, but not systematically one-signed),
or to label the number as the bound it is.

### 2. A module-name collision meant the wrong function was subtracted

The `ns` phase times `crate::ipc::namespace::check_writable(raw)`. `write_file_resolved`
calls **`crate::fs::vfs::check_writable`** — a different, private function in a different
module that happens to share the name. So the phase credited 5 ns to the chain for a
function that is not in it, and the real `check_writable` stayed unmeasured inside the
residual. Two functions with one name, one import away from each other, and the benchmark
picked the wrong one without a compile error because both take a `&Path` and return
`KernelResult<()>`.

### 3. The component list was wrong: nine, not two

I recorded that the residual "contains exactly two things" — the dcache scan and memfs's
own write — and redirected the next work item onto memfs from that. Reading
`write_file_resolved` line by line, the unmeasured set is: `vfs::check_writable`,
`enforce_quota_write` (distinct from the `charge_bytes` the `quota` phase times),
`history::try_auto_record`, `resolve_mount`, `fs.lock()`, memfs's `write_file`,
`cache_identity`, `page_cache::invalidate_identity`, and
`dcache.invalidate_negative_prefix`. Four of those are private to `vfs.rs`, which is
*why* they were skipped — and skipping them was never recorded.

### Three bounds I tried to put on it from data already in hand, and why each failed

Worth keeping, because each looked sound:

* **"`metadata_now_ns()` is an MMIO clock read, like the journal bug."** No — it is
  documented as deliberately `clock_realtime()` (TSC) precisely so a file is not stamped
  1970; the comment at `vfs.rs:199` predates me and is correct.
* **"`index` was 4 115 ns and calls `Vfs::metadata`, so one path resolution is <= 4 µs."**
  No — `on_file_changed` early-returns on `!is_live() || !is_watched(path)` and may never
  reach `add_entry` at all, so that phase bounds nothing about resolution.
* **"memfs's `write_file` must hold it."** Its existing-file arm is `resolve_write_path`,
  `walk` (zero components for a root-level file), `child_ino`, three attribute tests,
  `clear` + `extend_from_slice(256)`, `touch_modified`. Nothing there is tens of µs.

Every nameable component being cheap is what finally indicted the method rather than the
kernel.

### The fix, not yet applied

Held back because a boot test is mid-build and `bench.rs` is in it.

1. Point the `ns` phase at the function actually called; `pub(crate)` the four private
   helpers so they can be timed at all.
2. Report the residual as an upper bound, and compute a mean-based residual beside it.
3. State in the serial line how many components the residual covers, so "unaccounted" can
   never again be read as "one thing I have not looked at yet."

### The generalisation

This session has now produced four errors of one shape: *a population I had not inspected,
summarised by a number I trusted.* A baseline median compared against its own member; a
42× asymmetry spanning a page-cache boundary; eight layout-sweep rows read as ordinary
runs; and now a residual read as a cost. The defence that worked every time is the same
one: **list the members and look at them before computing anything over them.** Here the
members were the nine call sites of a 38-line function.
