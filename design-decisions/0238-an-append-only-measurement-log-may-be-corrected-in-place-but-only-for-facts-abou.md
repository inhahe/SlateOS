## §238 — An append-only measurement log may be corrected in place, but only for facts about the *harness*, never about the tree

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** We keep a log of every kernel boot — did it work, how long did it
take — and other parts of the project read it to answer questions like "has this
bug stopped happening?" Normally we only ever add lines to that log; we never go
back and change one, because a log you can edit is a log you can quietly make
say whatever you want. But three of its lines were wrong in a specific way: they
recorded deliberate experiments (runs where the emulator was deliberately
misconfigured to see what would happen) as if they were ordinary boots of our
code, and one of those "boots" never even loaded our kernel. Left alone, they
made a bug look like it had stopped recurring when nothing had been tested. The
decision is that this narrow kind of correction is allowed, and the three
conditions that make it allowed are written down so the exception cannot widen.

### What happened

`bench/history.jsonl` had carried an `experiment` field since the layout sweeps;
`bench/boot-history.jsonl` had none. Three WHPX probe runs on 2026-08-19 were
therefore recorded as ordinary boots. The `-cpu host` probe, which died inside
OVMF's `PlatformPei` before our kernel was loaded (see
`ENV-WHPX-CPU-HOST-FIRMWARE-GP`), landed as a plain `TIMEOUT`. Two consequences:

- `current consecutive clean streak` read **0**. Four open kernel issues state
  their closure condition as a count of consecutive clean boots, so all four had
  silently had their bar reset by a fact about which CPU models WHPX accepts.
- Worse, `streaks()` counted it toward every failure fingerprint's `since_last`.
  That function's entire claim to being usable for closing an issue is the
  argument *"a boot that failed differently is still a boot in which this did not
  appear"* — which assumes the kernel **ran**. Here it had not. That is a
  manufactured clean streak, the exact failure the module's docstring exists to
  prevent, arriving through the one door nobody had put a guard on.

Adding the field fixes the future. It does not fix those three rows, and the
damage is entirely in those three rows.

### The decision

**In-place correction of an append-only log is permitted when all three of these
hold.** If any one fails, the row stands and the discrepancy is documented
instead.

1. **The row misstates a fact about the harness, not about the tree.** Whether a
   run was a deliberate probe is a property of how it was launched. It is not a
   measurement, it was never observed-and-recorded, and re-deciding it cannot
   change any number the run produced. Contrast: a wall time that looks wrong, a
   verdict that seems mistaken, a serial log that disagrees with memory — those
   are the tree talking, and they are never editable, however implausible.
2. **The correction is verifiable from records outside the file.** Otherwise the
   file is being edited to agree with somebody's recollection, which is what the
   append-only rule exists to forbid. Each row here was matched on its exact
   timestamp and checked against its `(profile, verdict, marker)` before being
   touched, and each was corroborated externally — a byte-identical experiment
   string on the same timestamp in `bench/history.jsonl`, the row's own recorded
   OVMF `tail`, and the session transcript's record of the invocation.
3. **Leaving it wrong actively corrupts an inference something else depends on.**
   A cosmetic inaccuracy is not worth the precedent. A poisoned streak that four
   issues read as a closure condition is.

And two procedural requirements, which cost nothing and are what make the edit
auditable afterwards:

- **Reconstruct what the harness would have written, do not paraphrase.** The
  reason strings were assembled the way `record_boot_outcome()` assembles them
  today — `BENCH_EXPERIMENT`, then `QEMU_EXTRA`, then an overridden `QEMU_CPU`,
  joined `"; "`. A hand-written summary would read as an annotation by a later
  hand, and would not survive being diffed against a real row.
- **The edit asserts its way in.** The backfill script refuses to write if a
  timestamp is missing, if the fingerprint disagrees, or if the row already
  carries the field. A backfill that silently no-ops is worse than none, because
  it reports success.

### Why not the alternatives

**Leave the rows and note the discrepancy in `known-issues.md`.** This was the
conservative option and is what the append-only rule says by default. Rejected
because the note does not reach the consumer. `streaks()` is read by a future
session asking "can I close this?", and it would answer "not seen in 166 boots"
in a machine-checkable-looking format; the human reading that number has no
prompt to go find a paragraph in a 40,000-line file explaining that three of
them do not count. A correction that only works if someone remembers to apply it
is not a correction — the same reasoning that made `accel` part of the grouping
*key* in §237 rather than a check performed afterwards.

**Delete the three rows.** Strictly worse than labelling them. The `-cpu host`
row is the sole machine-readable record that the probe happened and how it
failed, and `--list` should keep showing it: probes are excluded from
*inference*, not hidden. Deletion also destroys the audit trail that makes this
edit reviewable.

**Append three compensating "correction" rows.** Preserves the literal
append-only property while abandoning what it is for. It leaves the file with
rows that are not boots, which every consumer must now learn to skip — the same
problem, moved, plus a new record type.

### The cost, stated plainly

This weakens a property that was previously absolute, and "we may edit it when
the edit is clearly justified" is exactly the shape of rule that erodes. The
mitigation is that condition 1 is a bright line and not a judgement call:
*was this fact ever measured?* An experiment label was not — it was known before
the run started and simply not written down. Anything the boot itself produced is
off limits, permanently, no matter how wrong it looks. If a future session finds
itself arguing that a *measurement* meets these conditions, the answer is no, and
the argument itself is the evidence that the exception is doing what exceptions
do.
