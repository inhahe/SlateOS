### TD-OILS-BG-SINK-OUTLIVES-SUBSHELL. A job started inside a `( )` subshell stops holding the enclosing capture open when the subshell ends — RESOLVED 2026-07-28

**Resolved 2026-07-28** by the proper fix named below: the capture sink is now
refcounted like an fd, so *who still holds a write end* is a fact the sink can
answer rather than something the caller has to infer from a job table.

`Out::Capture` and `StderrTarget::Buffer` now carry a `CaptureSink` — an `Arc<{
buf, writers: Mutex<usize>, idle: Condvar }>` plus a `claimed` flag. `share()`
(what a `&` job and a pipeline stage get) increments `writers` and returns a
claimed handle whose `Drop` decrements it and notifies `idle` at zero; `clone()`
returns an *unclaimed* alias, for the shapes that write to the sink without
holding it open. `finish_comsub` waits with `cap.wait_for_writers()` instead of
`sub.drain_jobs()`, which is what makes the nesting depth irrelevant: the count
does not care which `Shell`'s job table the writer was forked from, and it drops
to zero when the last writer's handle does.

Whether a job gets a claimed handle is decided by `job_holds_sink`, which walks
the async node's *own* redirect list over a small fd set (seeded `{1}`, or
`{1,2}` when `stderr_aliases_capture` reports fd 2 is bound to the same sink) and
answers "is any of them still the sink?". That is the same accounting bash gets
from fd inheritance: `sleep 2 > /dev/null &` drops fd 1 and holds nothing;
`sleep 2 2>&1 > /dev/null &` made its dup *before* the overwrite and still holds
one; a job under a scope that already rebound fd 1 (`{ sleep 2 & } > /dev/null`,
`exec > f`) never held it, which `exec_stdout_shadowing` detects. Anything more
complex than a single simple-or-redirected command conservatively holds.

Covered by `tests/corpus/background-capture-sink.sh`, which records the waited/
immediate outcome and the collected output for 19 shapes.

**Residual — closed 2026-07-28.** A writer that reaches the sink through a
descriptor ≥ 3 dup'd from fd 1 (`x=$( exec 3>&1; ( echo held >&3 & ) > /dev/null )`
— bash `[held]`, osh `[]` with `held` on the terminal) was still missed, but not
by anything this fix introduced: fd 3 never named the capture in the first place,
because `open_write_fds` mapped fd→`Arc<File>` and a capture has no OS fd. That
was TD-OILS-COMPOUND-SCRATCHFD-PIPE, and it showed up identically without a `&`
(`x=$( exec 3>&1; echo plain >&3 )`). Resolving it — `open_write_fds` now holds a
`WriteFd`, which can be a `CaptureSink` — made such an fd name the sink, and an
outliving clone upgrades every inherited entry to a *counted* handle via
`Shell::share_write_fds()`, so this shape now waits and collects like bash. See
TD-OILS-COMPOUND-SCRATCHFD-PIPE.

**Where:** `userspace/oils/src/interp.rs` — `Out::share()` / `drain_jobs()` /
`command_sub`, and the `Command::Subshell` arm of `exec_command`.

**What.** `x=$( ( echo n & ) ); echo "[$x]"` substitutes `n` in bash and nothing
in osh.

A background job holds a copy of the collecting pipe's write end, so bash's
`$( … )` reader does not see EOF until the job exits — *however deeply nested*
the job's starting point was. osh reproduces this for a job started directly in
the substitution (`command_sub` calls `drain_jobs()` before taking the capture)
and for any pipe sink (an `io::PipeWriter` clone gives the OS refcount for free),
but not for a job started inside a nested `( )` subshell whose sink is a
**capture**: the subshell's `Shell` clone has its own job table, which is dropped
when the subshell ends, detaching the thread. Its writes then land in a buffer
nobody reads.

**Proper fix.** Refcount the capture sink the way an fd is refcounted. Today
`Out::Capture(Arc<Mutex<Vec<u8>>>)` cannot express "someone else still holds a
write end", because `Arc`'s drop is not observable. Replacing the payload with a
`CaptureSink` newtype (`Arc<{ buf: Mutex<Vec<u8>>, writers: Mutex<usize>, idle:
Condvar }>`) whose `share()` claims a writer and whose guard releases it on drop
lets `command_sub` wait for "no writers but me" — which handles the nested case
and also *correctly declines* to wait when the inner subshell redirected fd 1
away (`x=$( ( sleep 2 & ) > /dev/null )` returns immediately in bash). ~40
`Out::Capture(…)` construction sites would need mechanical updating, most of them
in tests.

A cheaper "drain the subshell's jobs if its `out` is a capture" patch was
considered and rejected: it over-waits in exactly that redirected-away case,
because the redirect is applied as an `exec_stdout` override rather than by
rebinding `out`.

**Impact.** Narrow: a `&` job started inside a `( )` **and** inside a command
substitution **and** writing to the substitution's stdout. Output is lost, not
corrupted.
