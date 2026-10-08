## TD-B-SORT-HAS-NO-EXTERNAL-MERGE-RANDOM-SORT-OR-DEBUG (lane B, 2026-10-08)

**Status:** OPEN. Found while checking `sort` for the safer opens
(`TD-B-GUARDED-PROGRAMS-OPEN-FILES-WITHOUT-OPEN-SAFER`): three things GNU's
`sort` does that ours does not. They were written down on 2026-08-16 as
"remaining limitations" inside an entry that was then moved to
`known-issues-resolved/` (`B-b-sort-had-three-flags-and-got-all-three-wrong`),
so nothing open tracked them since.

**In short:** `sort -R` and `sort --debug` are refused, and every input is
sorted in memory, so a file larger than memory cannot be sorted at all and
the options that steer GNU's temporary files are accepted and ignored.

| what | ours | GNU coreutils 9.4 |
|---|---|---|
| `-R`, `--random-sort` | `sort: random sort is not implemented: it needs a keyed hash this system does not have yet`, status 2 | orders lines by the MD5 of each key, salted with 16 bytes from `--random-source` (or the system's random numbers): equal keys stay together, and with a fixed source the order is reproducible |
| `--debug` | `sort: --debug is not implemented`, status 2 | prints each line with its keys underlined, after warnings about the options |
| inputs larger than the sort buffer | read whole into memory | sorted a buffer at a time into temporary files (`-T DIR`, `$TMPDIR`, `/tmp`), merged `--batch-size` at a time, compressed through `--compress-program` |
| `-S`, `-T`, `--batch-size`, `--compress-program`, `--parallel` | accepted, ignored | obeyed; observable with a small `-S`, e.g. `sort -S 1k -T /nonexistent big` is `sort: cannot create temporary file in '/nonexistent': No such file or directory`, status 2 |

**Where:** `userspace/coreutils/src/bin/sort/main.rs` (`RANDOM_UNIMPLEMENTED`,
`DEBUG_UNIMPLEMENTED`, and the `b'S' | b'T' | b'y'` arm that discards the
resource options).

**The fix:** port each from `sort.c`, held to GNU's by `sort-diff.sh`:
`-R` with `--random-source` (deterministic, so the harness can compare
orders exactly); `--debug`'s annotations and warnings; and the external
merge -- whose temporary files are `mkstemp_safer`'s, as upstream's are.
