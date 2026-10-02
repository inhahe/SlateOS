## §90 — A background job's signal disposition is modelled on bash's *intent*, not on what the reference shell happens to print

**Date:** 2026-07-27
**Decided by:** Claude (autonomous)

**Decision.** osh gives a job started with `&` the dispositions bash's
`setup_async_signals` gives an asynchronous child: `SIGINT` and `SIGQUIT` are
ignored (job control is off, so an interrupt meant for the foreground must not
reach into the background), and every other signal keeps its default — so
`CHLD`, `URG`, `WINCH` and `CONT` are no-ops too, and only a signal that would
really terminate kills the job. Before this, osh killed a job for *any* nonzero
signal. A consequence worth stating: **no background job can ever be listed as
`Interrupt` or `Quit`.**

**Why this needed deciding.** The reference bash appears to contradict this. Run
directly, `sleep 1 & kill -INT %1` reports `Interrupt` — and the corpus case
`jobs-listing.sh` had been asserting exactly that. Two hypotheses were
investigated and *both* were disproven by probing:

* *"Cygwin drops `SIG_IGN` for `SIGINT` across `exec`."* Disproven directly:
  `(trap "" INT; exec sleep 1) & kill -INT %1` leaves the job running, so the
  ignore does survive `exec` on this host.
* *"The job's leader decides — a wrapped job (`( )`, `{ }`, a pipeline) has a
  shell in front of it that ignores the signal, a bare command does not."* This
  fitted a 16-case shape×signal matrix exactly — and was still wrong. Re-running
  the same file showed bash answering `Interrupt` for the `( )` shape on one run
  and `Running` on the next.

The actual explanation is a **race**: bash establishes an asynchronous child's
dispositions *after* forking it, so a signal delivered immediately can land
while the child still holds the disposition it was forked with. Given 0.05 s to
settle, the reference shell is unambiguous — 30 trials: `INT` 10/10 running,
`QUIT` 10/10 running, `TERM` 10/10 terminated.

* **Alternative A — implement the settled behaviour (chosen).** Costs the
  corpus its only `Interrupt` row, since that state is now unreachable.
* **Alternative B — keep matching what the reference prints when signalled
  immediately.** Would have kept the row, at the price of encoding a race as if
  it were a rule — and the row would have been a latent flake all along, since
  bash decides it by timing.

Chosen A. **The general lesson, which cost three wrong models here: when the
reference shell is used as an oracle, a disagreement that is *reproducible* is
not thereby *deterministic*.** Before modelling any behaviour that involves a
freshly forked child, re-run the probe several times and give the child time to
settle; a single confident-looking answer can be one side of a race. Every case
in `tests/corpus/kill-dispositions.sh` therefore sleeps before signalling, and
says in a comment why.
