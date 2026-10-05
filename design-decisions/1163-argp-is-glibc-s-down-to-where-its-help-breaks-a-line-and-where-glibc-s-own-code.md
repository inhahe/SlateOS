## 1163. argp is glibc's, down to where its help breaks a line, and where glibc's own code goes wrong it ends

**Date:** 2026-10-01
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** argp is GNU's command-line parser: a program describes its
options in a table, and `argp_parse` parses the command line, calls the
program's parser for each option and argument, and makes `--help` and
`--usage` out of the same table. elfutils, among others, calls the C
library's. This library had none. It now has glibc 2.39's: the same
structures and constants, the same calls to the program's parsers in the
same order, and help and usage messages that are glibc's byte for byte --
the order of the options, the columns, and where each line breaks. Where
glibc's own code misbehaves -- loops for ever, crashes, or prints what its
buffer happened to hold -- this library's does something sensible instead,
and says which.

**As glibc does** (`posix/tools/oracle/argp_harness.py`: 175 scenarios of
twenty-six parsers, replayed by `posix/src/argp/tests.rs`):

- The parse: the order of `ARGP_KEY_INIT` ... `ARGP_KEY_FINI` across a
  tree of parsers, each child's input from its parent; each argument
  offered as `ARGP_KEY_ARG` then `ARGP_KEY_ARGS` to each parser in turn;
  getopt's messages, `--`, `ARGP_IN_ORDER`, `POSIXLY_CORRECT`; every flag
  (`ARGP_PARSE_ARGV0` taking effect only with `ARGP_NO_ERRS`, as the manual
  says); the built-in `--help`, `--usage`, `--version`, and the hidden
  `--program-name` and `--HANG`; `argp_error`, `argp_failure`,
  `argp_usage` and `argp_state_help`, each under `ARGP_NO_EXIT` and
  `ARGP_NO_ERRS`.
- The help: options sorted by group, by cluster (a child with a header or
  group), and by first letter, glibc's comparisons, which put clusters of
  one group in reverse order and blank lines between groups only once a
  header has been printed; the columns and every `ARGP_HELP_FMT` setting
  and complaint; the help filter, called for each text in glibc's order --
  the duplicate-arguments note only for the root's filter; the usage line,
  its alternatives, and its hand-made breaks.
- Where lines break: glibc's line filler (argp-fmtstream), buffer and all.
  Its buffer is 200 bytes, a formatted write makes room for 150 first, and
  text written out of the buffer cannot be re-broken -- so where a long
  line breaks depends on when the buffer was last written out, and only
  the same buffering gives glibc's answer. It is kept, quirks and all:
  lines that end in blanks, a line of blanks where a margin leaves no room,
  blanks written ahead of the buffer when it is full.

**Where not.**

- *A help whose margins leave no room* -- `ARGP_HELP_FMT=rmargin=20`,
  narrower than the column the option texts begin in, or a right margin of
  0, which `rmargin=abc` gives: glibc's line filler starts its scan before
  its buffer, and loops for ever. Here such a line takes one word.
- *A word that fills a line exactly*: glibc's scan for its end steps past
  it, over the newline (losing the next character) or into whatever its
  buffer last held, which its output then depends on. Here it fits.
- *`argc` 0 and a NULL `argv`*: glibc's reads `argv[0]` and faults; here it
  is a parse of nothing.
- *An `OPTION_DOC` entry in the usage line*: glibc's lists `FILE` as
  `[--FILE]`, though its manual says such an entry "isn't actually an
  option" and is printed with no `--` added. Not listed here -- which is
  glibc's own answer for the same entry marked `OPTION_NO_USAGE`.
- *`ARGP_HELP_FMT` is read at each message*, where glibc's reads it once a
  process; a program that changes it between two messages sees the change.
- *A C++ exception thrown from a parser, a help filter or the version
  hook* ends the program (`std::terminate`), as one thrown from any of
  this library's callbacks does: built to abort, the library has no
  unwind tables for its frames, so the unwinder cannot pass through them.
  glibc builds argp with `-fexceptions`, and there the exception reaches
  a `catch` around `argp_parse`. (Calling them as `C-unwind` would not
  change that: built to abort, such a call gets a landing pad that
  aborts, and the pad names a personality routine that nothing a C
  program links defines. The first build did so, and `services/ctest-argp`
  failed to link; `scripts/check-libc-shape.py`'s CHECK 6 now refuses an
  archive with a member that needs a name nothing supplies.)

**Archive members.** The four variables -- `argp_program_version`,
`argp_program_version_hook`, `argp_program_bug_address`,
`argp_err_exit_status` -- are each a member of its own
(`scripts/check-libc-shape.py` STRICT_FAMILIES): the manual's programs
define `argp_program_version` and `argp_program_bug_address` themselves,
and a member defining one of them beside anything the program needs would
be a second definition. argp parses with
the library's getopt, whose engine is now a member of its own
(`posix/src/getopt.rs`, `mod engine`), so that a program with gnulib's
getopt and this library's argp does not link this library's getopt beside
its own.

**The alternatives.**

- *Help laid out by rules of this library's own*: simpler, and cleaner at
  narrow margins -- but every program's `--help` would differ from glibc's
  in its line breaks, and tests and documentation that quote help text
  would not match.
- *glibc's line filler with its faults too*: byte-for-byte glibc where
  glibc's answer is stable, and a hang or a crash where it is not -- and
  in one case an answer that depends on uninitialised memory, which no
  test can hold.

**Where:** `posix/src/argp.rs`, `posix/src/argp/`; `posix/include/argp.h`;
`posix/src/getopt.rs`'s `engine`; `scripts/check-libc-shape.py`.
