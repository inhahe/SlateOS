## TD-B-HTOPS-CPU-BARS-SHOW-TIME-SINCE-BOOT-NOT-RECENT-ACTIVITY (lane B, 2026-09-10) -- FIXED the same day

**In short:** the per-CPU bars along the top of `htop` show how the machine has
spent its time *since it booted*, not how it is spending it now. After a few
hours of uptime they barely move, whatever the machine is doing.

**Where.** `userspace/htop/src/main.rs`, the CPU-bar block in the renderer:

```rust
let total = stat.total().max(1) as f64;
let user_frac = stat.user as f64 / total;
```

`stat` is the *cumulative* counter from `/proc/stat`. Real `htop` divides the
**delta** between two samples, which is why its bars move.

**The program already has what it needs.** `App` keeps `prev_cpu_stats` and
uses it for the per-process percentages; the bars are the one consumer that
reads the current sample alone. The fix is to subtract field by field and
divide by the delta of [`procinfo::CpuTimes::total`].

**Found while** moving the reader into `procinfo`, not by anyone watching the
bars -- which is the point worth recording. A display that is *always* wrong in
the same direction looks like a design choice rather than a defect, and nobody
reports it.

**Deliberately not fixed in the same commit** as the extraction: that commit's
claim is "behaviour is unchanged except where it was wrong to read", and
changing what the bars *mean* is a different claim that deserves its own diff.

**Fixed** in that separate diff. `App` keeps a `cpu_delta` -- the per-CPU
difference between the last two samples, from `procinfo::CpuTimes::since` --
and the renderer divides that instead of the raw counters.

Two things fell out of doing it:

* **A zero-length interval now has no reading, rather than 0.0%.** The old code
  divided by `total().max(1)`, so the first refresh -- before any interval
  exists -- printed a confident "0.0%" for every CPU. It draws nothing now.
  The same case arises for one frame when a CPU is taken offline and brought
  back, which is why `since` saturates instead of asserting time runs forwards.
* **The arithmetic is a pure function with tests.** It was four lines inside a
  render loop, which is why nobody could have noticed it was dividing the wrong
  pair of numbers.
