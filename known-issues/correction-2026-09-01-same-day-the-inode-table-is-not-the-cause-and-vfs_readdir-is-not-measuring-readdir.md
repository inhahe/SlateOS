## CORRECTION, 2026-09-01 (same day): the inode table is not the cause, and `vfs_readdir` is not measuring readdir

**In short:** I blamed a refactor of mine for tripling the cost of listing a
directory, and built a benchmark to prove it. The benchmark disproved it. What
it found instead is worse and more useful: **the `vfs_readdir` number is
dominated by the state of the kernel heap, not by the filesystem**, so it
cannot presently detect a readdir regression at all — and the "3× regression"
below was never established.

**The experiment.** `bench_vfs_readdir_breakdown` lists directories of 0, 8 and
64 entries, then creates 2048 files *in a different directory* — growing the
memfs inode table ~10× while leaving the listed directories byte-identical —
and lists them again. If per-child lookups into a `BTreeMap` of large values
were the cost, a 10× bigger table would make the listings slower, and slower in
proportion to the number of children.

**They got faster.**

| window | before ballast | after ballast |
|---|---|---|
| 8 entries | 22497 ns | **16855 ns** (−25 %) |
| 64 entries | 80891 ns | **43796 ns** (−46 %) |

A per-child search of a larger tree cannot become cheaper. The hypothesis is
dead, and boxing or slab-allocating `MemFsNode` would have been effort spent
against a cause that does not exist. (The first version of the benchmark
printed `NOT CONFIRMED` for the right reason by accident: it computed the
deltas with `saturating_sub`, which rendered both of these negative results as
`+0 ns`. Fixed — the deltas are signed now, and there is a third verdict,
`REFUTED`, for exactly this outcome.)

**What is really going on: the ballast warms the allocator.** `readdir`
allocates a `Vec<DirEntry>` plus one owned name per entry on *every* iteration,
and 2048 file creations leave the heap grown and holding a pool of warm,
right-sized free blocks.

**The evidence is the dispersion, not the mean** — and getting this right
required a second pass, because the first draft of this correction cited the
mean/min ratios the harness prints and those turn out to be the wrong statistic.
Reconstructing each window's distribution from `min`/`mean`/`max`/`iterations`:

| window | max/min | max as % of total | rest, as multiple of min |
|---|---|---|---|
| `heap_raw_alloc_free_512` | 179708 | **98.1 %** | 1.7× |
| `vfs_readdir_n0` | 3775 | **92.5 %** | 1.5× |
| `vfs_readdir_n8` | 3273 | **92.4 %** | 1.4× |
| `vfs_readdir_n64` | 1089 | 49.0 % | 5.7× |
| `vfs_readdir_root` | 878 | 31.0 % | 9.8× |
| `vfs_readdir_n8_ballasted` | **16** | 3.9 % | 2.0× |
| `vfs_readdir_n64_ballasted` | **50** | 10.4 % | 2.2× |

So `heap_raw_alloc_free_512`'s alarming 90× mean/min is **one single stall**
that accounts for 98 % of the window; with it removed the allocator is steady at
1.7× its min. The kernel heap does *not* have a heavy tail, and an earlier draft
of this entry said it did. Same for `n0` and `n8` — one outlier each, otherwise
clean. Those isolated multi-millisecond stalls are consistent with the host
descheduling the vCPU thread, which inflates an `rdtsc` delta without any guest
work happening, and they are why `min` is the statistic the harness reports.

**The two windows with a genuinely repeated tail are the two that list many
entries**: `vfs_readdir_n64` (5.7× min after the max is removed) and
`vfs_readdir_root` (9.8×). And the ballast collapses it — `n64` goes from
max/min 1089 and a 5.7× body to max/min **50** and a 2.2× body. The ballast did
not merely shift the level, it removed the repeated stalls.

That is the shape of **heap growth**: listing 64 entries allocates ~65 times,
which repeatedly pushes the heap past its high-water mark and forces it to map
new pages; once the ballast has grown the heap and freed everything back, later
listings allocate into space that already exists and never pay it. The
hypothesis is testable and the 2×2's `n64_cold` vs `n64` arms test it directly.

**The corroborating evidence was in the scored number all along**, and I read
past it: three binaries have now produced **16675 ns, 50216/49970 ns, and
84331 ns**. The third differs from the second only by code appended *after*
`vfs_readdir` runs, which cannot affect it causally — but perturbs heap layout,
which can.

**Why the A/A replication did not catch this, which is the lesson.** Two boots
of a byte-identical image landed 0.5 % apart, and I took that as proof the
number was a property of the binary rather than the host. It is — but "property
of the binary" and "property of the code under test" are not the same claim. A
deterministic boot reaches an identical heap state at an identical point every
time, so an A/A pair reproduces heap-derived cost *perfectly*. **A/A
replication distinguishes binary-determined from host-determined; it says
nothing about whether the binary-determined part is the thing you named.**

### What is actually wrong, and what the fix is

1. **`vfs_readdir` is scored (target 50 µs) on a number it does not control.**
   This survives correction 2 and is the one durable conclusion of the whole
   entry — only the *reason* changed. It is not heap state (0.4 %); it is that
   the benchmark lists `/`, whose entry count nothing pins. A scored benchmark
   on the performance-critical path whose workload can change without any code
   change will keep flagging false regressions and will hide real ones.
   ~~The fix is to warm the heap before the measured window~~ — superseded:
   the fix is to list a directory the benchmark **builds itself**, so the
   workload is fixed by construction. See correction 2.
2. ~~**The heap's tail is the thing worth chasing.**~~ **Withdrawn — this item
   was wrong when written, and the table two paragraphs above it already said
   so.** There is no 90× tail: `heap_raw_alloc_free_512`'s max is 98.1 % of the
   window's *total* time, i.e. one stall, and with it removed the allocator is
   steady at 1.7× its min. Those isolated multi-millisecond stalls are the host
   descheduling the vCPU thread. Correction 2's controlled arms confirm it from
   the other side: heap state moves a listing by 0.4 %.
3. **The memfs refactor is unindicted.** Nothing here says it is free — only
   that no measurement attributes anything to it, now across two controlled
   experiments. Any future attempt must fix the workload first or it will
   reproduce this mistake.
