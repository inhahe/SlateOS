### FIXED-A-SYSTEMSOUNDS-A-SUITE-ASSERTED-A-PLAY-COUNT-IT-COULD-NOT-REACH

**Date:** 2026-08-23 · **Lane:** A · **Where:** `kernel/src/fs/systemsounds.rs`
(test 8 of `self_test_inner`, and `play()` / `State::total_plays`)

Boot batch 24 halted on:

```
panicked at kernel\src\fs\systemsounds.rs:424:5:
assertion failed: plays >= 3
```

Tests 1–7 had all reported OK.

**Diagnosis.** `total_plays` counts *audible* plays, not attempts. It is
incremented beside the per-assignment `play_count`, inside the
`assignment.enabled` branch and downstream of the `global_enabled` gate — so a
muted event is not a play. That is the right semantic: a stats line that
counted suppressed events would be reporting sounds the user never heard, and
the per-assignment counter it sits next to plainly means "times this sound
played".

The suite calls `play()` six times, but four of them exist precisely to check
that nothing is emitted:

| Test | Call | Outcome |
|---|---|---|
| 2 | `Notification`, enabled | audible (+1) |
| 3 | `Notification`, event disabled | silent |
| 4 | `Error`, custom sound | audible (+1) |
| 5 | `Startup`, `"Silent"` scheme (all assignments disabled) | silent |
| 6 | `Startup`, global sound off | silent |

The true count is 2. `>= 3` could never hold.

**Fix.** The counter is correct; the assertion was fiction. It now reads
`assert_eq!(plays, 2, "two of the six play() calls are audible")` — an
equality rather than a bound, because the value is deterministic and an
equality also catches the counter *over*-counting, which no `>=` would. The
enumeration above is recorded as a comment at the assertion so the next reader
does not have to re-derive it.

**Why it hid, and what that says about the batch.** Nothing: the suite was a
`systemsounds test` shell subcommand and had never executed. This is the
fourth bug in this batch surfaced by wiring an *existing* suite into the boot
path rather than by writing a new test — after the procfs self-deadlock, the
twenty-one byte formatters, and sysmaint's never-due tasks.

It is also the mildest of the four, and worth recording for exactly that
reason: an assertion nobody ever ran is not a weaker test than no test, it is
a *false* one. Until batch 24 the module reported "all 8 tests passed" to
anyone who typed the subcommand — because the subcommand ran against live
state where an earlier real play could push the count past 3. Against pristine
state it never could. De-fanging the suite is what made it honest, and then
immediately made it fail.
