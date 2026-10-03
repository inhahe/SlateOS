## Every boot-time median in `bench/boot-history.jsonl` pooled debug and release boots (lane A)

**Status: FIXED 2026-08-21** — `scripts/boot-history.py`, `population_of()`.
Awaiting a boot test on `main` before archiving.

**In short:** The boot test prints a table of "how long does a boot take",
grouped so that builds which run at different speeds are never averaged
together. The grouping had two of the three factors that change the speed, and
was missing the third — which build profile was compiled. Release boots ~2.7×
faster than debug, and 100 of the 243 recorded boots were release, so the
printed medians were blends of two populations rather than measurements of
either. The fix adds the missing factor. **If you quoted a boot-time median
from this file before 2026-08-21, re-check it.**

**Where it lived.** `population_of()` returned
`f"{sanitizer_of(rec)} on {accel_of(rec)}"` — sanitizer × accelerator. The
`profile` field was recorded in every row and used by nothing.

**Why it went unnoticed for the life of the file.** `report_wall()`'s docstring
is an unusually careful argument *for* partitioning, written after a real
incident in which two WHPX boots shifted the TCG median. It states the
principle exactly right — "a wall time is a property of the pair… neither
factor makes the other irrelevant" — and then enumerates two factors. Naming a
partition is not the same as checking it covers every factor that moves the
number, and the axis it omitted is the largest one after KASAN.

The pooled figures were also *plausible*, which is what let them survive. The
largest population read **155 boots, median 331 s**. That is a 95/60 mixture of
a 382 s population and a 130 s one — a duration no build on this host has ever
taken, but not an obviously silly one.

**Measured impact, from the file itself on 2026-08-21:**

| Population | Was printed | Actually is |
|---|---|---|
| `none on QEMU TCG` | 53 boots, median **370 s** | debug: 38 boots, **395 s** · release: 15 boots, **144 s** |
| `none on unknown accel` | 12 boots, median **121 s** | debug: 3 boots, **327 s** · release: 9 boots, **119 s** |
| `unknown on unknown accel` | 155 boots, median **331 s** | debug: 95 boots, **382 s** · release: 60 boots, **130 s** |

The second row is the worst: the pooled median is within 2% of the *release*
figure, so anyone asking "what does a boot of this tree cost" got the release
answer under a label that never said release, off by 2.7× for a debug run.

**Consequences beyond the table.** `open-questions.md` Q46 turns on exactly
this comparison — it asks whether the non-bench boot test should build release,
and prices it as "slower build, faster boot". Its supporting numbers (142 s
release vs 615 s debug) came from a hand-measured bench pair rather than from
this file, which is fortunate, because the file could not have answered it.

**The fix.** `population_of()` is now the *triple*
`profile/sanitizer on accelerator`, with `profile_of()` as the third twin of
`sanitizer_of`/`accel_of` — collapsing key-absent and key-null to
`unknown profile`, and never guessing `debug` merely because that is the
recorder's argparse default. Six tests cover it, including one built from the
real 95/60 shape above. Two stale quotes of the 370 s figure in
`bare-metal-boot.md` were corrected to 395 s in the same change.

**Related gap closed at the same time.** Build time was never recorded at all,
so Q46's "slower build" half had no evidence anywhere. Step 1 of
`scripts/boot-test.sh` is now timed into a `build_seconds` field, reported as
`build time by profile`, and absent (not zero) for `--no-build` runs.
