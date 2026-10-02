## `split` ran its `--filter` through `/bin/sh` whatever `$SHELL` said, called a killed command `exit 1`, and could truncate its own input; it is GNU's `create` and `closeout` now (lane B, 2026-10-02) — **FIXED** 2026-10-02

**Fixed** while replacing the last lossy decodes in coreutils' diagnostics,
which led into `run_filter` and showed the rest. Each was measured against GNU
split 9.4 and is now a case in `scripts/split-diff.sh` (243 agree):

| | was | now (upstream's) |
|---|---|---|
| the filter's shell | always `/bin/sh` | `$SHELL`, `/bin/sh` only when unset; `argv[0]` its last component; found as `execl` finds it, never through `PATH` |
| a shell that cannot run | `with FILE=xaa: No such file or directory` | the child's `failed to run command: "SH -c CMD": ...`, then `with FILE=xaa, exit 1 from command: CMD` |
| a command killed by a signal | `exit 1`, status 1 | `signal TERM` (gnulib's `sig2str`, now `coreutils::sig2str`), status 128 + the signal; `SIGPIPE` not a failure |
| `SIGPIPE` for the command | always the default | ignored if split was started with it ignored (`default_SIGPIPE`; `stdfdguard` now records it before the runtime replaces it) |
| a write to the pipe failing | every error ignored | only `EPIPE`; anything else is `NAME: <errno>` |
| `--verbose` | `executing with FILE=` + the raw name, line-buffered | `quotef`, and stdio's buffering, so after the commands' own output |
| an output name that is the input | truncated it | `'in.txt' would overwrite input; aborting` (`O_EXCL` first, then `fstat` and compare, then `ftruncate`) |
| a failed read | `NAME: read error: ...` | `NAME: ...`, or `NAME: cannot determine file size: ...` in the two chunk modes |
| standard output | `print!`, which panics on a write error | stdio's verdict at the close: `write error: ...`; `-n K/N` names `-` |
