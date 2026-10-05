### [A] RESOLVED — the three unpairable benchmarks: the `seq` was never the problem — 2026-08-19

**In short:** the previous entry said three benchmarks could not be used as
load-window bounds because a kernel lookup failed for them. That diagnosis was
wrong. The lookup always succeeded; what was wrong was one word at three call
sites, where a benchmark told the harness it was called one thing and then
printed a different name on the line the harness's consumers actually read.
Fixed, verified by a boot: all three are now usable, and the suite's pairing
count went from 24 to 27.

#### What the earlier entry got wrong

It read the emission site —

```rust
if let Some(m) = measurements.get(entry.seq) {
    if m.name != entry.name {
```

— and concluded "for three entries `measurements.get(entry.seq)` does not
resolve". It resolves for every entry. The kernel says so on its own coverage
line, and said so at the time the entry was written:

```
Scorecard coverage: 86 of 98 measured windows reached the scorecard,
12 are declared diagnostics, 0 unjudged
```

`0 unjudged` is exactly the statement that no scorecard entry has a dangling
`seq`, and `SCORED_WITHOUT_MEASUREMENT` exists to count that case and read zero.
The `if let` was read as the failing step because it is the only fallible-looking
thing in the snippet; the *inner* condition was never checked. **The lesson is
narrow and worth keeping: a conditional emission has two ways to stay silent,
and the guard that looks fallible is not always the one that fired.**

#### What was actually wrong

`m.name != entry.name` was **false**. Three benchmarks build their `BenchResult`
by hand, and each passed the *scored* name to `note_measurement()` while printing
a *different* literal on its live result line:

| live line printed | passed to `note_measurement` | scored as |
|---|---|---|
| `context_switch_rt` | `"context_switch"` | `context_switch` |
| `ipc_channel_sync_rt` | `"ipc_channel_sync"` | `ipc_channel_sync` |
| `isr_hard_irq` | `"isr_latency"` | `isr_latency` |

So the two names compared equal, no `MEASURED-AS` was emitted, and
`canary-load.py` took the scorecard name for a live one and would have waited
through the entire suite for a result line the kernel never prints.

`run()` cannot make this mistake — it passes one `name` to both the print and
`note_measurement`. The other three hand-built results (`io_ring_nop_submit`,
`page_fault_anonymous`, and the fourth via `run`) already passed the live name.
Only these three had drifted, and nothing anywhere stated that they must not.

The earlier entry's hypothesised pairings, incidentally, were all three correct.
That is not vindication of the method: it declined to act on them precisely
because host-side name-guessing is what `MEASURED-AS` exists to replace, and the
kernel has now stated the same three pairings itself. Being right by suffix rule
and being right by evidence are not the same, and only the second one is
repeatable.

#### The fix

Each of the three sites now binds its live-line name once and uses that binding
twice:

```rust
const LIVE_NAME: &str = "context_switch_rt";
serial_println!("[bench] {}: min={} cycles …", LIVE_NAME, …);
…
seq: note_measurement(LIVE_NAME, SplitCheck::NotChecked),
```

The invariant this restores — **`Measurement::name` is the name printed on the
window's live result line** — was true of every benchmark that goes through
`run()` and was written down nowhere. It is now on both `Measurement::name` and
`note_measurement()`, along with the consequence of breaking it, because the
consequence is not a cosmetic mislabel: noting the *scored* name suppresses the
very line that tells a consumer which live line to watch for.

#### Enforcement, because nothing caught this for a day

Nothing in the kernel can catch the drift — it cannot know what literal a
`serial_println!` printed. So the check is host-side and runs every time:
`canary-load.py:report_unreachable_scored()` names any scored benchmark that
resolves to no live line, after every run, silently when there is nothing to say.
This is the same reasoning as `SCORED_WITHOUT_MEASUREMENT`: a gap recomputed from
each run's own output cannot go stale, and a silent miscount becomes a printed
anomaly. Before this, the only thing that noticed was a test that happened to
probe for orphans and reported a count.

`scripts/test-canary-load.py` covers the sweep's message, its silence, and its
refusal to accuse when it has no data — and asserts on the last `--bench` serial
log that no scored name is unreachable. That last check deliberately does **not**
read `build/canary-load-names.txt`: the name file is a cache only a canary-load
run refreshes, so a stale one would fail long after a fix and pass through a
regression no canary-load run has yet observed.

#### Verified

Boot of 2026-08-19, `RUN CLEAN`, 652s: 27 `MEASURED-AS` lines (was 24), the three
new pairings exactly as tabulated above, coverage still `0 unjudged`, and all
canary-load tests green. All 86 scorecard names are now usable as window bounds;
the "83 of 86" figure in the previous entry is superseded.
