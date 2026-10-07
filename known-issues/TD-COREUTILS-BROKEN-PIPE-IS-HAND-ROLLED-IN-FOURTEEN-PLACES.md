## TD-COREUTILS-BROKEN-PIPE-IS-HAND-ROLLED-IN-FOURTEEN-PLACES (lane B, 2026-08-24)

**Status:** FIXED 2026-10-07 by lane B, pending a boot test on `main` before
the move to `known-issues-resolved/`. Every site that should ask the shared
predicate now does, and the predicate itself changed meaning with
design-decisions §1060.

**In short:** when a program writes into a pipe whose reader has left, GNU's
programs are ended by a signal (`SIGPIPE`), saying nothing. Ours could not be,
so each of fourteen programs had its own code to notice the failed write and
stay quiet. That code disagreed in small ways. It now lives in one place,
`coreutils::stdfd::reader_gone`. Since 2026-10-07 the signal is put back
(§1060), so a converted program is ended by it just as GNU's is, and the
shared code matters only where that could not happen.

**What it was.** A write that fails because the reader went away was the one
write failure a utility must *not* report. GNU dies of `SIGPIPE` there, prints
nothing and exits 141. SlateOS sent no signal to die of, so the same situation
arrived as `EPIPE` (design-decisions §377). Fourteen binaries each worked that
out separately, three of them with the same paragraph of comment copied
verbatim.

**What was done.**
- The predicate is `stdfd::reader_gone`, and `stdfd::close_stdout` is the
  shared tail that applies it. The fourteen sites were converted to it during
  the closed-standard-descriptor sweep, the last five (`yes`, `uniq`, `sed`,
  `tail`, `stat`) on 2026-10-07.
- §1060 then made `stdfd::restore` put `SIGPIPE` back to the disposition the
  program inherited. `reader_gone` is now true only where that did not happen:
  - a program not yet given `guard_std_fds!` and `restore` (see
    `TD-COREUTILS-AN-UNWRITABLE-STDERR-ABORTS-THE-PROCESS`);
  - the Windows development host.

  Everywhere else a broken pipe ends the program as it ends GNU's, and an
  inherited "ignore" makes it the write error GNU reports.

**The direct `ErrorKind::BrokenPipe` checks that remain are deliberate.** Each
models an upstream that tests `errno == EPIPE` itself:

| Where | Why it is not `reader_gone` |
|---|---|
| `tee.rs`, the `--output-error` classification | the modes are *about* telling `EPIPE` apart from other failures (`fail_output`) |
| `split.rs`, the write into a `--filter` command | upstream's `ignorable (errno)`: a command that stops reading is not a failure |
| `ps/main.rs`, `Ps::flush` | procps catches `SIGPIPE` and exits 0, whatever it inherited |
| `kill.rs`, `kill -l` | left alone while open question B-Q22 is decided |
| `sh.rs` | the shell's own model of a closed descriptor, not a utility's output |
| `head.rs`, after `emit` | a *read* failing with `EPIPE`, not a write |

**The `uniq`-versus-`cut` disagreement this entry reported was not one.**
`uniq`'s `write_failure` returns status 0, which is the status a run has
earned at that point: upstream reports a read error only after its output is
done. `uniq badfile good | head -1` cannot reach the write at all, since
`badfile` fails to open first and the run exits 1.
