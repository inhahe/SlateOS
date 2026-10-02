### [A] RESOLVED — `bench/boot-history.jsonl` recorded `profile: "debug"` for both an instrumented and an ordinary boot, so every duration statistic drawn from it averaged two populations that differ by 3.4x — 2026-08-19

**Symptom.** `python scripts/boot-history.py --list` over the 153 records
accumulated to 2026-08-19 shows rows like

```
2026-08-19T08:14:15+00:00  27bb7a96d  PASS               1096s
2026-08-19T09:04:58+00:00  27bb7a96d  PASS                337s
```

— same verdict, same commit, same `profile`, and a wall time that differs by a
factor of 3.3. Nothing in the record says why. The 1096 s rows are
KASAN-instrumented boots (`scripts/kasan-build.sh`); the 337 s rows are ordinary
ones. The file's own `profile` field could not tell them apart because it
records the *cargo* profile, and both builds are `debug`.

**Why it mattered.** Every consumer of the durations in that file was silently
averaging a mixture:

- the median printed by `--streaks` described no build that exists;
- "is this boot unusually slow?" — the question the file is kept for — had no
  answerable form, because slow-for-an-instrumented-boot and
  slow-for-an-ordinary-boot are three-fold apart;
- a timeout calibrated off the mixture is too tight for one population and too
  loose for the other. There is already one record with this shape: a `TIMEOUT`
  at 367 s wall with 3492 serial lines, which is a perfectly healthy
  instrumented boot cut off by an uninstrumented timeout, filed as a failure.
- `B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT` is titled "KASAN
  builds only" and its fingerprint could not honour its own scope, because the
  record it matches against did not carry the build.

**The fix.** The kernel now announces its own build, immediately after
`=== Kernel booting ===` (`kernel/src/main.rs`):

```
[boot] build profile: sanitizer=kasan-instrumented
[boot] build profile: sanitizer=none
```

`scripts/boot-history.py` parses it into `Serial.sanitizer`, writes it into
every record, groups wall times by it (`wall_populations` / `report_wall`,
which deliberately prints *no* combined figure), shows it as a column in
`--list`, and scopes the KASAN fingerprint by it.

**The part that is the actual lesson, and is not the parsing.** The banner is
printed **unconditionally, with a varying value** — not printed only by the
instrumented build. The tempting version is one line inside
`#[cfg(kasan_instrumented)]`, and it is wrong for a reason this project has now
been bitten by three times (`B-KASAN-FALSE-CLEAN-STREAK`, the liveness watchdog
counting its own breadcrumbs, the preshadow check's unexpandable roots): *a
signal whose absence is meaningful is a signal that cannot distinguish "no" from
"nobody asked".* Every one of the 153 historic records predates the banner, and
a good number of them are instrumented. Had absence meant "uninstrumented", the
fix would have relabelled all 153 of them — in exactly the direction that makes
the two populations look like one, which is the bug.

So `sanitizer` is three-valued and stays that way through the whole pipeline:
`"kasan-instrumented"` / `"none"` / `None` ("this log cannot say"). The three
states are kept distinct in the JSONL too — the key is written even when the
value is null, so an *absent* key means "row predates the field" while a *null*
one means "the kernel did not say". Nothing anywhere is permitted to fold
`None` into `"none"`; `_fp_kasan_midprint` declines only on an explicit
`"none"`, so its single validated occurrence (2026-08-12, from a pre-banner
kernel) still matches. `scripts/test-boot-history.py` asserts that directly —
it is the one test whose failure would mean the fingerprint's streak had
silently reset to a perfect clean one.

**Not fixed here:** the 153 existing records stay `null`. They are not
recoverable — the evidence needed to reclassify them is precisely the line that
did not exist — and guessing from wall time would manufacture the certainty the
whole three-valued design exists to refuse.
