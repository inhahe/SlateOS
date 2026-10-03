## TD-B-A-FAMILY-HARNESS-CANNOT-BE-AIMED-AT-ONE-HALF-OF-A-PAIR (lane B, 2026-09-11)

**Six harnesses here cover several binaries at once via `DIFF_BINS`, and for
those the `DIFF_PKG` knob cannot select which half of a duplicate pair is
measured. What it selects instead is a coin flip.**

`DIFF_PKG=sha256sum` fails outright — cargo is asked for an `md5sum` bin in the
`sha256sum` package, because `DIFF_BINS` names the whole family. The obvious
next try, `DIFF_PKG="coreutils sha256sum"`, builds *both* packages' copy of
`sha256sum` into the same path, and whichever links last wins:

    $ DIFF_PKG="coreutils sha256sum" PROG=sha256sum ./scripts/digest-diff.sh
    113 passed,  0 differed     <- sha256sum (SlateOS coreutils)
    113 passed,  0 differed     <- sha256sum (SlateOS coreutils)
     39 passed, 74 differed     <- sha256sum (Slate OS)

Three runs of one command, no edits between them, subject confirmed by
`--version` each time.

**This is the same defect family as `DIFF_PKG` not crossing the WSL boundary**
— an authoritative-looking pass count about a binary nobody chose — with one
thing worse: it is *non-deterministic*, so it cannot be reproduced into a bug
report and cannot be caught by a gate that runs once. The only reason the
`sha256sum` numbers below are trustworthy is that `--version` was checked on
every single run, which is a discipline and not a mechanism.

**Which names this affects:** `calc-diff.sh` (bc, dc), `digest-diff.sh` (md5sum,
sha256sum), `interleave-diff.sh` (eleven names), `write-error-diff.sh` (eleven
names), `time-diff.sh`, `osh-diff.sh`.

**The proper fix.** Build each side to a path of its own instead of letting both
write `debug/<name>`. `diff-wsl.sh` already reaches every subject through
`$bindir/{ours,gnu}/NAME`, so the missing piece is a per-package target
directory — `--target-dir` per `DIFF_PKG` entry — after which the symlink can
point at the right one deliberately rather than at whatever survived. Until then
the survey's column says "coreutils half only", which is the truth.
