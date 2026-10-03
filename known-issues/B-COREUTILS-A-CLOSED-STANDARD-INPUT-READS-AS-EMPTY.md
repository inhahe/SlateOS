## B-COREUTILS-A-CLOSED-STANDARD-INPUT-READS-AS-EMPTY (lane B, 2026-10-03)

**Status:** OPEN (lane B). Fixed so far in `sed`, `tac`, `shuf`, `wc`, and
the `-i`/`-ok` prompts of `rm`, `cp`, `mv`, `ln` and `find`.

**In short:** when a program is started with its standard input closed
(`prog <&-`) and then reads it, GNU's tools report it as an error, for
example `wc: 'standard input': Bad file descriptor` with status 1. Many of ours instead behave as if the input were empty, with no
message and status 0. A script that accidentally closes stdin gets a quiet
wrong answer (`wc` says `0 0 0`) instead of an error.

### Why

Two layers hide it:

1. **Rust's runtime** opens `/dev/null` on any of descriptors 0-2 that is
   closed when the process starts. `coreutils::guard_std_fds!` and
   `stdfd::restore` undo that. They are now in every program except those
   listed in `TD-COREUTILS-AN-UNWRITABLE-STDERR-ABORTS-THE-PROCESS`.
2. **`std::io::stdin()`** treats `EBADF` from `read(2)` as end of input
   (std's `handle_ebadf`), by design, for the same reason. So even with the
   descriptor really closed, a program that reads through `io::stdin()`
   sees an empty file.

### The fix

Read descriptor 0 directly rather than through `io::stdin()`, so that
`EBADF` arrives as an error:

* `stdfd::read(0, ..)` for a raw read;
* `coreutils::stdio::StdioReader::stdin()` where upstream's buffering is
  observable (C's `stdin`: block reads, sticky EOF, give-back at exit).

Then print each program's own GNU wording, which differs between programs
(`wc`, `cat`, `head` and `tac` each phrase it their own way, and some add a
second message when `close (STDIN_FILENO)` fails at the end). Measure each
program with `<&-` against its reference before and after the change, and
check what it creates on the way. `tac` was the example: it made its
temporary file with a plain open, which then took descriptor 0 and was read
back as the input. Upstream's tools use gnulib's `*_safer` openers, which
are `stdfd::fd_safer` here.

### `wc` (fixed 2026-10-03, boot confirmation pending)

GNU coreutils 9.4, standard input closed:

| command | GNU output (2>&1), status 1 in every row |
|---|---|
| `wc` | `wc: 'standard input': Bad file descriptor`, `0 0 0`, `wc: -: Bad file descriptor` |
| `wc -l` / `-c` / `-L` | the same, with the one count `0` |
| `wc -` | `wc: -: Bad file descriptor`, `0 0 0 -`, `wc: -: Bad file descriptor` |
| `wc f -` | `4 4 8 f`, `wc: -: Bad file descriptor`, `0 0 0 -`, `4 4 8 total`, `wc: -: Bad file descriptor` |

So three things: the read error names the input (`'standard input'`
unnamed, `-` as an operand); the counts line is **still printed**, with
whatever was read; and the `close (STDIN_FILENO)` at the end fails too and
says `-`. The same shape applies to a standard input that is open but cannot
be read: `wc < dir` was `wc: -: Is a directory` and no row, where GNU says
`wc: 'standard input': Is a directory` and prints `0 0 0`.

Reading descriptor 0 directly was not enough on its own: `wc.rs` treated a
read failure as "no row for this input". It now follows upstream's
`wc_file`/`wc` split. An input that could not be opened has no row. One that
opened is counted as far as it was read, with the read error reported before
its row and a failed `close` after it. Descriptor 0 is closed last, after
the total. A `--files0-from` list that cannot be read follows upstream's two
paths as well. A list read up front fails fatally, with no reason. A streamed
one reports `-: read error: …` after the rows of the names it did read, and
carries on. `wc-diff.sh` gained 24 cases for all of this (`run_closed`,
`run_from`); 19 of them failed on the old binary, and all 149 cases pass.

### Where

These programs contain `io::stdin()` or `stdin().lock()` (some only for a
tty check or in a comment, so each needs looking at):
`awk bc cat comm csplit cut date diff dircolors du ed expand factor find fold
grep head join more nl numfmt od paste patch sed sh sort split strings tail
tar tee test tr tsort unexpand uniq xargs`, and in the library
`basenc.rs`, `digest.rs`, `filekind.rs`. An earlier `<&-` measurement found
about 16 of them diverging from GNU. The measurement is simple to repeat: in
WSL, run each command line with `<&-` against our binary and the reference,
both reached by bare name through a one-entry `PATH` directory, and compare
`2>&1` output and status.
