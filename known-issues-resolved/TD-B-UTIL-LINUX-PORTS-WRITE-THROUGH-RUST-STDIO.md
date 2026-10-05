## TD-B-UTIL-LINUX-PORTS-WRITE-THROUGH-RUST-STDIO (lane B, 2026-09-26) — ✅ FIXED 2026-09-26 (lane B)

**In short:** util-linux programs end with `close_stdout`, which decides the
exit status from what happened to standard output and standard error --
and a Rust program cannot see most of that through `println!` and
`eprintln!`: the runtime reopens a closed descriptor on `/dev/null` before
`main`, and Rust's `Stdout`/`Stderr` report a write to a closed descriptor
as a success. `lsmem` and `getopt` now do what util-linux does (the
`stdfdguard` and `ulclosestream` crates; `getopt-diff.sh` asks 44 cases of
closed and full descriptors); `flock` and `logger` still write through `std`,
so for them `>&-` and `2>&-` are invisible, and output that outgrows glibc's
buffer before failing is reported with a reason where upstream gives none.

**Where:** `userspace/flock/src/main.rs` (`Out`, which flushes through
`io::stdout()`), `userspace/logger/src/main.rs` -- their stdout and
diagnostic paths.

**How to see it:** `flock --bogus 2>&-` -- upstream exits 1, not its usage
status 64: the diagnostic could not be written, and `close_stdout` answers a
lost diagnostic with `CLOSE_EXIT_CODE`. Ours exits 64. (`getopt -o a -- -x
2>&-` was the same, 1 against upstream's 3, until getopt was converted.)

**The proper fix:** in each, `stdfdguard::guard_std_fds!()` at module scope
and `stdfdguard::restore()` first in `main`; stdout through
`ulclosestream::Stdout` (with the program's own `CLOSE_EXIT_CODE` -- 3 for
`getopt`); diagnostics through `ulclosestream::warnx`/`warn`/`stderr_write`;
and cases for `>&-`, `>/dev/full`, `2>&-` and `2>/dev/full`, with small and
large output, in each program's harness.

**Fixed** as described, in all four: `lsmem` (348 cases), `getopt` (151, 44
of them these), `flock` (114, 36 -- including the command inheriting a
closed stdout, which with the lock file on descriptor 1 is upstream's), and
`logger` (170, 32 -- including `-s`, whose copy to stderr is `writev` and
not stdio, so its failure is no lost diagnostic: `ulclosestream::stderr_raw`).
Two refinements came out of the measuring: glibc sizes stdout's buffer at
the first write (8192 on a closed descriptor, `st_blksize` otherwise), and
upstream lsmem's held `/sys` descriptor is what descriptor 1 is by then.
**How to see it.** `target/fontcheck` draws emoji lines from any font given
it; `target/colr_compare.py` compares with Edge.
