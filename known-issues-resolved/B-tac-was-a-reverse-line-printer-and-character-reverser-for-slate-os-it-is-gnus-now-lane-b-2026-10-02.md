## `tac` was a "reverse line printer and character reverser for Slate OS"; it is GNU's now (lane B, 2026-10-02) — **FIXED** 2026-10-02

**In short:** `userspace/tac` was hand-written: no `-b`, `-r` or `-s`, options
of its own that no `tac` has, and its arguments read as `String`, so a file
name that was not UTF-8 killed it before its first statement. It is replaced by
`coreutils/src/bin/tac.rs`, a port of GNU coreutils 9.4's `tac.c` function by
function, and the standalone crate is deleted (§1005: coreutils is the one
home). `scripts/tac-diff.sh`: 98 cases, none differing from GNU's (two more,
`--help` and `--version`, differ on purpose).

**What the port had to reproduce, because it is observable:**

- `-s ''` without `-r` separates on NUL (upstream compares the empty string's
  terminator); with `-r` it is "separator cannot be empty".
- `-r` reads glibc's Emacs syntax with the newline anchor, and searches
  *backwards* within the part of the read buffer not yet printed -- so `^` and
  `$` also hold where that window begins and ends. The port reads the file the
  same way (8 KiB, doubling for long records, the size carried from file to
  file) so those windows fall where GNU's do. `ere` gained the backward search
  for it (`Search::rsearch`, glibc's `re_search` with a negative range).
- Output goes through upstream's own 8 KiB buffer before standard output, so a
  diagnostic about a later file comes out *before* earlier files' output, and
  a full device fails an earlier write (a bare "write error").
- Standard input that is a file is seeked to its end once per `-`, so
  `tac - - <f` prints it twice; a pipe is copied to an unlinked temporary file
  named as gnulib's `temp_stream` names it, in `$TMPDIR` only if that exists.

**Found on the way, and fixed in the engine:** `.` and bracket expressions
matched a byte that is not UTF-8, where glibc's never do --
`B-ERE-DOT-TOOK-A-BYTE-GLIBC-LEAVES`.
