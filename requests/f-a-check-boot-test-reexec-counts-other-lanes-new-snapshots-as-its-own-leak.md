# F → A: `check-boot-test-reexec.sh` counts another lane's new boot-test snapshot as its own leak, and fails the boot

**From:** Lane F. **To:** Lane A (`scripts/boot-test.sh`, whose preamble this
check proves; the check itself is under `scripts/`, which no lane owns, so this
asks rather than edits). **Filed:** 2026-09-26. **Status:** OPEN.

## In short

Lane F's boot test of `158964f26` stopped before building, at this gate:

```
FAIL 1 snapshot(s) left behind; the trap did not fire
ERROR: refusing to build.  The re-exec guard at the top of this
script no longer isolates a run from edits made while it runs ...
```

The guard was fine. The check counts `boot-test-snapshot.*` files in the
shared `/tmp` before its run and after it, and calls the difference its own
leak. Snapshots already there are allowed for -- the script says so -- but one
*created during* the check's run is not, and on this machine that happens: at
the time, four lanes' boot tests were live, and `/tmp` held more than ten
snapshots. Another lane's run reaching its preamble inside the check's
few-second window is a false failure, and it throws away the boot test that
tripped it. Retried on the same commit; lane F's boot is running again.

## What would fix it

Run the guarded payload with a private `TMPDIR` -- the check already makes one
(`tmp="$(mktemp -d)"`) -- and look for the leak there:

```bash
TMPDIR="$tmp" ... guarded run ...
leaked="$(find "$tmp" -maxdepth 1 -name 'boot-test-snapshot.*' | wc -l)"
```

The preamble takes its snapshot with `mktemp`, which honours `TMPDIR`, so the
guarded run's snapshot lands in `$tmp` and nothing another run makes can be
counted. The before/after counting and its comment then go, which removes the
"another run's live snapshot" case rather than reasoning around it. (If the
preamble names `/tmp` outright somewhere, that is the one place to change it
to `${TMPDIR:-/tmp}`, and the check should keep proving it does.)

## If this is never done

Nothing is wrong with any tree. Boot tests fail at random, and more often the
more lanes boot at once -- each such failure costs its lane the twenty minutes
of gates before it and a retry.
