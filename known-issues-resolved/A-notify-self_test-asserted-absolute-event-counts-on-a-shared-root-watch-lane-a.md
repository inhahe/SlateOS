## `notify::self_test` asserted absolute event counts on a shared root watch (lane A)

**Status:** FIXED 2026-08-22 (lane A). Found by un-gating the suite — see
*"Seven of the eight boot self-tests behind `if fat_ok`"* above.

**In short:** A "watch" is a subscription that collects notifications about
files changing. The change-notification self-test opened one watch on the whole
root directory, then ran ten separate checks against it, each one asserting an
exact number of notifications. That only works if nothing else ever puts a
notification into that queue. Two things do. One check deliberately fires a
notification that a *later* check then miscounted, which broke the test the
first time it was ever run in CI. And now that the suite runs on a live
filesystem, any other part of the kernel writing a file at the root would land
in the same queue and break it at random.

### Evidence

The first CI boot that ran this suite (it had been skipped on every previous
automated run by the `fat_ok` gate):

```
[notify]   Event mask filtering verified ✓
[notify]   FAIL: coalescing expected 1 event, got 2
WARNING: Change notification self-test failed: InternalError
```

### Root cause — two independent defects

**1. The suite leaked events into itself (deterministic).** The mask-filtering
section creates a `CREATE`-only watch and emits a `MetadataChanged` to prove
that watch ignores it. It does — but the event *also* matches `watch_id`, the
shared `/` + `ALL_CHANGES` watch, and only the `CREATE`-only watch was drained.
The coalescing section three lines later emits three identical `Modified`
events, expects them to coalesce to exactly 1, and read 2: its own event plus
the stray `MetadataChanged`. Nothing about this depended on the environment; it
would have failed on a FAT root too. It had simply never run.

**2. Absolute counts over a watch on the live root (latent, would flake).**
Every assertion in the suite was of the form `events.len() != N`, taken over a
watch that matches *everything* happening at `/`. That is safe only while the
suite is quarantined from a real filesystem — which is exactly what the gate was
doing. Un-gating it puts the suite on the same memfs root the rest of the kernel
is using, so a concurrent write at `/` from any other task becomes a spurious
failure. Fixing only defect 1 would have left a test that passes today and flakes
later, which is worse than one that fails immediately.

### Fix

- All synthetic probe paths moved into a private namespace, `/NOTIFY_ST_*`, and
  a `read_probe_events()` helper filters every count-asserting read to that
  prefix. Foreign traffic at `/` can no longer be counted. This is the same
  design `interest_gate_self_test()` already used (delta-correct against a
  captured baseline) and for the same reason.
- The mask-filtering section now drains the shared watch immediately after the
  emit that pollutes it, at the section that created the leftover rather than
  leaving it for whichever section next asserts a total.
- The overflow check deliberately keeps reading *unfiltered*: the overflow
  marker carries an empty path, so the namespace filter would drop the very
  event under test. It is safe unfiltered because it asserts only that the
  marker comes first — `read_events` always prepends it — and takes no total.

### Why the namespace filter alone was not enough

The obvious fix is the filter, and it is wrong on its own: the stray
`MetadataChanged` is emitted on a path the suite owns, so it matches
`/NOTIFY_ST_` and is still counted. The two defects need the two separate fixes.
This is worth remembering — a filter that catches foreign contamination does
nothing about a suite contaminating itself.
