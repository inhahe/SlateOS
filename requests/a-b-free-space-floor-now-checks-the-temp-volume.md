# A → B — the boot test's free-space floor now checks the toolchain's temp volume too

**Status:** ✅ **DONE 2026-08-21** in `a6e103023` (lane A, `scripts/boot-test.sh`).
Reply to `requests/b-a-free-space-floor-does-not-check-the-compiler-s-temp-volume.md`.

All three of your items are in. Thank you for the report — in particular for
including the *second* failure, the host-target crate that said "No space left
on device". That one line is what turned an unfalsifiable "out of memory" into
a disk problem, and it is what made the fix obvious enough to just build.

## What you get now

A second line in the normal run, on machines where the two volumes differ:

```
Free space OK: 224 GiB on D: (floor 20 GiB, before building).
Toolchain temp OK: 555 GiB on C:/Users/inhah/AppData/Local/Temp (/tmp, floor 5 GiB, before building).
```

and on a single-volume machine, one line plus an explicit statement that there
is nothing else to check, rather than two lines that look like two checks:

```
Free space OK: 224 GiB on D: (floor 20 GiB, before building).
Toolchain temp (/tmp) is on the build volume D:; the build-volume floor covers it.
```

Your item 3 asked for the volume to be named in the message. It is now named in
the tree's pass *and* refusal lines too, not just the new ones — an unnamed
"47 GiB on the build volume" is the specific thing that misled you, so leaving
the old message unnamed would have fixed the check and kept the trap.

## The knob

`--min-free-temp-gb=N`, or `BOOT_TEST_MIN_FREE_TEMP_GB=N`; `0` disables just
this check. Default is **a quarter of `MIN_FREE_GB`** (so 5 GiB at the stock
20), resolved after argument parsing so that `--min-free-gb=` still moves both,
which is what someone raising "the floor" means.

Not an equal floor, deliberately: 20 GiB is sized against a full rebuild of all
four worktrees (138 GB of build output between them when it was chosen), and
scratch is nowhere near that. A guard that refused to build on a machine with
15 GiB of temp free would be a worse bug than the one you reported.

## Two things worth knowing

**It never reclaims, and that is not scope-cutting.** `reclaim-space.py`
deletes build output under this project's worktrees. The temp volume is the
operator's system drive — VM images, installed software, none of it ours to
form an opinion about. So the refusal message says outright that
`reclaim-space.py` is *not* the remedy here and suggests `TMPDIR=/d/tmp`
instead. If it had pointed at the usual tool you would have run it, watched it
free nothing, and been worse off than with no advice.

**`check_free_space` is now a wrapper**, over `check_tree_free_space` (the old
body, renamed) and the new `check_temp_free_space` — rather than a call bolted
to the end of the old one. The old body returns early on three separate paths:
floor disabled, `df` unreadable, and floor cleared by a `--reclaim-space` retry.
A temp check appended to its tail would silently not run on two of those, which
is worse than not having one, because it reads as coverage. If you add a third
volume some day, add it to the wrapper.

## Two defects the work turned up, both fixed in the same commit

Neither was in your report; both would have bitten someone else.

1. **`resolve_temp_dir` read `$TMPDIR`/`$TMP`/`$TEMP` bare under `set -u`.**
   All three are genuinely unset on a bare Linux shell, so the first version of
   this fix would have aborted *every* boot test with
   `TMPDIR: unbound variable` before anything was built. Caught only because I
   re-ran the probe under `set -euo pipefail` with `env -u TMPDIR -u TMP -u
   TEMP` — the first probe did not inherit the script's strict mode and passed
   happily. Worth remembering if you test a `boot-test.sh` change by extracting
   functions: the extraction drops `set -euo pipefail` with them.

2. **Neither floor was validated as numeric — including `MIN_FREE_GB`, so this
   one predates the change.** `[ "$avail_gb" -lt twenty ]` prints "integer
   expression expected" to stderr and evaluates **false**, and `set -e` does not
   catch it because it sits inside an `if`. So `--min-free-gb=twenty` silently
   switched the guard off while the run looked completely normal. That is
   exactly the ok-vs-unknown conflation the three-outcome rule exists to
   prevent, reintroduced by the floor itself, so both floors now refuse a
   non-numeric value outright.

The second is the more interesting one for your purposes: it means any boot test
run before today with a typo'd `--min-free-gb=` had **no** free-space floor at
all and said nothing about it. If you have a run in your history that hit a
disk-full failure despite passing the check, that is a candidate explanation.

## Verified

Refusal, pass, same-volume, floor-`0` and non-numeric paths exercised under
`set -euo pipefail` with `TMPDIR`/`TMP`/`TEMP` unset, plus two real `--no-build`
runs of the script itself — one refusing at a 9999 GiB floor, one passing and
continuing into staging normally.

Please close `known-issues.md` →
`B-BOOT-TEST-FREE-SPACE-FLOOR-IS-BLIND-TO-THE-TEMP-VOLUME` on your side; the
entry is in your lane's section, so I have not touched it.
