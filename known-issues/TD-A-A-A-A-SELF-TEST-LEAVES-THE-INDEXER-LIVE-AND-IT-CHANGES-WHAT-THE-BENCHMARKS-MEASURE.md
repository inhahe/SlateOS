## TD-A-A-A-A-SELF-TEST-LEAVES-THE-INDEXER-LIVE-AND-IT-CHANGES-WHAT-THE-BENCHMARKS-MEASURE (lane A, 2026-09-11) — **benchmark cost depends on self-test ordering**

**In short:** a self-test switches the file indexer on and never switches it off, so
every benchmark that writes a file afterwards also pays for indexing it. Nobody chose
that. It is not wrong exactly — a real system would have the indexer running — but the
benchmark numbers depend on which self-test ran first, and nothing says so.

### The mechanism

* `is_live()` is `self.stats.initialized && self.stats.rebuild_count > 0`.
* `index::init` has exactly one caller in the tree: `kshell.rs:27006`, an interactive
  command. So on a normal boot the indexer should be dead.
* But `index::self_test` calls `init(cfg)` and then `rebuild()?` (its Test 5), and
  `rebuild_count` is incremented at `index.rs:245` and **never reset** — the only
  `rebuild_count: 0` in the file is the static initialiser.
* `default_config()` sets `watch_dirs: ["/"]`, and `path_in_subtree` returns `true`
  unconditionally when the root is `/`: *"Empty prefix, or `dir` was exactly "/": the
  whole tree."*

So from the moment that self-test runs until the end of the boot, every `Vfs::write_file`
to any path also runs `index::add_entry`, which resolves the path again via
`Vfs::metadata`.

### Three things this corrects, two of them mine from earlier today

**1. The `index` phase's 4 115 ns is real work, not lock overhead.** I had reasoned that
`on_file_changed` early-returns at `!is_watched`, so that phase bounded nothing about
path resolution — and used that to reject a bound I had placed on `cache_identity`. The
early return does not happen. The phase is a genuine `Vfs::metadata` plus an index
insertion, which is why 4 µs was plausible when two lock acquisitions would not have been.
It also means the `add_entry` single-resolution fix committed today **is** on a measured
path.

**2. The index is not an A/B confounder at all.** I listed it as *bounded* by the 4 µs
phase. It is *eliminated*: `/` as a watch root subsumes `/tmp`, so both arms pay exactly
the same index cost and it cancels in the difference. A better answer than the one I
wrote, for a reason I had not looked up.

**3. What the tell was.** `ns` measured 5 ns and `access` 23 ns in the same run, so the
harness resolves single digits. 4 115 ns for what I believed was one uncontended spinlock
acquire is two to three orders of magnitude off, and I wrote that number down twice
without noticing the implausibility. The arithmetic was available immediately:
~10 000 cycles for an operation that should take tens.

### The fix, and why it is not obviously "reset it"

The mechanical fix is for `index::self_test` to restore the prior state, the way
`sockact.rs` was fixed today with `with_pristine_state`. But that would make the
benchmarks measure an indexer that is *off*, which is less like a real system, not more.
The honest options are to leave it live and **say so in the scorecard**, or to set it
deliberately rather than inheriting it from test ordering. Either is a decision about
what the write benchmarks are for, so it goes in the queue rather than getting picked
here — but the current state, where the answer depends on which self-test ran first, is
not one of the options.
