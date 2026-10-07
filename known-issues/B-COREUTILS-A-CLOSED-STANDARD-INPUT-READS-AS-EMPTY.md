## B-COREUTILS-A-CLOSED-STANDARD-INPUT-READS-AS-EMPTY (lane B, 2026-10-03)

**Status:** FIXED 2026-10-07 (lane B), pending a boot test on `main` before
the move to `known-issues-resolved/`. Every program the 2026-10-03 sweep
found reads a closed standard input as upstream does -- 37 of them, and
the `-i`/`-ok` prompts of `rm`, `cp`, `mv`, `ln` and `find`; "Where,
measured" lists them.

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

### Where, measured

`scripts/read-error-diff.sh` is the regression net: every converted program,
with standard input closed and with a directory as standard input, against a
built coreutils 9.4. Converted so far: `base32 base64 cat comm csplit cut date
dircolors du expand factor fold head join nl numfmt od paste sort tail tee tr
tsort unexpand uniq wc` and the digests (`md5sum sha*sum b2sum cksum sum`); also `sed`, `tac`,
`shuf`, the prompts, and five whose references are not coreutils, so their own
harnesses carry the cases: `awk` (gawk 5.2.1: `init_fds` reopens a closed
descriptor the wrong way round, then `fatal: error reading input file `-'`
with status 2, and a directory is refused as it would be opened),
`cmp` (diffutils 3.10: `xstdopen` first, then
`cmp: -: Bad file descriptor` with status 2), `grep` (3.11: `(standard input)` unquoted, and an
`fstat` failure skips `-c`'s count where a read failure does not), `xargs`
(findutils 4.9: a read error ends the input, and gnulib's `close_stdin` reports
it at exit, giving a seekable standard input back its read-ahead) and `strings`
(binutils 2.42: a failed read is the end of the input, silently).

Three lessons from converting them, each of which bit more than once:

* **Descriptor 1 has the same trap.** `io::stdout()` answers a write's
  `EBADF` with success, so once the guard keeps a closed descriptor 1 closed,
  a program writing through it exits 0 on `>&-`. `stdfd::RawStdout` (or a
  `Stream`) is the writer for a program that reports its own write errors.
* **A file opened while descriptor 0 is closed becomes descriptor 0.** GNU's
  `join` and `comm` include gnulib's `stdio--.h`, whose `fopen` never returns
  0, 1 or 2; without the same (`stdfd::fd_safer`), `join f - <&-` read `f`
  twice. `paste` uses a plain `fopen` and says `standard input is closed`.
* **`freopen` leaves `EBADF` behind.** A program that `freopen`s a named
  input onto `stdin` (`uniq`, `shuf`) reports a failed open with the errno of
  glibc's close of the old descriptor: `uniq nosuch <&-` is
  `uniq: nosuch: Bad file descriptor`.

A sweep of 49 command lines on 2026-10-03 found the rest. Each needs
`stdfd::RawStdin` (or `stdio::StdioReader`), the guard if it lacks one, and
upstream's wording, which is in this table, with standard input closed unless
noted. `du`, `date` and `csplit` were converted on 2026-10-07; what each
needed beyond the descriptor is in its commit -- `du` streams its list and
goes on after a read error, still printing `-c`'s total, and `csplit` reads
as its split needs lines, so the error lands after the first piece's file
is made. `awk`, the last, followed the same day: its table row read
`awk: fatal: error reading input file `-': Bad file descriptor`, 2, and
that is what it says now.

Other files contain `io::stdin()` too, some only for a tty check or in a
comment: `bc diff ed find more patch sed sh split tar test`, and in the
library `filekind.rs`. The sweep found no
divergence for `split`, `pr`, `fmt`, `ptx`, `tac`, `shuf`, `diff`.
