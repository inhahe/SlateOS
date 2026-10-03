### TD-OILS22. `osh` process substitution `<(cmd)`/`>(cmd)` uses a temp-file model, not streaming FIFOs — OPEN (gated on `/dev/fd` or named-pipe support)

**Where:** `userspace/oils/src/interp.rs` (`Shell::proc_sub`, `finish_procsubs`,
the `procsub_in_temps`/`procsub_out_jobs` fields, the `exec_simple` wrapper), plus
`lexer.rs` (`Seg::ProcSub`, `read_word`), `parser.rs` (`WordPart::ProcSub`),
`ast.rs`, `unparse.rs`.

**What:** process substitution is implemented, but because the host (and the
current SlateOS target) exposes neither `/dev/fd/N` nor named pipes (FIFOs), the
substituted pathname is backed by an ordinary temp file rather than a pipe wired
to a concurrently-running process (this is the same fallback several shells use
on systems without `/dev/fd`). Consequences:
- **Not streaming.** For `<(cmd)` the command runs to completion *at expansion
  time* and its entire output is buffered into the temp file before the enclosing
  command runs. An unbounded/interactive producer — `<(tail -f log)`,
  `<(yes)` — blocks at expansion instead of streaming, whereas real bash would
  run it concurrently and let the reader consume incrementally.
- **`>(cmd)` is deferred, not concurrent.** The enclosing command writes to a
  temp file; only *after* it finishes does `cmd` run with that file as stdin. So
  the two processes never overlap in time (bash runs them concurrently). Final
  results match for the common finite cases (`tee >(wc -l)` counts correctly),
  but ordering/interleaving and any back-pressure semantics differ.
- **A pid/`/dev/fd` path is not observable.** The substituted word is a real
  temp path (e.g. `/tmp/osh_psub_<pid>_<n>.tmp`), not `/dev/fd/63`; a script that
  parses the substituted filename expecting `/dev/fd/…` would see something else.

Temp files are created lazily during word expansion and cleaned up when the
enclosing command finishes: `exec_simple` records a mark into
`procsub_in_temps`/`procsub_out_jobs` before running the command and calls
`finish_procsubs` after (running deferred `>(cmd)` bodies, then deleting all temp
files). `exec_redirected` does the same for a *compound* command's redirect
target, so `{ …; } > >(cmd)`, `( … ) > >(cmd)`, and `for/while/if/case … > >(cmd)`
now correctly run the deferred output body and clean up (**fixed 2026-07-20**;
regression test `process_sub_output_deferred_on_compound`; previously these
produced *no* output because `finish_procsubs` was never called for compound
commands). Still not swept: process substitutions created while expanding a
non-redirect, non-simple-command *word* context (a `for`-list operand `for x in
<(…)`, a `case` word, a `[[ … ]]` operand) — those leak their temp file for the
shell's lifetime (input procsubs there still *function*, since the file is
written at expansion; only cleanup leaks). A minor, rarely-hit gap.

**Proper fix:** once SlateOS exposes named pipes (a `mkfifo`/FIFO VFS node) or a
`/dev/fd/N` mechanism, replace the temp file with a real pipe: for `<(cmd)` spawn
`cmd` writing the pipe's write end and substitute the read-end path; for `>(cmd)`
spawn `cmd` reading the pipe and substitute the write-end path — so both run
concurrently with the enclosing command and stream. The lexer/parser/AST plumbing
(`Seg::ProcSub` → `WordPart::ProcSub`) stays; only `proc_sub`/`finish_procsubs`
change. Also sweep the non-simple-command contexts (or move cleanup to a
shell-level teardown) to avoid the temp-file leak.
