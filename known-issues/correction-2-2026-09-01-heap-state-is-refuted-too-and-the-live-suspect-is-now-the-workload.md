## CORRECTION 2, 2026-09-01: heap state is refuted too, and the live suspect is now the *workload*

**In short:** correction 1 (below) cleared the inode table and blamed the
kernel heap instead. The controlled experiment it proposed has now run, and the
heap is innocent as well — it accounts for **0.4 %** of the number, where the
thing being explained moved by **200 %**. Two mechanisms proposed, two
mechanisms measured, two mechanisms dead. What the same run *did* establish is
a clean cost model for a listing, and that model points somewhere neither
hypothesis looked: **the benchmark's own workload is not a constant.**

### The 2×2, resolved

The rebuilt benchmark warms the allocator (create 2048 files, delete them
again) *before* the small-table arms, so the ballast arm afterwards differs in
table size **only**. Both variables move one at a time:

| comparison | what it isolates | result |
|---|---|---|
| `n64_cold` 39324 ns → `n64` 39473 ns | heap warmth (same dir, same table) | **+149 ns = +0.4 %** |
| `n64` 39473 ns → `n64_ballasted` 39929 ns | +2048 inodes (both post-warm-up) | **+456 ns = +1.2 %** |

Correction 1's headline — "the ballast made listings 25–46 % *faster*" — was
itself an artifact: it was measuring the heap warm-up that the first version of
the benchmark had not controlled for. With the warm-up in place the effect
vanishes in both directions. Neither number is a mechanism; a hypothesis that
predicts a *dominating* per-child cost is not supported by 1.2 %.

### What the run did establish: a listing costs ~10 µs + ~0.46 µs/entry

`n0` = 10013 ns for an empty directory; the 8- and 64-entry windows and their
ballasted twins all sit on a line of about 460 ns per entry. This is stable
across four of the five windows and is the first solid fact in this entry.

### Where that points

**The scored `vfs_readdir` lists `/`, and nothing pins how many entries `/`
has.** It is whatever the boot left there — mounts, service directories,
anything an earlier benchmark created. So a commit that never touches a
filesystem can change the number, and at 460 ns/entry a root that gained ~70
entries covers the whole 16.3 µs → 84.3 µs span the history shows, with no
performance change of any kind.

That is also the only hypothesis so far that explains the shape of the data
that has defeated the other two: the series is **binary-determined** (A/A pairs
land 0.5 % apart, and `scripts/bench-history.py` independently reports that the
fence "separates BINARIES, not runs") yet indifferent to every mechanism inside
memfs and the allocator. A per-binary difference in *what is being listed* is
exactly that.

`bench_vfs_readdir` now logs the entry count and the names of `/` before it
times anything. The next `--bench` boot either shows a count that tracks the
number — in which case this was never a performance regression and the fix is
to list a directory the benchmark builds itself — or shows a count identical
across disagreeing binaries, which kills this hypothesis too and leaves the
mount walk or `finish_listing` as the place to look.

### A benchmark-harness bug found on the way

`bench_vfs_readdir_breakdown`'s verdict was `d64 > 0 && d64 > d8 * 3`. A
**negative** `d8` makes that second test vacuously true, so any positive `d64`,
however tiny, passes it. It duly printed

> `CONFIRMED per-child inode-table lookup dominates — the penalty scales with
> entry count (456x from 8 to 64 entries)`

from `d8 = −10116 ns` and `d64 = +456 ns` — a "ratio" computed as
`d64 / d8.max(1)`, i.e. `456 / 1`. **The benchmark asserted the opposite of
what it measured, in the log line intended to save the next reader from
re-deriving it.** Fixed: materiality is now tested first (a delta under 5 % of
its own window is reported as refuting, whichever way it points), the ratio
test is guarded on `d8 > 0`, and every delta is printed as a percentage
alongside its nanoseconds. This is the second arithmetic bug in three versions
of this benchmark — after `saturating_sub` — and both had the same effect:
letting it print a confident verdict its own numbers did not support.
