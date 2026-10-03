## B-PATCH-ORIGFILE-PATCHFILE-EXITS-0-HAVING-DONE-NOTHING (lane B, 2026-09-25) — FIXED 2026-09-25

**In short:** `patch ORIGFILE PATCHFILE` -- the second commonest way to run
`patch` -- exits 0 and changes nothing. Our parser keeps only the *last*
operand, as the file to patch, and reads the patch from standard input, which
in that command is whatever the terminal or the caller left there. GNU applies
PATCHFILE to ORIGFILE.

Measured in WSL against GNU patch 2.7.6:

```text
$ printf 'a\n' > o; printf 'b\n' > n; diff -u o n > p.diff
$ cp o o2; patch o2 p.diff               # GNU: "patching file o2"; o2 now holds b; rc 0
$ cp o o3; patch o3 p.diff </dev/null    # ours: no output; o3 still holds a; rc 0
```

**Where:** `userspace/coreutils/src/bin/patch.rs`, `parse_args` -- its final
`else` arm is `opts.target_file = Some(arg.clone())`, so every operand
overwrites the one before and there is no second-operand slot at all.

**How it was closed (2026-09-25).** `patch`'s command line is upstream's, on
the shared parser: GNU patch 2.7.6's `shortopts` and `longopts` in declaration
order (`--merge` included, as the build measured has `ENABLE_MERGE`), and
`get_some_switches`' operand rule -- `ORIGFILE`, then `PATCHFILE`, which
overrides `-i`, then `extra operand` at status 2. `--help` and `--version`/`-v`
are answered where getopt meets them rather than found by scanning all of argv
first, so `patch --bogus --help` reports the bad option, as GNU does. Numbers go
through upstream's `numeric_string`, so `-F x` is `fuzz factor x is not a
number` rather than this build's `invalid fuzz factor`, and `-p -1` is `strip
count -1 is negative`. The options GNU has and this build does not (`-B`, `-D`,
`-e`, `-g`, `-t`, `-T`, `-V`, `-x`, `-Y`, `-z`, `--merge`, `--posix`,
`--quoting-style`, `--reject-format`, `--read-only`, `--follow-symlinks`,
`--binary`, `--backup-if-mismatch`) are refused by name instead of as invalid.
Upstream's CVS 1.9 hack that reads `-b SUFFIX ORIGFILE PATCHFILE` as `-b -z
SUFFIX` is not reproduced: it is a spelling of `-z`.

Pinned by `scripts/patch-diff.sh`'s command-line block -- `patch ORIGFILE
PATCHFILE` with and without `--dry-run`, the second operand over `-i`, a third,
`--dry`, `-sp1`, `--st=1`, `--s`, `--`, the three number refusals, `--bogus
--help`, and `POSIXLY_CORRECT` -- at 113 passed, 0 differed. One row differs on
purpose: GNU's `numeric_string` tests for overflow after the multiply has
already overflowed an `int`, the compiler deletes the test, and GNU patches
with a wrapped `-F 99999999999`; ours refuses it as `too large`, as upstream's
source says.
