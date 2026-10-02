### TD-OILS13. `osh` job control lacks terminal job control (no job *stop*/resume, no controlling-tty transfer); async `&` for all command forms now works — PARTIALLY RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`exec_background`, `builtin_jobs`,
`builtin_wait`, `builtin_fg`, `builtin_bg`, `builtin_disown`, the `Job` struct /
`JobBody` enum and `Shell::jobs` table).

**What:** background-job tracking (`&` → job table, `jobs`, `wait`, `wait -n`,
`$!`, `disown`, `fg`/`bg`) is implemented. Item (1) below is now **resolved**;
the parts that require a controlling terminal and job-control signals remain
incomplete:
1. ~~Only a single external simple command backgrounds asynchronously.~~
   **RESOLVED 2026-07-19:** every backgrounded form that is *not* a single
   external process — a builtin (`true &`), function, compound command
   (`{ …; } &`, `( … ) &`), pipeline (`a | b &`), negated command, or
   multi-command and-or list — now runs asynchronously on a dedicated OS thread
   executing a subshell clone, registered in the job table via the new
   `JobBody::Thread` variant with a synthetic pid (`SYNTH_PID`, ≥ 900_000). So
   `$!`, `jobs`, `wait`/`wait -n`/`wait $!`, and `disown` all see these jobs, and
   the backgrounded body genuinely runs concurrently (bash's "run `&` in a
   subshell", using a thread rather than a fork). A backgrounded thread job's
   stdin is disconnected (fed an empty cursor → immediate EOF), matching bash's
   redirect of async stdin from `/dev/null` in a non-interactive shell.
   *Two residual narrow limitations of the thread model:* (a) a thread-backed job
   **cannot be `kill`ed** — Rust threads are not cancellable, so `kill %n` on such
   a job detaches the handle and records the signalled status but the thread runs
   to completion; (b) a background job nested inside a **redirected/piped** context
   (`cmd | { job & wait; }`) does not inherit that ambient stdin (it sees EOF),
   because the live pipe reader is not shareable across the thread boundary — this
   pre-dates the fix (the old synchronous fallback passed `Inherit`, equally wrong)
   and is rare. Proper fix for (b): a thread-safe shareable stdin reader.
2. **No job *stop*/resume and no controlling-tty transfer.** `fg`/`bg` are
   implemented as far as is meaningful without terminal job control: `fg` prints
   the job's command line and *waits* for it (it cannot resume a stopped job or
   move the terminal foreground process group), and `bg` is a spec-resolving
   form. There is no process-group / controlling-terminal machinery, so jobs
   cannot be *stopped* (Ctrl-Z / `SIGTSTP`) — the "Stopped" state never occurs —
   and `fg` cannot grant a job the terminal. **Diagnostics now match bash's
   mode-dependent behavior (2026-07-20):** `fg`/`bg` are gated on job control,
   which (as in bash) is on for interactive shells and off for non-interactive
   `-c`/script shells, with `set -m` / `set -o monitor` toggling it explicitly
   (osh tracks this via the `Shell::monitor` flag and `job_control_enabled()`).
   When job control is off, `fg`/`bg` print `fg: no job control` / `bg: no job
   control` on stderr and return 1, exactly like bash — even when a background
   job is running. When it is on and a target resolves, because osh never has a
   *stopped* job (every tracked job is already running), `bg` matches bash's
   already-running case: `bg: job N already in background` on stderr, exit 0
   (previously osh printed a non-bash `[id] cmd &` line on stdout).

3. **`%name` job specs match the job's whole command text, not each of its
   processes.** `lookup_job` (2026-07-27) implements bash's spec grammar —
   `%N` only when the remainder is all digits, `%`/`%%`/`%+`/`%-` reading just
   the first character (so `%+x` is the current job), `%str` a prefix match,
   `%?str` a substring match, and more than one match an `ambiguous job spec`
   error rather than a pick. But bash matches a spec against *every process* of
   a pipeline job, so `sleep 5 | cat &` answers to `%cat`; osh stores only the
   job's source text (`Job::cmd`), so it answers only to `%sleep`. Proper fix:
   record the per-process command words for a pipeline job and match each.
4. **No asynchronous job-death notification.** bash announces a job killed by a
   signal other than INT/TERM/PIPE (which its `DONT_REPORT_*` build options
   suppress) on stderr as `bash: line N: PID Hangup   sleep 5`, before the next
   prompt or command; osh reports such a death only when `jobs` next runs. Ties
   into TD-OILS11 (async signal delivery).
5. **A job's death is learned synchronously, so a job osh has just signalled is
   one it already knows is over.** bash reaps on `SIGCHLD`, so for a moment
   after `kill %1` the job is still `Running` as far as the shell is concerned;
   osh's `kill` reaps the child itself and knows at once. Two visible
   consequences, both verified to be *timing*-dependent in bash (inserting a
   `sleep 0.1` makes bash agree with osh), which is why neither is in the
   corpus: (a) `sleep 0.3 & kill %1; jobs` lists `Running` under bash and
   `Terminated` under osh; (b) after `sleep 0.5 & kill %sleep; wait`, bash
   numbers the next job `[1]` — its `wait` blocked on a job it did not yet know
   was dead, and so discarded it — where osh, seeing a job that had already
   finished, would keep the row and number the next one `[2]`. osh handles (b)
   by recording per job whether the shell has *learned* of the exit
   (`Job::exit_seen`): a job that only `kill` reaped is one an operand-less
   `wait` still counts as waited for, which restores bash's numbering. (a) has
   no such workaround and stands. Ties into the same asynchronous-notification
   gap as item 4.

**Note — `jobs -x` answers with a pid, not a process group.** bash substitutes
`job->pgrp`; osh has no process groups, so a job is its own process and the pid
is what stands in. Deliberate — see design-decisions.md §89, which also explains
why `tests/corpus/jobs-execute.sh` covers everything about `-x` except the
substituted value (a unit test pins that instead).

**Note — reference bash's `jobs` ambiguity answer is not stable, so it is not
in the corpus.** For `sleep 0.7 & sleep 0.8 & jobs %sleep`, bash 5.2.37 prints
either `ambiguous job spec` followed by `%sleep: no such job` with status 1, or
the ambiguity alone with status 0 — decided by whether `PS1`/`PS2`/`LC_ALL` are
present in the environment, which this path never consults (`LANG`, `PATH` and
arbitrary new variables make no difference). It looks like an out-of-bounds read
of bash's job array with its internal `DUP_JOB` sentinel (-2). `disown`, `kill`
and `wait` are stable, and are what `tests/corpus/jobs-spec.sh` exercises; osh
follows bash's plain-environment answer (both messages, status 1), which is also
what bash's own `disown` does either way.

**Proper fix (remaining):** once the kernel provides process groups +
job-control signals (ties into TD-OILS11 async-signal delivery), extend `fg`/
`bg` to genuine stop/continue + terminal-foreground transfer and add the
"Stopped" job state. The current implementation is correct for the
overwhelmingly common cases (`cmd & … wait`, `fg` to wait on a background job).
