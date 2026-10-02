### TD-OILS-BG-STDIN-REDIR. A `&` job does not inherit an *explicitly redirected* fd 0 — it always reads /dev/null — RESOLVED 2026-07-28

**Where:** `userspace/oils/src/interp.rs` — `exec_background`'s thread path
(`StdinSrc::Cursor(&empty)`) and its direct-spawn shortcut
(`cmd.stdin(Stdio::null())`).

**What.** bash gives an asynchronous command `/dev/null` for fd 0 **only in the
absence of an explicit redirection**. Its `stdin_redir` flag (execute_cmd.c) is
set both by a redirection of fd 0 in the *currently active* redirect list and by
`do_piping` when the command is a pipeline stage — and when it is set, the job
inherits fd 0 as it stands. So all of these read real input in bash and produce
nothing in osh:

```sh
echo A | { cat & wait; }          # bash: A     osh: (nothing)
{ cat & wait; } < two.txt         # bash: l1 l2 osh: (nothing)
echo A | ( cat & wait )           # bash: A     osh: (nothing)
x=$(echo Z | { cat & wait; })     # bash: Z     osh: (empty)
exec < two.txt; cat 0<&0 & wait   # bash: l1 l2 osh: (nothing)
```

The flag is genuinely per-redirect-list, not "fd 0 is not a terminal": `exec <
two.txt; cat & wait` reads /dev/null in bash too, because `exec`'s redirection is
not part of the async command's list. osh already matches every case whose answer
is /dev/null, including that one — see the "fd 0 goes the other way" section of
`tests/corpus/background-output.sh`. What is missing is the inherit case.

**Why it is not fixed yet.** `StdinSrc` is a *borrowed* enum
(`Cursor(&'a RefCell<io::Cursor<Vec<u8>>>)`, `Pipe(RefCell<BufReader<PipeReader>>)`),
so it cannot be moved into the job's thread, which outlives the call. Handing a
job the caller's fd 0 therefore needs `StdinSrc`'s payloads to be shareable and
`Send` — an `Arc<Mutex<…>>` cursor and an `Arc`-wrapped pipe reader — which is
the shape `Out` already has. That is the proper fix: make `StdinSrc` own `Arc`
handles, give it a `share()` alongside `Out::share()`, and have `exec_background`
pass `stdin.share()` when the command is a pipeline stage or carries an fd-0
redirect and `/dev/null` otherwise. The sharing is also what gives bash's
shared-file-offset semantics between shell and job for free.

**Impact.** Narrow: a `&` job that reads its enclosing pipeline's or group's
input. Nothing in the corpus depends on it, and the failure mode is empty input
rather than corruption.

**Fix (2026-07-28).** Both halves, as described above.

1. **`StdinSrc` now owns `Arc` handles and is `Send`.** Its lifetime parameter is
   gone; `Cursor` holds a `ByteCursor` = `Arc<Mutex<io::Cursor<Vec<u8>>>>` (with
   `byte_cursor`/`lock_cursor`/`snapshot_cursor` helpers) and `share()` sits
   alongside `Out::share()` — `Arc::clone` for a cursor, `try_clone` for a pipe.
   `Shell::exec_stdin` and `Shell::open_fds` hold `ByteCursor`s too, so a
   subshell clone still takes a *snapshot* (`snapshot_cursor`) while a job takes
   a *share*. Sharing the cursor is what gives bash's shared file offset between
   shell and job: `{ read v & wait; read w; } < four.txt` leaves `w=r2`.
2. **`Shell::stdin_redirected` models bash's `stdin_redir`.** `exec_redirected`
   — the compound-command redirect site, and the only one — *assigns* it from
   `redirects_rebind_stdin(list)`, so `{ …; } < f` sets it and a nested
   `{ …; } > f` clears it again. The function-invocation / `eval` / `.` sites do
   not assign it, matching bash: `f < file` leaves the flag alone even though fd
   0 really does change. `exec_threaded_pipeline` sets it for every stage with an
   upstream pipe, *after* the stage node's own list is applied — that ordering is
   what `Shell::pipe_stage_redirect_root` (a one-shot ORed in by the stage's own
   `exec_redirected`) exists for, so `echo A | { cat & wait; } 2>/dev/null` still
   reads the pipe while `echo A | { { cat & wait; } > f; }` does not.
   `clone_for_subshell` copies the flag. `exec_background` then decides with
   `job_inherits_stdin(ao, self.stdin_redirected)`, which also consults the async
   node's *own* redirect list (for `cat 0<&0 &`, a dup that changes only the
   flag), and passes `stdin.share()` when it inherits.

Covered by `tests/corpus/background-stdin.sh` (new; the superseded "fd 0 goes
the other way" section of `background-output.sh` was removed in favour of it).

**Residual 1 — the flag does not leak past its scope.** bash keeps one global
that a redirect scope never restores, so its flag survives into a *later command
of the same top-level command*: with fd 0 bound by `exec < two.txt`,
`{ true; } 0<&0; cat & wait` prints `l1 l2` in bash and nothing in osh (the same
two on separate lines print nothing in both). osh saves and restores the flag
with the scope instead. Reproducing the leak faithfully would need the reset to
be per *parsed top-level unit* — which for bash is per input **line**, since its
reader parses a line at a time — and osh parses the whole script up front, so
there is no line boundary left to hang it on. Deliberate: the scoped rule is the
defensible one, and the divergence needs a redirect scope that changes nothing
(`0<&0`) followed by a job on the same line.

**Residual 2 — an external command still drains the cursor — ✅ CLOSED
2026-07-28.** `run_external` read a `Cursor`-backed fd 0 to the end to feed the
child, so `{ head -n 1 & wait; head -n 1; } < four.txt` printed `l1` where bash
prints `l1 l2`: the job's `head` consumed the whole snapshot rather than one
line. The builtin path (`read`) was already exact. Fixed by the named proper fix
— modelling `< file` as a real shared `File` handle rather than a byte snapshot
— under TD-OILS-PIPE-HEAD-CURSOR-DRAIN, which shares this root cause. The child
now receives a `try_clone` of the descriptor itself, so it consumes exactly what
it reads.

**Note on the type names above.** `Fix (2026-07-28)` item 1 describes the shape
as it was when this entry was written. That refactor was superseded the same day:
`ByteCursor` is now `InputFd = Arc<Mutex<InputSrc>>` where `InputSrc` is either a
byte snapshot or a real open `File`, `StdinSrc::Cursor` is `StdinSrc::Fd`,
`byte_cursor`/`lock_cursor` are `bytes_input`/`lock_input`, and `snapshot_cursor`
is gone — a subshell clone now *shares* rather than snapshotting, because a
subshell is a `fork`. See TD-OILS-PIPE-HEAD-CURSOR-DRAIN for the current model.
