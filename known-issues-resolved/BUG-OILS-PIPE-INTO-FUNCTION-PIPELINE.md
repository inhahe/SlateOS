### BUG-OILS-PIPE-INTO-FUNCTION-PIPELINE. A nested pipeline ignored both ends of the pipeline it was nested in — 2026-07-27 — ✅ RESOLVED

**Symptom.** osh hung forever (bash completed instantly):
```sh
h() { od -An -tx1 | tr -s ' '; }
printf 'ab' | h          # osh: never returned; `od` blocked on read
```
`printf 'ab' | f` with `f() { od -An -tx1; }` (a *single* command in the body)
worked, and so did `printf 'ab' | cat`. It was specifically a body containing a
**pipeline**. The brace-group and subshell forms (`printf ab | { od | tr; }`,
`printf ab | ( od | tr )`) failed identically, and the output half of the same
bug was silent rather than hanging:
```sh
inner() { cat | cat; }
printf 'ab\n' | inner | tr a-z A-Z    # bash: AB; osh: `ab`, leaked to the terminal
```

**Root cause.** Not fd leakage — plumbing that was never connected. Both
pipeline executors in `userspace/oils/src/interp.rs` hard-coded the ends of the
pipeline they built:

* `exec_threaded_pipeline` / `exec_concurrent_pipeline` never received the
  ambient `stdin: &StdinSrc`; the head stage got `StdinSrc::Inherit` /
  `Stdio::inherit()`. A *nested* pipeline's head therefore read the **script's**
  stdin — silently empty when that was at EOF, a permanent hang when it was a
  terminal (the reported symptom).
* `exec_concurrent_pipeline` chose the last stage's stdout from a bare
  `capturing` bool: `Out::Capture` → a pipe, *everything else* → inherit. When
  the enclosing `Out` was `Out::Pipe` (this pipeline was itself a non-final
  stage of an outer one), the tail wrote past the outer pipe to the real stdout.

**Fix.** `exec_pipeline` now threads `stdin` into both executors. A new
`Shell::pipeline_head_stdin` derives an **owned, `Send`** head input —
`Option<io::PipeReader>`, `None` meaning inherit — because the head stage runs
on a spawned thread and no borrowing form of `StdinSrc` is `Send` (`Cursor`
holds a `&RefCell`, which is not `Sync` at *any* lifetime, `'static` included).
A `Cursor`'s remaining bytes are pushed down a fresh OS pipe by a short-lived
writer thread — a thread, not an inline `write_all`, so a payload larger than
the pipe buffer cannot deadlock against a reader that has not started. A `Pipe`
head clones the read end so the stage streams rather than buffering (an
unbounded upstream like `yes` must not be drained). `exec_concurrent_pipeline`
now derives the tail's `Stdio` from all three `Out` variants, duplicating the
write end for `Out::Pipe`.

**Regression test.** `userspace/oils/tests/corpus/pipeline-nesting.sh` — every
nesting shape (function / brace group / subshell, two deep), both directions
(nested head reads the outer pipe, nested tail writes to it), both executors
(all-external and threaded), the here-string/here-doc `Cursor` input path, early
termination through a nesting boundary (`yes ab | t` with `t() { head -c 2 |
cat; }` — that one hangs on regression, so run the suite under
`scripts/run-timeout.py`), and `$?` propagation from the nested tail.
