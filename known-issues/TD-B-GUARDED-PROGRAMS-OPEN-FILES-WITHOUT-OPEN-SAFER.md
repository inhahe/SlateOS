## TD-B-GUARDED-PROGRAMS-OPEN-FILES-WITHOUT-OPEN-SAFER (lane B, 2026-10-07)

**Status:** PARTLY FIXED 2026-10-08 (lane B): `tee`, the program whose
difference corrupted data, opens its files as its upstream does now.
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
| `stdio--.h`: `fopen` is `fopen_safer` | `comm`, `csplit`, `digest.c` (the `*sum` programs, `cksum`), `dircolors`, `du`, `join`, `pr`, `ptx`, `shuf`, `tee`, `tsort`, `uniq` |
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

* `stdio--.h` (`fopen_safer`): the `*sum` programs and `cksum` (their
  inputs), `pr` (its inputs), `csplit` (its output files), `ptx` (its
  `-b`/`-i`/`-o` word files).
* `freopen`/`fd_reopen` (onto 0 or 1): `dircolors`, `du --files0-from`,
  `shuf`, `tsort`, `uniq`, `ptx`'s output, `csplit`'s and `split`'s input,
  `stty -F`, `touch`, `dd`, `nohup`.
* `fcntl--.h` (`open_safer`): `cp`, `mv`, `install` (the opens in
  `copy.c`), `split`'s output files.
* `stdlib--.h`: `sort`'s temporary files.
* `ln`'s target directory (`openat_safer`).

Already as upstream before this entry: `shred`, `comm`, `join`, `tail` and
`randint` (each called `fd_safer` at its one site), and `tac`'s temporary
file. Since, outside coreutils: `find`'s `-fprint` files (2026-10-08),
which findutils opens through `sharefile_fopen` under `stdio--.h`, and
keeps one stream per file by device and inode -- `find-diff.sh` holds both. `date` calls `fd_safer` too, where upstream `freopen`s onto standard
input -- to be measured with the rest.

Found while checking whether `cp` had `tee`'s hazard: it did not, by luck of
ordering -- `emit_verbose`'s line is written before each copy opens its files
(GNU's order, `copy.c:2630`), so no file of `cp`'s ever holds descriptor 1
when standard output is written; 300 files of `-v` with standard output
closed came out intact.
