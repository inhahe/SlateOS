## TD-B-GUARDED-PROGRAMS-OPEN-FILES-WITHOUT-OPEN-SAFER (lane B, 2026-10-07)

**Status:** PARTLY FIXED 2026-10-08 (lane B): `tee`, the program whose
difference corrupted data, opens its files as its upstream does now, and so
do `csplit`, whose pieces kept a diagnostic inside them, the digest
programs, whose `--check` read its own list as a `-` line's input, `pr`,
whose `-m` read a file twice -- once as itself and once as `-` -- and the
`freopen` programs (`uniq`, `shuf`, `tsort`, `dircolors`, `du
--files0-from`, `ptx -G`), one of which aborted.
**OPEN** for the other programs whose upstream keeps its files off
descriptors 0-2 (the list at the end): each is converted with the harness
case that shows the difference, not all at once.

**In short:** a program can be started with a standard descriptor closed
(`tee out.txt >&-`). The descriptor guard (`guard_std_fds!`) keeps it closed,
as upstream's programs see it, so that `write error` is reported where they
report it. But then the next file the program opens takes the lowest free
descriptor -- 0, 1 or 2 -- and *is* that standard stream. Whether that
happens is upstream's choice, made program by program, and observable; a port
has to make the same choice.

**It corrupted data in `tee`.** Measured 2026-10-07, against GNU coreutils
9.4:

```
printf 'hello\n' | tee out.txt >&-
  ours: out.txt holds "hello" TWICE, exit 0, nothing said
  GNU:  out.txt holds "hello" once, exit 1,
        tee: 'standard output': Bad file descriptor
```

`out.txt` had opened on descriptor 1, so every block `tee` copied to standard
output was written into the file as well. GNU's `tee` opens with `fopen`
under `stdio--.h`, which makes it `fopen_safer`. Ours does the same now
(`coreutils::stdfd::OpenSafer`, `open_read_safer`, `create_safer`), and
`tee-diff.sh` has seven closed-stdout cases with files to write.

**What upstream does, program by program.** In coreutils 9.4 the safer opens
come from headers a program includes for itself -- `system.h`, which all of
them include, carries none:

| mechanism | programs |
|---|---|
| `stdio--.h`: `fopen` is `fopen_safer`, `freopen` is `freopen_safer` | `fopen`: `comm`, `csplit`, `digest.c` (the `*sum` programs, `cksum`), `join`, `pr`, `tee`. `freopen` only: `dircolors`, `du`, `ptx`, `shuf`, `tsort`, `uniq` |
| `fcntl--.h`: `open` is `open_safer` | `copy.c` (`cp`, `mv`, `install`), `shred`, `split`, `tail` |
| `stdlib--.h`: `mkstemp` is `mkstemp_safer` | `sort`'s temporary files, `temp-stream.c` (`tac`'s) |
| `unistd--.h`: `dup`, `pipe` | `nohup` |
| `openat_safer`, called by name | `ln`'s target directory |
| gnulib modules that include `fcntl--.h` | `fts` (every `-R` walk, `du`, `rm -r`), `randread` (`--random-source`), `savewd`, `save-cwd` |

Everything else opens plainly. So `cat`, `wc`, `head`, `paste`, `sort`'s
inputs and the rest put a file on a closed standard descriptor, and that is
visible: measured, `sort f - <&-` sorts `f` once and exits 0 (its `xfclose`
leaves descriptor 0 open, so `-` finds `f` at its end), and `paste f - <&-`
says `paste: standard input is closed` (it checks for exactly that
descriptor). A first version of this fix made every open in the crate safe,
behind a `clippy.toml` rule, on the belief that gnulib's `fcntl-safer` made
all of coreutils safe; `read-error-diff.sh` and `sort-diff.sh` found these
four differences in it, and it was cut back to `tee` before it was
committed. GNU sed opens plainly too (`R /dev/stdin <&-` reads the input file
on descriptor 0) -- what keeps `sed 'w out' f >&-` from writing into `out` is
the order it closes in, not where it opens. gawk occupies a closed descriptor
with `/dev/null` (`init_fds`) before it opens anything, and ours does the
same, so `awk-diff.sh`'s three closed-stdout cases (added with this) agree
without any change to `awk`.

Several programs, in the table and out of it, do not keep a file *off* 0
and 1 at all but put it there on purpose (grepped, coreutils 9.4):
`freopen` onto standard input in `dircolors`, `du` (`--files0-from`),
`shuf`, `tsort` and `uniq`, and onto standard output in `ptx`, `shuf -o` and
`uniq`'s output operand; `fd_reopen` onto descriptor 0 in `csplit`, `split`,
`stty -F`, `touch` and `dd if=`, and onto 1 in `dd of=` and `nohup`. Each of
those has to be ported as upstream does it, which is not `fd_safer` either.

**Still to do** -- each with a closed-descriptor case in its harness that
fails before and agrees after:

* `fd_reopen` (onto 0 or 1): `stty -F`, `touch`, `dd`. (`nohup`'s was
  already right, and now shares `stdfd::move_to`; `csplit`'s and `split`'s
  inputs were measured and cannot be told apart -- below.)
* `fcntl--.h` (`open_safer`): `cp`, `mv`, `install` (the opens in
  `copy.c`).
* `ln`'s target directory (`openat_safer`).

**The `*sum` programs, `sum` and `cksum`, 2026-10-08.** All built from
`digest.c` upstream, as ours share `coreutils::digest`, whose two opens --
each input, and `--check`'s list -- were plain. With standard input closed
the list became it, and a `-` line in the list read the list itself:

```
md5sum -c SUMS <&-       (SUMS lists a, then -)
  ours: a: OK / -: OK, status 1
  GNU:  a: OK / -: FAILED open or read, then '-: Bad file descriptor'
        and 'WARNING: 1 listed file could not be read', status 1
```

Both are `stdfd::open_read_safer` now. `digest-diff.sh` has seven
closed-standard-input cases for each of the seven programs; three differed
before.

**The `freopen` programs, 2026-10-08.** `uniq`, `shuf`, `tsort`,
`dircolors`, `du --files0-from` and `ptx -G` take an operand with
`freopen (NAME, "r", stdin)` or `freopen (NAME, "w", stdout)`. glibc's
`freopen` opens the file and moves it onto the stream's own descriptor, and
when the open fails it closes that descriptor and reports *the close's*
`errno` -- `Bad file descriptor` if the stream was closed to begin with. Ours
opened the operand as an ordinary file, wherever a descriptor was free.
Measured against GNU 9.4, with each standard descriptor closed in turn:

```
uniq f /nonexistent/x >&-
  ours: uniq: /nonexistent/x: No such file or directory, then
        fatal runtime error: IO Safety violation: owned file
        descriptor already closed (status 134)
  GNU:  uniq: /nonexistent/x: Bad file descriptor (status 1)
shuf -o out -i 1-3 >&-
  ours: shuf: write error: Bad file descriptor, `out` empty
  GNU:  `out` holds the three lines, status 0
tsort nosuch <&-          (and dircolors nosuch, du --files0-from=nosuch)
  ours: ...: No such file or directory
  GNU:  ...: Bad file descriptor
```

`uniq` had put `f` on descriptor 1, then closed descriptor 1 while copying
glibc's error rule, and the standard library aborted over the descriptor it
had lost. `shuf`'s output file was created *on* descriptor 1, and its
`dup2 (1, 1)` followed by dropping the original closed the output again.
Both are now `stdfd::freopen`: open, then `stdfd::move_to` the stream's
descriptor (which keeps the case of an open that landed there already), with
glibc's error rule; each program then reads descriptor 0 or writes descriptor
1, and closes standard input where upstream does. `ptx -G`'s OUTPUT had the
wording difference (`ptx -G t1 /nonexistent/dir/out >&-` said `No such file
or directory`), unseen because no case closed a descriptor around it. The
harnesses of all six hold closed-descriptor cases now; 19 of them differed
before (`shuf` 7, `uniq` 3, `ptx` 3, `tsort`, `dircolors` and `du` 2 each)
and none after.

**`pr`, 2026-10-08.** Upstream's `open_file` is `fopen` under `stdio--.h`.
Ours opened plainly, and `-m` opens every file before it reads any, so with
standard input closed the first file became descriptor 0 and the `-` column
read it as well:

```
pr -m -D x f1 - <&-
  ours: every line of f1 beside an empty `-` column, status 0
  GNU:  the header and "line 1", then
        pr: 'standard input': Bad file descriptor   (status 1)
```

The `-` column came out empty only because `f1` was short enough for its own
column to have buffered all of it first; a longer file would have been split
between the two. `pr-diff.sh` has four closed-standard-input cases with `-`
among other files; three differed before (`stdfd::open_read_safer` now).

**`sort`'s temporary files, 2026-10-08.** `sort` had none until its
external merge was written (`known-issues-resolved/TD-B-SORT-HAS-NO-EXTERNAL-MERGE-RANDOM-SORT-OR-DEBUG.md`);
they were made as upstream's `mkostemp_safer` makes them from the first,
never on descriptor 0, 1 or 2 (`sort/external.rs`, `make_temp_file`).

**`csplit`, 2026-10-08.** Its pieces were plain opens. With standard
error closed a piece became descriptor 2, and a `match not found` said
while the last piece was open went into it; `-k` kept it there:

```
csplit -k marks.txt '/MARK/' '/nomatch/' 2>&-
  ours: xx01 holds MARK b c MARK d, a line each, and then the line
        csplit: '/nomatch/': match not found
  GNU:  xx01 holds MARK b c MARK d, a line each
```

`csplit-diff.sh` holds three such cases and seven more around them (1500
pieces with standard output closed, both descriptors closed at once); the
three differed before the fix. Its *input* is upstream's `fd_reopen` onto
descriptor 0, and ours is a plain open, which cannot be seen: ours reads the
whole input and closes it before the first piece is made.

Already as upstream before this entry: `shred`, `comm`, `join`, `tail` and
`randint` (each called `fd_safer` at its one site), and `tac`'s temporary
file. Since, outside coreutils: `find`'s `-fprint` files (2026-10-08),
which findutils opens through `sharefile_fopen` under `stdio--.h`, and
keeps one stream per file by device and inode -- `find-diff.sh` holds both.

`date -f FILE` calls `fd_safer` where upstream opens plainly -- `date.c`
uses `fopen` and includes no `stdio--.h` (this note used to say it
`freopen`s onto standard input; it does not). Measured 2026-10-08 with every
combination of standard descriptors closed, over four inputs (32 cases), the
two cannot be told apart: the file is opened read-only, so wherever it lands
a write to it fails with the same `EBADF` a closed descriptor gives.

**`split`, 2026-10-08: measured, nothing to convert.** Upstream opens each
piece with `open_safer` and takes its input with `fd_reopen` onto descriptor
0; ours opens both plainly, and agrees with GNU 9.4 with standard input,
output and error closed in every combination -- pieces, diagnostics and
status -- because each piece is announced, written and closed before the
next, so no piece is open when the buffered `creating file` lines reach
descriptor 1. Round robin too: upstream keeps every piece open at once there,
but ours deals the records out in memory first and still writes one piece at
a time, and `split --verbose -n r/900` with standard output closed gave the
same 900 pieces, byte for byte. `split-diff.sh` keeps ten such cases (a `CLOSING`
knob, standard input and output only).

Found while checking whether `cp` had `tee`'s hazard: it did not, by luck of
ordering -- `emit_verbose`'s line is written before each copy opens its files
(GNU's order, `copy.c:2630`), so no file of `cp`'s ever holds descriptor 1
when standard output is written; 300 files of `-v` with standard output
closed came out intact.
