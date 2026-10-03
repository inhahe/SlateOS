## D-POSIX-ERROR-PRINTED-THE-SHORT-NAME-AND-CUT-LONG-MESSAGES — `error` and `error_at_line` printed the wrong program name, a space glibc's does not, cut messages at 1023 bytes and flushed nothing; `err` and `warn` cut them too; both wrote around the `stderr` stream (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 (`posix/src/error.rs`, `posix/src/err.rs`)**

**In short:** GNU programs report errors through `error()`, BSD-derived ones
through `err()` and `warn()`. Ours printed something close to glibc's, but
not the same: `error` named the program by the last part of its path where
glibc names it as it was started, put `error_at_line`'s file after a space
(`prog: f.c:12:` where glibc prints `prog:f.c:12:`), silently cut any message
longer than 1023 bytes, and did not first flush what the program had written
to standard output -- so when both went to one file, the error could appear
before the output that led to it. Test suites that compare a tool's output
against a recorded file saw the difference.

| | Was | Is (glibc 2.39's) |
|---|---|---|
| `error`'s name | `__progname`, argv[0]'s last component | `program_invocation_name`, argv[0] as started |
| `error_at_line`'s place | `prog: file:12: `; nothing for a NULL file | `prog:file:12: `; one space for a NULL file |
| a message over 1023 bytes | cut, silently (`error`, `err`, `warn` alike) | whole |
| `stdout` before `error` | not flushed | flushed first, so earlier output comes first |
| `stderr` | written around the stream, to file descriptor 2 | through the stream, under its lock, flushed after |

`err` and `warn` already named the short name, as glibc's do, and do not
flush `stdout` -- glibc's do not either. `posix/tools/oracle/errfns_harness.py`
records glibc's output for each case, the two streams on one pipe as a
redirected program's are, and the tests replay it through two streams over
one sink.

**Where:** `posix/src/error.rs`, `posix/src/err.rs`.
