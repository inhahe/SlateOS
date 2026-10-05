## B-SED-ATE-THE-REST-OF-A-SHARED-STANDARD-INPUT (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B), boot confirmation pending.

**In short:** `{ sed 1q; cat; } < file` is how a script takes the first line
of a file and hands the rest to the next command. With GNU sed, `cat` prints
the rest; with ours it printed nothing, because our sed read ahead into a
buffer of its own and never gave back what it had not used. The same habit
broke a child started by sed's `e` command (it was given an empty standard
input instead of sed's), `-u` (which exists so that sed takes no more than it
uses), `R /dev/stdin` (which read the input a second time instead of taking
turns with it), and the `F` command at a file boundary. And three kinds of
read failure were silent where GNU stops with a message.

### What was wrong, measured against GNU sed 4.9

| | ours | GNU |
|---|---|---|
| `{ sed 1q; cat; } < f` (`f` = `1 2 3`) | `1` | `1 2 3`: `exit` seeks back over what was read ahead |
| `{ sed -n '2{p;q}'; cat; } < big \| head -3` | `2`, then a line cut in half | `2 3 4` |
| `printf '1\n2\n3\n' \| { sed -u 1q; cat; }` | `1` | `1 2 3`: `-u` reads a byte at a time |
| `{ sed -u '$!q'; cat; } < f` | `1` | `1`, a blank line, `3`: the byte `$` looked at is not given back |
| `sed -u '1e cat' < f` | `1 2 3` -- `cat` was given an empty input | `2 3 1` |
| `sed 'R /dev/stdin' - < f` | `1 1 2 2 3 3` | `1 2 3`: one stream, taken in turns |
| `sed 'r /dev/stdin' a < f` (two lines in `a`) | `f` after the first line only | after both: a file re-opened starts again |
| `sed -n '$!F' a b` | `a a` | `a b`: `$` opened `b` to look |
| `sed 'R dir' a`, `sed 'r dir' a` | `a`'s lines, 0 | `read error on dir: Is a directory`, 4 |
| `sed -n '$p' a dir` | `read error on dir`, 4 | `a2`, 0: `$` takes an unreadable file for an empty one |
| `sed p <&-` | nothing, 0 | `read error on stdin: Bad file descriptor`, 4 |
| `sed 'R /dev/stdout' a` | `a`'s lines | `read error on stdout: Bad file descriptor`, 4 |

### Why

Every input was a Rust `BufReader`, 8 KiB at a time, and standard input was
`io::stdin()` -- which also swallows `EBADF`, so a closed descriptor read as
empty. `$` was answered by reading the whole next line ahead. `R` read
through a second `BufReader` on a second open of `/dev/stdin`. `e` ran its
command through `Command::output`, which gives the child an empty standard
input unless told otherwise -- the doc comment above it said "inherited".

### The fix

* `coreutils::stdio::StdioReader` (new): glibc's `FILE` for reading -- a
  block of `st_blksize` (or `BUFSIZ`) per `read(2)`, or one byte after
  `setvbuf (_IONBF)`; `getdelim`; a one-byte look-ahead (`getc`/`ungetc`);
  sticky end of file until `clearerr`; `rewind`; and `exit_sync`, the
  `lseek` back over unread bytes that `exit` makes for a buffered stream that
  has been read (`_IO_unbuffer_all`), skipped for an unbuffered one and
  ignored on a pipe. `stdfd::seek_current` and `stdfd::read_at` under it.
* `sed`'s `Input` is GNU's `struct input`: `read_pattern_space`, `test_eof`
  with its one-byte look-ahead, `last_file_with_data_p` opening the next
  operand to answer `$`, `in_file_name` as the file most recently opened (what
  `F` and `--debug` print), `-u` setting each input unbuffered, standard input
  one shared stream (`STDIN`) never closed between operands, a read failure
  fatal (`read error on NAME: ...`), and the seek back at every exit.
* `R` sources are GNU's: `/dev/stdin` is the shared stream, `/dev/stdout` and
  `/dev/stderr` streams that refuse a read, any other file a stream of its own
  whose read failure is fatal. `r` is GNU's `print_file`: opened afresh, copied
  in 8192-byte pieces, a read failure fatal after what was read is written.
* `e` gives its child sed's standard input.
* `r /dev/stdin` on SlateOS: devfs's `/dev/stdin` is the console, not
  descriptor 0, so sed re-opens descriptor 0's file itself when the name it
  opened is not that file -- `requests/b-a-dev-stdin-stdout-and-stderr-are-
  console-nodes-not-the-callers-descriptors.md` asks lane A for Linux's
  answer, after which the stand-in is never chosen. On Linux it never is.

### Verified

`scripts/sed-diff.sh`: 574 passed, 0 differed, 6 differ on purpose (was 537)
-- 37 new cases, through a new `shell_case` helper that runs a shell snippet
with each side's sed on `PATH` and compares everything printed, so where the
*next* reader starts is compared. Against the previous sed, about thirty of
them differ. Unit tests: `sed` 106 (2 new, the `$` boundary), `stdio` 15 (8
new, the reader).
