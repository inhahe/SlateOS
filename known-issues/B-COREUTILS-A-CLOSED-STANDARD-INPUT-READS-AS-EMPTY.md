## B-COREUTILS-A-CLOSED-STANDARD-INPUT-READS-AS-EMPTY (lane B, 2026-10-03)

**Status:** OPEN (lane B). Fixed so far in `sed`, `tac`, `shuf`, `wc`, `cut`,
`expand`, `fold`, `nl`, `paste`, `unexpand`, and the `-i`/`-ok` prompts of
`rm`, `cp`, `mv`, `ln` and `find`. The rest are listed, with GNU's wording,
under "Where, measured".

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
built coreutils 9.4. Converted so far: `cut expand fold nl paste unexpand wc`
(also `sed`, `tac`, `shuf` and the prompts, which their own harnesses cover).

A sweep of 49 command lines on 2026-10-03 found the rest. Each needs
`stdfd::RawStdin` (or `stdio::StdioReader`), the guard if it lacks one, and
upstream's wording, which is in this table, with standard input closed unless
noted:

| program | GNU (each line a separate message, then status) |
|---|---|
| `cat` | `cat: -: Bad file descriptor`, `cat: closing standard input: Bad file descriptor`, 1 |
| `head` | `head: error reading 'standard input': Bad file descriptor`, `head: -: Bad file descriptor`, 1 |
| `tail` | `tail: cannot fstat 'standard input': Bad file descriptor`, `tail: -: Bad file descriptor`, 1 |
| `sort` | `sort: stat failed: -: Bad file descriptor`, 2 |
| `uniq` | `uniq: error reading '-': Bad file descriptor`, 1 |
| `tr`, `base64`, `base32`, `join` | `PROG: read error: Bad file descriptor`, 1 |
| `tee` | `tee: read error: Bad file descriptor`, `tee: standard input: Bad file descriptor`, 1 |
| `od` | `od: 'standard input': Bad file descriptor`, then `0000000` on stdout, `od: standard input: Bad file descriptor`, 1 |
| `sum`, `cksum`, `md5sum`, `sha256sum` | `PROG: -: Bad file descriptor`, `PROG: standard input: Bad file descriptor`, 1 |
| `tsort` | `tsort: -: read error: Bad file descriptor`, 1 |
| `dircolors -` | `dircolors: -: read error: Bad file descriptor`, `dircolors: -: Bad file descriptor`, 1 |
| `du --files0-from=-` | `du: -: read error: Bad file descriptor`, 1 |
| `date -f -` | `date: 'standard input': read error: Bad file descriptor`, 1 (a directory: ours drops the quotes) |
| `numfmt`, `factor` | `PROG: error reading input: Bad file descriptor`, 1 |
| `grep` | `grep: (standard input): Bad file descriptor`, 2 (a directory: ours says `-:`; with `-c`, GNU still prints `0`) |
| `comm - f` | `comm: -: Bad file descriptor`, 1 (ours went on comparing) |
| `cmp - f` | `cmp: -: Bad file descriptor`, 2 |
| `csplit - 1` | `csplit: read error: Bad file descriptor`, then `0` on stdout, 1 (a directory: ours omits the `0`) |
| `xargs` | `xargs: error closing file: Bad file descriptor`, 1 (a directory: `xargs: error closing file`, no reason) |
| `awk 1` | `awk: fatal: error reading input file `-': Bad file descriptor`, 2 (a directory: gawk warns `command line argument `-' is a directory: skipped` and exits 0) |
| `strings` | a directory as standard input: binutils says nothing and exits 0; ours reports it |

Other files contain `io::stdin()` too, some only for a tty check or in a
comment: `bc diff ed find more patch sed sh split tar test`, and in the
library `basenc.rs`, `digest.rs`, `filekind.rs`. The sweep found no
divergence for `split`, `pr`, `fmt`, `ptx`, `tac`, `shuf`, `diff`.
