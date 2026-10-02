### TD-OILS-AN-EXTERNAL-CHILD-CANNOT-BE-GIVEN-A-CLOSED-FD-0 — 2026-08-06 — ✅ FIXED 2026-08-06 (all three descriptors)

**Where:** `userspace/oils/src/interp.rs` — `child_input` / `ChildIn::Closed`,
and `HeadIn::Closed`, both of which fall back to `Stdio::null()`.

**What.** bash hands a child a genuinely closed descriptor, so the child's own
read fails; osh gives it an empty one, so the child sees a clean EOF:

```text
$ bash -c '( exec 0<&-; cat; echo rc=$? ) < /dev/null'
cat: failed to set file descriptor text/binary mode: Bad file descriptor
rc=1
$ osh -c '( exec 0<&-; cat; echo rc=$? ) < /dev/null'
rc=0
```

Only *external* commands diverge — every builtin (`read`, `mapfile`, …) reports
the read error correctly, because it never leaves the shell.

**Proper fix.** `std::process::Stdio` has no closed form, so this needs a raw
`CreateProcess` with `hStdInput = NULL` (and a `close`-before-`exec` on POSIX).
That is the same trade already recorded for the write side in TD-OILS-FD0-WRITE;
if either is ever done properly, do both together.

**Impact.** A child that would have failed on a closed stdin instead succeeds
reading nothing. Rare, and the shell's own status is the only thing that differs.

**Fixed 2026-08-06** — and *without* the raw `CreateProcess` the entry assumed
was needed, which is the interesting part. Reimplementing the spawn would have
meant reimplementing Windows argument quoting, the environment block, the
working directory and handle inheritance, and putting all of that on the path
every external command in the shell takes; a bug in the quoting alone would have
been far worse than the divergence being fixed.

The observation that makes it a small change instead: on Windows a child's
standard handles are the three `STARTUPINFO` fields, and `Stdio::inherit()`
fills them from the *parent's own slots* — passing a null handle straight
through when the parent has none, which the library does deliberately
(`sys::pal::windows::process`: "If no stdio handle is available, then propagate
the null value to the child"). So emptying the shell's own slot with
`SetStdHandle` for the length of the `CreateProcess` call, and putting it back
afterwards, hands the child exactly the closed descriptor bash's fork gives it.
Measured before writing any of it, with a twenty-line probe: an MSYS `cat`
spawned that way prints `cat: failed to set file descriptor text/binary mode:
Bad file descriptor` and exits 1, which is bash's output character for
character.

The pieces, all in `interp.rs`:

* `ClosedStd { stdin, stdout, stderr }` — which of a child's standard
  descriptors are handed over closed. `ChildIn::Closed` and `HeadIn::Closed`
  now produce `ClosedStd::STDIN` where they used to produce `Stdio::null()`.
* `spawn_with_closed_std`, which every production spawn funnels through
  (`spawn_resolved`'s two callers and the `&`-job shortcut, which used to call
  `PCommand::spawn` directly). It forces `Stdio::inherit()` on the descriptors
  being closed — the mechanism works *through* inheritance, so whatever the
  caller put there is what must not be used.
* `NulledStdHandles`, an RAII guard around the `GetStdHandle`/`SetStdHandle`
  pair, so the slot is put back on the failing spawn as well as the succeeding
  one. `close_std_before_exec` is the unix half: a `pre_exec` closure dropping
  an `OwnedFd::from_raw_fd(0)` in the forked child, which is what bash's own
  child does.
* `SPAWN_STD_HANDLES`, an `RwLock<()>` every spawn takes — shared for an
  ordinary one, exclusive for a closing one. The slots are process-global and
  osh's pipeline stages and subshells are *threads* of one process, so without
  it a stage arranging a closed fd 0 could hand a sibling stage's child, in
  `CreateProcess` at that moment, the same closed descriptor. Ordinary spawns
  share the read side and never wait on each other.

The shell's own reads through the slot are not in the window, and by
construction rather than by luck: a thread can only read the ambient fd 0 when
its stdin is inherited, and a thread only reaches a closing spawn having
redirected fd 0 away, so the two are never the same thread — and no two
concurrent threads inherit fd 0 together, since a pipeline gives it to the head
stage alone and a `&` job gets an empty stream instead of it.

**Verified:** the last section of `tests/corpus/redirect-close-of-stdin.sh` —
the persistent and per-command closes, both spellings of the operator, the
reopen orderings, a pipeline head, a child that never reads, and the shell's own
fd 0 still working after a closing spawn. Plus
`a_closing_spawn_gives_the_shells_own_standard_handle_back`, which pins the
restore on both the succeeding and the failing spawn.

**fd 1 followed the same day**, as the second use of `ClosedStd`. The shell used
to refuse `cmd >&-` outright (`1: Bad file descriptor`, status 1) rather than let
the child's write fail; it now sets `closed.stdout` and resolves no destination
at all, and the child says what bash's child says:

```text
$ cat in >&-      # both shells
cat: standard output: Bad file descriptor      → 1
```

Three sites feed it: `RedirPlan::stdout_closed` (`cmd >&-`), a scoped
`exec 1>&-` seen through `exec_stdout_shadowing`, and a persistent one seen
through `exec_stdout`. `capturing` is switched off alongside, because a closed
fd 1 collects nothing — `x=$(cmd >&-)` is an empty capture whatever the command
writes, and setting up the pipe-and-drain would be arranging a destination for
a descriptor that is not there.

That half needed one thing the read side did not: the shell's *own* fd 1 and
fd 2 have to wait the window out. `io::stdout()` on Windows re-reads
`STD_OUTPUT_HANDLE` for every write and answers a nulled slot with `BrokenPipe`,
and unlike fd 0 — which a `&` job never has — fd 1 is exactly what a `&` job
inherits, so `{ echo one & }; cmd >&-` really can put a write and a closing
spawn at the same moment. The four ambient write sites and `dup_std_handle` now
take `std_handles_shared()`, the read side of the same `RwLock`; a closing spawn
is rare and short, so nothing waits in practice.

Verified by the last section of `tests/corpus/redirect-close-of-stdout.sh`:
per-command and persistent closes, both spellings, the reopen orderings, a
pipeline *tail* stage with no fd 1, `sleep` (which never writes and is
untroubled) beside `cat /dev/null` (which is not, because it closes fd 1 at
exit), an empty capture, and the shell's own fd 1 still working afterwards.

**fd 2 completed the set**, in the commit after fd 1's. It needed the wider
change the handoff here predicted: a new `StderrTarget::Closed` beside
`Discard`, because a closed fd 2 and a read-only one are indistinguishable
*inside* the shell — both drop what is written and have no second stderr to
report on — and differ only in what leaves it:

```text
$ sh -c 'echo E >&2' 2>&-              # both shells now
                                       → 1   (the child's own write failed)
$ sh -c 'echo E >&2' 2>/dev/null       → 0   (an empty stream takes it)
```

`Discard` remains what a fd 2 with no *write half* maps to (`2>&0` onto a
read-only fd 0 — the descriptor is there, so the child gets one); `Closed` is
what `2>&-`, `exec 2>&-` and a `WriteFd::Closed` target map to. The two sites
that pushed `StderrTarget::Discard` for a close now push `Closed`, and the four
places that only care "can this be written to?" —
`stderr_has_no_write_half`, `emit_stderr_depth`, and the external command's
`exec_stderr` and stack arms — match on `Discard | Closed` or on `Closed`
alone as the question requires.

One of them is not a write at all. `child_stdio_for_stderr` is `1>&2`'s path as
well as fd 2's own, and a dup of a descriptor that is not there fails when the
redirect is *set up*, not when the child writes — so `StderrTarget::Closed`
answers it with `Err("2: Bad file descriptor")`, and
`{ sh -c 'echo O' 1>&2; } 2>&-` prints nothing and exits 1 as bash's does. The
diagnostic is itself invisible, having only the closed fd 2 to be reported on.

**Verified** by the new "the child's own write to a missing fd 2 fails" section
of `tests/corpus/redirect-close-of-stderr.sh`: the per-command and persistent
closes, a group, a child that keeps its own non-zero status through the failed
write, a child that never touches fd 2 and is untroubled, and the `1>&2` dup.
