## 1071. A program whose original never notices its output was lost reports it, as the coreutils programs do

**Date:** 2026-10-09
**Lane:** B
**Decided by:** Operator (answering B-Q25: option A, which Claude
recommended). Relayed verbatim by lane A from the operator's answers file
(`open-questions/answers.txt` in the integration tree).

**In short:** most of SlateOS's command-line tools are faithful copies of GNU
programs. When one cannot write its output -- the disk is full, or the output
was closed -- the GNU *coreutils* programs (`ls`, `cat`, `sort`...) say
`write error` and fail, and ours do the same. A few programs from other
packages never check: they exit with success and say nothing, and the output
is simply gone. `which`, `ed`, `hostname` and `patch` are four. Ours now
report the lost output in all four, so a script that saves a program's answer
to a file is told when the answer was lost.

**The operator's answer, verbatim:** `B-Q25: A`.

### What each program does now

| program | upstream, output on a full disk or closed | ours |
|---|---|---|
| `which` (GNU which 2.21) | silent, status 0 | `which: write error: No space left on device`, status 1 (as before: its divergence 3) |
| `ed` (GNU ed 1.20.1) | silent, the session's status | `ed: write error: ...`, status 1; a closed output too, now that `ed` sees the descriptors it was given |
| `hostname` (Debian 3.23, all five names) | silent, status 0 | `hostname: write error: ...` under the name it was run by, status 1 |
| `patch` (GNU patch 2.7.6) | silent, status 0 -- and with its output closed it writes its `patching file f` line into `f`, the file it patched | `patch: write error: ...`, status **2**; never writes into `f` |

The message is gnulib's `close_stdout`'s: `<name>: write error: <reason>`
(for `ed` and `hostname`, through `stdfd::Stream`, a bare `write error` when
the failure was an earlier write's and the close had nothing left to fail on,
as glibc and gnulib say it; `patch`, which writes every message at once, names
the first failure's reason).

### The details that were Claude's to settle

- **`patch` exits 2, not 1.** 1 is `patch`'s status for "some hunks failed";
  2 is its trouble, and its own `write error` for an output file it could not
  write is already 2. diffutils, whose programs do register `close_stdout`,
  sets `exit_failure` to 2 for the same reason. Every exit of `patch` but a
  signal's checks (`Ctx::exit`), as every one upstream reaches `exit`.
- **`ed` and `hostname` now see the standard descriptors they were started
  with** (`guard_std_fds!`). Without it the Rust runtime puts `/dev/null` on a
  closed one, and `ed >&-` writes its output nowhere and succeeds. With it,
  two more things follow GNU: a closed *input* is what GNU `ed` takes it
  for, a script file that has ended (its `is_regular_file` counts `fstat`
  failing as regular), so `ed nosuch.txt <&-` exits 2 as from a script,
  where it used to carry on with an empty buffer; and `ed` and `hostname`
  die of a broken pipe as GNU's do, rather than ignoring `SIGPIPE`.
- **A failing standard *error* stays unchecked**, as it is upstream: the
  decision is about output. `ed` used to end with status 1 when a
  diagnostic could not be written -- the coreutils rule -- and no longer
  does; `hostname` and `patch` never did.
- **`ed --help` and `--version`** went through `print!`, which panics on a
  failed write -- and with `panic = "abort"`, aborts. They go through the
  same stream as everything else now.

### The rule for the next port

A program ported from outside coreutils whose upstream never checks its
standard output ends through `stdfd::close_stdout` (or its `_bytes` and
`_with` forms), exiting with the status the program uses for trouble -- 1
where it has no other -- and expands `guard_std_fds!`. Its harness carries
the full-disk and closed-output cases as differences on purpose, citing this
section. Copying a corruption upstream commits (`patch` writing into the file
it patches) is never part of fidelity.

### Where it lives

`userspace/coreutils/src/bin/hostname/main.rs` (`main`);
`userspace/coreutils/src/bin/patch/util.rs` (`STDOUT_FAILURE`, `Ctx::exit`);
`userspace/coreutils/src/bin/ed.rs` (`run_main`, `Editor::read_line`,
`input_failed`); `which.rs` unchanged. The cases: `scripts/hostname-diff.sh`,
`patch-diff.sh` and `ed-diff.sh`, each under "output that cannot be written"
or "standard streams that are full or closed"; `which-diff.sh` already had
them.

### How to reverse

For one program: end it with a plain flush whose result is ignored (and, for
silence on a *closed* output, drop `guard_std_fds!`), and turn its harness's
`xfail` cases into ordinary ones. For all four, option B of the question.
