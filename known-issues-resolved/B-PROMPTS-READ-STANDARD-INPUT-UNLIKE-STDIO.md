## B-PROMPTS-READ-STANDARD-INPUT-UNLIKE-STDIO (lane B, 2026-10-03)

**Status:** RESOLVED -- fixed 2026-10-03 (lane B), boot-confirmed on `main` 2026-10-07: the fix is in `8a7cdaf26`, whose boot passed and which was published to `main` as `73d857e1b`.

**In short:** `rm -i`, `cp -i`, `mv -i`, `ln -i` and `find -ok` read their
yes/no answers from standard input. They read more of it than they used and
kept the rest. In `{ rm -i a; cat; } < answers`, GNU's `rm` takes one line and
`cat` gets the others; ours left `cat` nothing. Two smaller faults came with
it. A prompt that could not read its answer at all (standard input closed) was
not reported, where GNU says `rm: error closing file: Bad file descriptor` and
exits 1. And `cp --help` and `mv --help` panicked when their output could not
be written.

### What was wrong, measured against GNU coreutils 9.4 and findutils 4.9

| command | ours | GNU |
|---|---|---|
| `{ rm -i a; cat; } < answers` (also `cp`, `mv`, `ln`, `find -ok`) | `cat` prints nothing | `cat` prints the lines after the first |
| `rm -i a <&-` (also `ln -i`, `cp -i`, `mv -i`) | the prompt, status 1 | the prompt, then `rm: error closing file: Bad file descriptor`, status 1 |
| `cp --help >/dev/full` (also `mv`) | a Rust panic, `failed printing to stdout` | `cp: write error: No space left on device`, 1 |
| `cp --help >&-` (also `mv`) | status 0 | `cp: write error: Bad file descriptor`, 1 |

### Why

`coreutils::yesno::StdinAnswers` read through Rust's `io::stdin()`, which
fills an 8 KiB buffer from the descriptor and never gives back what it did not
use. C's `stdin` does the same reading, but gnulib's `close_stdin` (which
upstream registers with `atexit` in `cp`, `install`, `ln`, `mv` and `rm`)
seeks a seekable descriptor back over the unread bytes, and so does glibc's
`exit` for any buffered stream that was read (`find`, which registers no
`close_stdin`). `close_stdin` also reports a stream on which a read failed.

`find -ok` made a fresh `StdinAnswers` for every prompt. That worked only
because Rust's `Stdin` is one global buffer; on any per-stream buffer it would
lose the next prompt's answers. `cp` and `mv` printed `--help` with `print!`,
which panics on a write error, and had no closed-descriptor guard, so a closed
standard input became `/dev/null` before `main` and a failed read could not be
seen.

### The fix

* `StdinAnswers` reads through `coreutils::stdio::StdioReader` -- C's `stdin`,
  block reads and a sticky end of file -- and hands the stream back with
  `into_stream()`.
* `StdioReader::close_stdin` is gnulib's `close_stdin` up to its message: the
  read-ahead goes back to a seekable descriptor, and `close_stream`'s verdict
  follows (a failed read is reported, a closed descriptor nobody read is not,
  and a failed read followed by a clean close is reported with no reason,
  because `close_stream` zeroes `errno`).
* `stdfd::close_stdin_and_stdout` is the funnel for `rm`, `cp`, `mv` and `ln`:
  it prints `PROG: error closing file[: reason]`, then does `close_stdout`'s
  work, and returns 1 if either failed.
* `find` holds one stream for the run and calls `exit_sync` on it at the end,
  as glibc's `exit` does.
* `cp` and `mv` gained `guard_std_fds!` and write `--help`/`--version`
  through `Stream` and `close_stdout`.

### How it is checked

Unit tests: `stdio.rs` has six `close_stdin` verdict tests. Differential
(drafts, then in the harnesses): 29 command lines over `rm`, `cp`, `mv`, `ln`
and `find`, covering a closed stdin, a directory as stdin, a shared seekable
stdin, a pipe, several prompts from one stream, and a sticky end of input; all
29 agree with GNU. Closed and full stdout and stderr, and closed stdin, for 26
`cp`/`mv`/`ln`/`rm` command lines: 121 of 130 rows agree, and the other nine
are `--help` text, which differs on purpose.

**Not covered:** `install` also registers `close_stdin`, but it never reads
standard input, so the funnel could only change what happens when `close(0)`
fails for a reason other than `EBADF`. It is on the guard list in
`TD-COREUTILS-AN-UNWRITABLE-STDERR-ABORTS-THE-PROCESS` and will get the funnel
when it is converted.
