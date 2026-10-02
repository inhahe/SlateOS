## §97 — osh gives a coproc a command of grace before disposing of it

**Date:** 2026-08-01 (revised the same day — see "Revision" at the end)
**Decided by:** Claude (autonomous)

bash disposes of a coproc when it reaps the body: both parent-side descriptors
are closed and `NAME`/`NAME_PID` are unset. That is now osh's behaviour too
(`Shell::dispose_reaped_coproc`, hung off the job sweep in `poll_jobs`) — with
one deliberate deviation: neither the command that started a coproc nor the
command in which the shell first notices the body is over may dispose of it,
however finished it already looks.

**The problem.** bash's coproc body is a forked child. A fork costs orders of
magnitude more than the builtin the parent runs next, so the child cannot be
reaped before the parent has moved past the `coproc` statement — bash's window
is not guaranteed by anything in the code, but in practice it is hundreds of
fast builtins wide. osh's body is a thread on the same machine, and a body like
`{ exit 7; }` routinely finishes before the shell reaches the next command
boundary. Without a grace period, the commonest idiom of all —

```sh
coproc C { … }
echo feed >&"${C[1]}"
```

— works or does not work depending on how the two were scheduled. Measured: the
same script alternated between `waitC=7` and `wait: `': not a pid or valid job
spec` across runs, and two `interp.rs` unit tests failed intermittently for the
same reason.

**Alternatives considered.**

- *No grace (dispose at the first sweep that finds the body reaped).* The purest
  reading of bash's rule, and what was implemented first. Rejected: it makes
  osh's observable behaviour depend on thread scheduling, which is not a
  property a shell should have. Every test touching a coproc becomes flaky, and
  worse, so does every *script*.
- *Require N consecutive sweeps to find it reaped.* No N is right: the number of
  sweeps between one command and the next is not fixed (see the revision note
  below), so any N buys a probability rather than a guarantee, and a larger N
  also delays disposal for a long-running body whose end bash notices at once.
- *A wall-clock minimum lifetime.* Would model the fork cost most literally, but
  it makes behaviour depend on a timer, which is worse than depending on a
  scheduler, and it is untestable.
- *Spawn the body as a real process.* Would remove the divergence at the root,
  but osh's whole job model is threads (`JobBody::Thread`); changing that is a
  different, much larger decision, and on the SlateOS target processes are not
  cheaper anyway.

**The cost.** A script that puts *two* command boundaries between an
instantaneous body and a use of `NAME` still diverges from bash, which would
usually keep it alive there too. That is accepted: such a script is relying on
an unspecified race in bash as well, and the corpus and unit tests are written
to keep bodies alive with a trailing `read` rather than to bet on the window.

**How to reverse.** Delete `TrackedCoproc::born_item`,
`TrackedCoproc::seen_dead_item`, `Shell::item_seq` and the guards at the top of
`Shell::dispose_reaped_coproc`. The corpus cases
`coproc-is-a-job.sh`, `coproc-read-end-can-be-duped.sh` and
`coproc-is-disposed-of-when-it-is-reaped.sh` already avoid the window, so they
would keep passing; the `coproc_*` unit tests in `interp.rs` likewise.

**Revision — the grace is counted in commands, not in sweeps.** As first
implemented the grace was "the next reap sweep is a no-op" (a `TrackedCoproc::fresh`
flag). That was not enough, and the corpus caught it: `coproc-is-a-job.sh` failed
under the load of a full corpus run with an empty `$C_PID`, because **two**
sweeps happen between one top-level command and the next — `cleanup_dead_jobs`
at the input-unit boundary and `notify_signalled_jobs` at the top of the item —
so the second one could still dispose of a body that had already finished. One
sweep of grace made the race rare instead of removing it, which is the worst of
both: it passed in isolation and failed under load, and it made the timing of
disposal depend on how many sweeps a given construct happened to perform.

The grace is now anchored to a counter of *items* (`Shell::item_seq`, bumped at
the top of each item **after** that item's sweep, so the sweep still belongs to
the item just finished), and two fields on `TrackedCoproc`:

- `born_item` — the counter's value when the `coproc` ran. The item that started
  a coproc never disposes of it.
- `seen_dead_item` — the counter's value at the first sweep that found the body
  over. Disposal waits for a *strictly later* item than that.

The second field is the one that matters, and the first attempt at this revision
lacked it. A grace tied only to birth still lost, because a body is usually
released by the command *after* the `coproc` rather than by the `coproc` itself:

```sh
coproc D { read -r x; }
echo d >&"${D[1]}"     # the body reads, and ends, during this command
wait "$D_PID"          # …and must still be able to name it here
```

Here the body dies during the *second* item, so a birth-anchored grace has
already expired and the sweep at the top of the third can dispose before
`$D_PID` is expanded. That is exactly how `coproc-second-warns-about-the-first.sh`
failed. Requiring the death to be *noticed* in one item and acted upon in a
later one gives every such pair a whole command in which the body may end. bash
gets the same effect for free: between the write that releases a forked body and
the next command there is not enough time for it to exit *and* be reaped.

Both rules are arguments from the code rather than probabilities — no sweep
count, no timer. Verified with ten concurrent runs of `coproc-is-a-job.sh` and
`coproc-second-warns-about-the-first.sh` under four spinning CPU hogs, plus the
full corpus.
