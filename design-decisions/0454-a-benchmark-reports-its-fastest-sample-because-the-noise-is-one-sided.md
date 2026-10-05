## §454 — A benchmark reports its *fastest* sample, because the noise is one-sided

**Date:** 2026-08-17. **Decided by:** Claude (autonomous).
**Lane:** C

**In short:** When you time the same piece of code many times, you get many
different answers, and you have to pick one number to report. The obvious pick
is the middle one (the median) — that's what this project's shaping benchmarks
did, and it turned out to be wrong. Timings here vary because the operating
system occasionally freezes a program for a millisecond or two; that can only
ever make a run *slower*, never faster. So the middle answer is "what the code
costs, plus however busy the machine typically was" — and comparing two
versions of the code by that number partly compares how busy the machine was
each time. We now report the *fastest* run instead, which is the best available
estimate of what the code costs when nothing is interfering with it.

### The evidence

Two runs of an identical benchmark — the cost of shaping a 1,000-character line
of text, each the only test running — reported **776 us and 1,313 us**. Nothing
had changed between them. At that point the shaping entry in `known-issues.md`
was drawing conclusions from differences of 1.2x-1.4x, which a 1.7x measurement
error cannot support.

`text::shaping_cost::is_this_instrument_stable` repeats one fixed workload
twelve times inside a single process and prints the median and the minimum of
each block. Across the twelve:

| statistic | spread across 12 identical blocks |
|---|---:|
| median | **1.89x** |
| minimum | **1.02x** |

Re-measuring the historical benchmarks by minimum against their published
medians gave a correction factor of 2.18x / 2.13x / 2.14x / 1.89x / 1.74x at
80 / 200 / 1,000 / 5,000 / 20,000 characters. The median was not adding *noise*
so much as adding a fairly consistent tax — but a tax that shrinks as the
workload grows, which is exactly what makes it dangerous: it distorts the
*shape* of a curve, not just its height. Two of the conclusions published from
those tables were artefacts of precisely that (see `known-issues.md` →
`C-FONT-SHAPING-IS-1400X-SLOWER-THAN-IT-SHOULD-BE`).

### The alternatives, and why not

- **Median.** The default, and the usual advice, because in most statistical
  settings noise is two-sided and the median resists outliers on both sides.
  Timing is not that setting. Here the "outliers" are all on one side and are
  not outliers at all — they are the common case, one in every few samples on
  this host. The median therefore estimates *cost + typical interference*, and
  ranking two code versions by it partly ranks them by how busy the box was.
- **Mean.** Worse than the median on the same grounds, plus it lets a single
  100 ms stall decide the answer.
- **A trimmed mean or a low percentile (p5, p10).** Defensible, and close to
  what the minimum does. Rejected as strictly more knobs for the same answer:
  if the noise is one-sided, the correct trim is "all of it", which is the
  minimum. A percentile would need justifying at every workload size, since the
  fraction of stalled samples depends on how long a sample takes.
- **Criterion.** The project already uses it for `bench/`, and it would do the
  statistics properly. Not used here because these instruments live inside
  `guitk`'s unit-test binary specifically so that one of them
  (`the_layout_tables_do_not_dominate_shaping`, §453) can be a *non-ignored
  assertion* in the normal suite, and so that all of them share the toolkit's
  private font cache. A criterion bench cannot reach either.

### The objection, and the answer

**"Reporting the minimum is cherry-picking."** It would be, if the goal were to
predict what a user experiences — a user does pay the stalls. It is not: these
instruments exist to compare implementations of the same function, and for that
the question is "what does this code cost", not "what does this machine do to
it". A stall costs the same milliseconds whichever version of the shaper is
running, so including it adds a common term to both sides and variance to
neither's advantage. Anyone who wants the user-facing figure should measure the
frame, not the function.

### Two things the minimum needs before it works

Both were found by the minimum *failing* first, which is why they are recorded
here rather than left as implementation detail.

1. **A floor on total sampling time, not just a sample count.** This host stalls
   every process for a millisecond or two at a time, several times a second. 51
   shapings of the built-in bitmap face is 0.7 ms of work — it fits *inside* one
   stall, so every sample is stalled and so is the minimum. That is why the
   built-in control still spread 1.84x when the system-face column had settled
   to 1.02x. `timed()` now samples until both the requested count and 20 ms of
   wall time are reached. The general rule: **the sampling window must be longer
   than the interference it is trying to step around**, and a sample count does
   not guarantee that.
2. **Batching for anything near the clock's resolution.** The minimum of 200,000
   timings of a single font-cache hit printed a flat `0.000us` — the clock
   reporting that it cannot see this, not the lookup being free. The minimum has
   a floor at the timer's granularity and will happily report it as a result.
   Timed in batches of a thousand and divided back down it reads 0.045 us.

### Also settled by the same episode

**Never measure in the same invocation as a compile.** A `cargo test` that
rebuilds first inflates the measurement that follows it by ~10%, which was on
its own enough to manufacture a 1.14x "effect" in a published table. Build with
`--no-run` first, then measure in separate invocations.

### Where it lives

`gui/toolkit/src/text.rs` → `mod shaping_cost`: `best()` (the statistic and its
justification), `timed()` (the wall-time floor), `font_lookup_us()` (the
batching), and `is_this_instrument_stable()` (the measurement that decided it,
kept in the tree so the choice can be re-checked on other hardware rather than
believed on this one).
