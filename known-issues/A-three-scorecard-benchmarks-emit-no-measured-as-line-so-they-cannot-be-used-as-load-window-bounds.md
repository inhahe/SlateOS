### [A] Three scorecard benchmarks emit no `MEASURED-AS` line, so they cannot be used as load-window bounds — 2026-08-19

**Status:** OPEN

**In short:** the load-window tool needs a benchmark name to mark where a
disturbance starts or stops. Three of the suite's 86 scored benchmarks —
including `context_switch` and `ipc_channel_sync`, two of the headline
performance numbers — cannot be named at all. Asking for a window at
`context_switch` is refused in about a second with a list of near-miss
suggestions, none of which is the right answer, because the right answer does
not exist. Nothing is mismeasured and no run is silently wrong; a region of the
suite is simply unavailable to the experiment that would probe it.

#### The two namespaces, and the line that bridges them

A benchmark prints a live result line while it runs, and a `SCORE` line at the
end of the suite. For 27 of 86 the two names differ. `canary-load.py` triggers
on the *live* name and `grade-positional.py` places its window with the *scored*
name, so a bridge is required, and `bench.rs:1297` supplies it — the kernel
states each pairing itself as `[bench] MEASURED-AS <scored> <live>`. That is the
right design: the pairing cannot be recovered host-side, because the two orders
genuinely interleave, nor from the source, because several benchmarks build
their `BenchResult` by hand.

#### The gap

The emission is conditional on a lookup succeeding:

```rust
if let Some(m) = measurements.get(entry.seq) {
    if m.name != entry.name {
        serial_println!("[bench] MEASURED-AS {} {}", entry.name, m.name);
```

For three entries `measurements.get(entry.seq)` does not resolve, so no line is
emitted and the pairing is simply absent. Derived from the current
`build/canary-load-names.txt` (98 live names, 86 scored, 24 pairings):

| scored name | almost certainly measured as | status |
|---|---|---|
| `context_switch` | `context_switch_rt` | live, unscored, unpaired |
| `ipc_channel_sync` | `ipc_channel_sync_rt` | live, unscored, unpaired |
| `isr_latency` | `isr_hard_irq` | live, unscored, unpaired |

The right-hand names are three of the 15 live names that are neither scored nor
an alias target, and the `_rt` suffix pattern makes two of the three pairings
hard to read any other way. **They are a hypothesis, not a finding** — the whole
point of `MEASURED-AS` is that host-side name-matching is not trustworthy, so
guessing the pairing here would repeat the mistake the line exists to prevent.
The fix must come from the kernel: find why `entry.seq` does not resolve for
these three, not paper over it with a suffix rule.

#### Effect, and why it is bounded

83 of 86 scorecard names work as window bounds. The three that do not are
**refused before the build**, by name, in about a second — the failure mode is a
window that cannot be requested, never a window that lands somewhere other than
its label. That distinction is the whole reason the guard was written, and it is
holding here: this is the guard reporting a real gap, not a hole in it.

#### Not repaired here, deliberately

`bench.rs` is the harness PREDICTION P23 is registered against. Changing it in
the same breath as running the experiment it hosts would mean the run no longer
tests the code the prediction was written for — the same objection already
recorded against retuning `trace_reference` before the measurement. P23 does not
need any of the three names, so the gap does not block it.

**Trigger to fix:** after P23 is run and graded. The investigation starts at
`ScoreEntry::seq` in `kernel/src/bench.rs` and the question is why three entries
carry a `seq` with no corresponding `MEASUREMENTS` slot — the scorecard entries
built by hand are the obvious suspects. Regression coverage already exists:
`scripts/test-canary-load.py` asserts that a scored-but-not-live name is refused
*by name*, so the count of such names shrinking to zero is observable.
