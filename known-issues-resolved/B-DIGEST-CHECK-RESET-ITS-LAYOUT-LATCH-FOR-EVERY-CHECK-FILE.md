## B-DIGEST-CHECK-RESET-ITS-LAYOUT-LATCH-FOR-EVERY-CHECK-FILE (lane B, 2026-09-25) — FIXED 2026-09-25

**In short:** `md5sum -c`, `sha1sum -c` and `sha256sum -c` refuse to mix the
two untagged checksum-file layouts -- `<hex>  NAME` and the "BSD reversed"
`<hex> NAME` -- because a reversed line whose name starts with a space would
otherwise read as a standard line naming a different file. GNU latches the
layout for the **whole run**; ours latched it per check file, so
`md5sum -c A B` with `A` reversed and `B` standard verified both files where
GNU refuses every line of `B` as improperly formatted.

**Where:** `userspace/coreutils/src/digest.rs`. The latch was a field of a
`Checker` built afresh inside `check_file`; upstream's is `static int
bsd_reversed = -1;` at file scope in `src/digest.c`, set only by `split_3` and
never reset. Found while porting the rest of the family onto the module, when
the other globals `split_3` changes -- `cksum`'s algorithm, every
variable-width build's digest length -- had to become run-wide state too, and
the latch turned out to be the one of them already living somewhere
narrower.

**Fix:** the latch lives in the run's `State`, beside those. Pinned by
`scripts/digest-diff.sh` section 8 (`-c REV STD` and `-c -w STD REV`, for all
seven programs) and `scripts/cksum-diff.sh` (`-a md5 -c REV STD`).

**Impact while it lasted:** the run accepted lines GNU refuses, never the
reverse, and only across two check files named in one command -- so it could
verify a file GNU would not, which is the direction that matters for a
checksum tool, but only for a file whose own lines were all correct.
