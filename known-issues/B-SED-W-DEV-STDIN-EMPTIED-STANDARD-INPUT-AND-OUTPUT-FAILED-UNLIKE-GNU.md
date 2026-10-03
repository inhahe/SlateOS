## B-SED-W-DEV-STDIN-EMPTIED-STANDARD-INPUT-AND-OUTPUT-FAILED-UNLIKE-GNU (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B), boot confirmation pending.

**In short:** two of these destroyed data. `sed 'w /dev/stdin' f < g`
re-opened standard input for writing, so the file `g` was emptied and `f`'s
lines written into it; GNU sed refuses the write (`couldn't write 1 item to
stdin: Bad file descriptor`) and leaves `g` alone. And `sed -i 'w /dev/stdout'
f` wrote the copies meant for the terminal into `f` itself. The rest were
fidelity: when its output could not be written, our sed said something
different from GNU's, or nothing at all, and a line with no final newline
gained or lost one in four places. Found by porting GNU's output layer, which
names every write that fails, and measuring each piece.

### What was wrong, measured against GNU sed 4.9

| | ours | GNU |
|---|---|---|
| `sed -n 'w /dev/stdin' f < g` | `g` emptied, then given `f`'s lines; 0 | `couldn't write 1 item to stdin: Bad file descriptor`, 4; `g` untouched |
| `sed -i 'w /dev/stdout' f` | the copies written into `f` | the copies on standard output, `f` unchanged |
| `sed p f >/dev/full` | `couldn't write: No space left on device` | `couldn't flush stdout: No space left on device` |
| `sed p long-line >/dev/full` | the same | `couldn't write 5000 items to stdout: ...` |
| `sed p f >&-`, `sed -n 9p f >&-` | 0, silently | `couldn't close stdout: Bad file descriptor`, 4 |
| `sed -n 'w /dev/stderr' f 2>&-` | 0 | 4 |
| `sed 'w out' f >/dev/full` | `out` cut short at the first line | `out` complete: standard output fails only at the end |
| `printf 'a\nb' \| sed 'w /dev/stdout'` | `a a b b`, newline-separated | `a`, `a`, `bb` -- `w /dev/stdout` keeps its own debt |
| `sed -s -n '$p' unterminated other` | `bc` | `b`, newline, `c` -- the debt crosses files under `-s` |
| `printf z \| sed q` | `z` | `z` and a newline -- `q` pays the debt |
| `sed -e 'a X' -e N f` | the appended text before the last line | after it: `N` at the end of input reads nothing, so dumps nothing |
| `sed -e 'a X' -e Q f` | `X` | nothing: `Q` drops the queue |

### Why

The old sed wrote through Rust's `io::stdout()`, which is line-buffered even
into a file or a pipe, ignored every flush's result, and answered any failure
with one message of its own. GNU sed checks every `fwrite`, `fflush` and
`fclose` and words each failure differently, and *which* write fails depends
on where glibc's buffer fills -- so the port needed glibc's buffer arithmetic,
not just GNU's sentences.

`w /dev/stdin`, `/dev/stdout` and `/dev/stderr` are GNU's `special_files`
(`compile.c`): they are never opened, but stand for the streams the process
already has, each as an output of its own with its own missing-newline flag.
Ours special-cased two of them through the *main* output -- sharing its flag,
and under `-i` its destination -- and opened the third.

### The fix

* `userspace/coreutils/src/stdio.rs` (new): glibc's `FILE` for writing --
  `st_blksize`-sized buffer chosen at the first write, `_IO_new_file_xsputn`'s
  fill/flush/direct-write arithmetic (including that the first write finds no
  buffer and sends whole blocks straight through, and that a buffer under 128
  bytes is not kept aligned), a failed flush discarding the buffer, `ferror`.
* `sed.rs`: GNU's `ck_fwrite`/`ck_fflush`/`ck_fclose` and their messages;
  `ck_fclose (NULL)` at the end (`w` files newest first, then standard
  output); `panic` and other fatal exits as `exit`, which flushes and checks
  nothing; `-u` as `flush_output`; the append queue dumped where GNU dumps it;
  writes cut into the pieces GNU's are (`l` per character, `r` in 8192 bytes,
  `e` in 4096); `=`, `F` and `--debug`'s trace unchecked as GNU's `fprintf`
  is; `w /dev/stdin|stdout|stderr` as special streams with debts of their
  own; the output's debt carried across files under `-s` and dropped under
  `-i`; `guard_std_fds!` so a closed descriptor stays closed.
* One measured difference is kept on purpose: GNU unlinks a `w` file from its
  list of names before the flush that closes it, so a failure there reads
  `couldn't flush <unknown>`. Ours names the file. (`xfail` in the harness.)

### Verified

`scripts/sed-diff.sh`: 537 passed, 0 differed, 6 differ on purpose (was 483
and 5) -- 54 new cases: a `redir_case` helper that sends standard output to
`/dev/full` or closes it, does the same to standard error, or merges the two,
and compares what is left; the debt cases above; and `run_inplace` now
compares standard output as well, which it used to discard (that is how the
`-i` + `w /dev/stdout` corruption went unseen). `OURS=/usr/bin/sed` turns all
six differences into XPASS. Unit tests: `sed` 104 (3 new), `stdio` 7.

### Not covered here

The input side has the same kind of gaps, measured and next: a closed
standard input should be `read error on stdin: Bad file descriptor`; `q`
should leave a seekable input positioned after the last line read; `-u`
should read unbuffered; `R /dev/stdin` should share GNU's `stdin` stream and
`r /dev/stdin` should re-open the name. And `--posix`/`POSIXLY_CORRECT`, under
which the three names above are ordinary files again:
`TD-B-SED-HAS-NO-POSIX-MODE-AND-NO-FOLLOW-SYMLINKS`.
