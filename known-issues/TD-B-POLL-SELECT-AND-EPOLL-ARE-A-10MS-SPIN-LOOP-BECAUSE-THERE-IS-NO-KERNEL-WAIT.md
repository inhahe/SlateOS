## TD-B-POLL-SELECT-AND-EPOLL-ARE-A-10MS-SPIN-LOOP-BECAUSE-THERE-IS-NO-KERNEL-WAIT — 2026-09-05

**In short:** `poll()`, `select()` and `epoll_wait()` are the calls a program
uses to say "wake me when any of these is ready." Ours do not wait at all: they
check every file descriptor in turn, sleep 10 ms, and check again, until
something is ready or the timeout runs out. The program still gets the right
answer, so nothing is visibly broken — it just arrives up to 10 ms late, and the
process wakes 100 times a second while doing nothing. Every event-driven program
on SlateOS pays this, which today means every server we have.

**Where it is:** `posix/src/poll.rs` — `poll()` at the `POLL_INTERVAL_NS`
constant (10 ms), `select()`'s identical loop below it, and
`posix/src/epoll.rs`, whose `epoll_wait` walks its interest list through the
same per-fd readiness checks. The module doc has always said so plainly
("we can't do kernel-level event waiting yet"); it was never tracked here.

**Why it is this way.** Readiness is per-object: a TCP socket answers
`SYS_TCP_POLL_STATUS`, a pipe answers its own status call, a pty answers
`SYS_PTY_POLL`. There is no syscall that takes a *set* of kernel objects and
blocks until one of them is ready. Without that, "wait for any of these" can
only be built as "ask each of these, repeatedly" — which is what this is.

**What the proper fix is.** A kernel-side multi-object wait: hand it a list of
handles and an event mask, it blocks the thread on all of them at once and
returns which fired. `posix`'s three interfaces then become thin wrappers over
one real wait, and the interval constants are deleted rather than tuned. This is
lane A's to build (it is a scheduler/wait-queue feature, not a libc one).

**Consequences today, in ascending order of who notices:**

- Any program calling `poll` with a timeout of `-1` gets a 10 ms granularity
  instead of an immediate wake-up.
- Idle event loops cost 100 wake-ups/second each, so the cost scales with the
  number of *idle* connections, which is the wrong direction.
- It is why sshd's session loop does **not** route through `poll` and runs its
  own 0.5 ms–20 ms backoff over the two readiness syscalls directly — see
  design-decisions.md §770. A correct kernel wait would let that loop, and the
  hand-rolled loops in every other daemon, collapse into one blocking call.

**If never fixed:** everything keeps working and everything stays slightly
late and slightly wasteful, with each new daemon either paying the 10 ms or
hand-rolling its own tighter loop — which is how a single missing primitive
becomes N incompatible workarounds. Nothing is blocked; the cost is diffuse,
permanent, and grows with the number of programs.
