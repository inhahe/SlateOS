## B-THIRTEEN-PROGRAMS-HAVE-NO-DIFFERENTIAL-HARNESS (lane B, 2026-10-07)

**Status:** OPEN (lane B).

**In short:** almost every program in `userspace/coreutils` is checked against
its upstream by a `scripts/*-diff.sh` harness, which runs both on the same
inputs in WSL and compares output, messages and status. Thirteen are not, so
nothing has ever measured whether they behave as the programs they replace:
their messages, their `-v` output, their exit statuses, and what they do with
a closed or full standard output. Found on 2026-10-07 while converting `chmod`
to the descriptor guard: its `-v` output and its help go through `println!`,
which panics when the write fails (`chmod -v 644 f >/dev/full` is Rust's panic
message, status 101, where GNU says `chmod: write error: No space left on
device`, status 1), and no harness was there to show it.

**Where:** found by listing `userspace/coreutils/src/bin/` against the
`DIFF_PROG`/`DIFF_BINS` of every harness:

| program | upstream to measure against |
|---|---|
| `chmod`, `mkdir`, `mkfifo`, `rmdir`, `readlink`, `realpath`, `sleep`, `true`, `false` | GNU coreutils 9.4 (`DIFF_GNU_SOURCE=9.4`, as `stat-diff.sh` builds it) |
| `which` | Debian's `which` (debianutils) |
| `renice` | util-linux 2.39.3 |
| `kill` | procps or util-linux -- open question B-Q22 decides which, so its harness waits for that |
| `fetch` | none: a SlateOS program, so its tests are its own |

**The fix:** a harness for each, written the way the rest are -- every option,
every message, and the descriptor cases (`<&-`, `>&-`, `>/dev/full`, `2>&-`) --
and each divergence it finds fixed. `chmod` first, since it also still lacks
the descriptor guard (`TD-COREUTILS-AN-UNWRITABLE-STDERR-ABORTS-THE-PROCESS`).
