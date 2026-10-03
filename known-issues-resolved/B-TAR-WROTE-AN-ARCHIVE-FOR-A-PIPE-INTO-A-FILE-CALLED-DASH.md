## B-TAR-WROTE-AN-ARCHIVE-FOR-A-PIPE-INTO-A-FILE-CALLED-DASH (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B); boot-confirmed on main at 8be413362 (the boot of 583c2a823 passed).

**In short:** `tar -cf - dir | ssh host tar -xf -` is how a directory is moved
through a pipe, and the `-` in it means "standard output" (or "standard
input"). Ours took `-` for an ordinary file name: it wrote the archive into a
file called `-` in the current directory, sent nothing down the pipe, and
exited 0; the reading side reported `-: Cannot open`. Found while measuring a
different bug -- what `tar` does when its output cannot be written -- which
turned out to be broken in five further ways, all fixed together.

### What was wrong, measured against GNU tar 1.35

| | ours | GNU |
|---|---|---|
| `tar -cf - dir \| wc -c` | `0`, and a 10240-byte file named `-` | `10240` |
| `tar -tf - <a.tar` | `-: Cannot open: No such file or directory`, 2 | the listing, 0 |
| `TAPE=t.tar tar -t` | read standard input | lists `t.tar` |
| `tar -cf a.tar -f b.tar dir` | wrote `b.tar` | `Multiple archive files require '-M' option`, 2 |
| `tar -c dir` on a terminal | binary on the screen | `Refusing to write archive contents to terminal (missing -f option?)`, 2 |
| `tar -cf /dev/null unreadable` | `Cannot open: Permission denied`, 2 | 0 -- the file is never opened |
| `tar -cf /dev/full dir` | `tar: Cannot write: ...`, then `Exiting with failure status due to previous errors` | `tar: /dev/full: Cannot write: ...`, then `Error is not recoverable: exiting now` |
| a disk filling part-way through a record | `Cannot write: No space left on device` | `a.tar: Wrote only 2048 of 10240 bytes` |
| `tar -tf a.tar >/dev/full` | `'standard output': Cannot write: ...` + `Error is not recoverable` | `tar: stdout: write error` |
| `tar -tf a.tar >&-`, `tar -cvf a.tar dir >&-`, `tar -xvf a.tar >&-` | 0, silently | `tar: stdout: write error`, 2 |
| `tar -cf a.tar /abs/path 2>/dev/full` | 0 | 2 |

The last three rows are the stderr/stdout half of
`TD-COREUTILS-AN-UNWRITABLE-STDERR-ABORTS-THE-PROCESS` reaching `tar`: it
had neither `guard_std_fds!` nor a status rule at the end of the run.

### What it does now

Each of these is GNU's mechanism, ported rather than imitated:

- **`resolve_archive`** -- `decode_options`' rule: the last `-f`, else `TAPE`,
  else `-`; and exactly `-` is the standard stream. A second `-f` is refused
  after the whole command line is read.
- **`refuse_terminal`** -- `check_tty`, at the point GNU checks it: after the
  record size, before the archive is opened.
- **`is_dev_null` / `Creator::dumpable`** -- `sys_detect_dev_null_output` and
  `file_dumpable_p`: an archive that is `/dev/null` (by name, or by device and
  inode) is not written, and regular files are not opened for it.
- **`archive_write_failed`** -- `write_error_details`: `Cannot write` when none
  of the record went, `Wrote only N of M bytes` when part did; fatal either
  way. `RecordWriter` now counts what each record delivered.
- **`list_line` / `conclude`** -- the member list is written a line at a time
  through `stdfd::Stream` and a failure is kept, not acted on; at exit, GNU's
  `if (stdlis == stdout) close_stdout (); else if (ferror (stderr)) …` decides
  the status. A reader that leaves stops the run quietly (§377).
- **`stdfd::stdopen`** -- gnulib's, which GNU tar calls first: a closed
  standard descriptor is reopened the wrong way round, which is why GNU's
  `>&-` cases say `write error` with no reason and why `tar -cf - dir >&-`
  succeeds. New in `stdfd`, with `stdfd::metadata`, `stdfd::write_some` and
  `stdfd::write_error_named`.

### Evidence

`scripts/tar-diff.sh` sections 9-12, 76 new cases: the archive on a standard
stream, `TAPE`, repeated `-f`, unwritable output and error streams, `/dev/null`,
a 12 KiB tmpfs that fills mid-record (in a user and mount namespace), and the
terminal refusal under a pseudo-terminal. 323 of 323 agree, 2 known
divergences unchanged. Spot-checked that the full-disk and terminal cases do
reach the failure on both sides (`Wrote only 2048 of 10240 bytes`,
`Refusing to write archive contents to terminal`), so the agreement is not two
programs that never met it. Seven new unit tests.

### Not done here

- GNU treats `host:path` as a remote archive (`rmt`); ours has no remote
  support and opens `host:path` as a local file. Recognised options for it
  (`--force-local`, `--rsh-command`) are refused, as before.
- Extraction onto a full disk: GNU's member writes use the same
  `write_error_details` (`Wrote only`), and ours has not been compared there.
  Section 11 of the harness is the place to add it.
