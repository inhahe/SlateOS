### TD-OILS-THREE-TESTS-MEASURED-THE-MACHINE-INSTEAD-OF-THE-SHELL. A job-control sleep, a pipeline start budget and a `times` digit count all failed under a full parallel `cargo test` and passed when re-run alone — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `exec_threaded_pipeline`, the
`jobs_*` tests, and `posix_mode_bare_time_reports_the_shell`.

**What:** four tests failed intermittently in a full sweep. All four were
asserting something about the *host*, not about osh:

* `jobs_reports_a_finished_job_once` and `jobs_words_exits_and_signals_apart`
  started a background job, slept a fixed 100 ms or 700 ms, and asserted it had
  finished. How long a spawn-run-reap cycle takes is the operating system's
  business; `cargo test` runs a thread per core, so a margin that is ample on
  an idle machine is not ample on a busy one.
* `a_pipelines_stages_begin_in_pipeline_order` failed about **two runs in
  five** under an eight-way run. This one was a real defect, not just a fragile
  test: the stage-start handshake gave stage i+1 a single 100 ms budget that
  covered both the OS getting stage i's thread running *and* stage i reaching
  its first command. Only the second is the stage's own doing. Under load
  thread startup alone consumed the budget and `f | :` traced `+ :` first with
  nothing blocked at all.
* `posix_mode_bare_time_reports_the_shell` pinned the output shape down to the
  digit count (`NmN.NNs`) — but those figures are the *test binary's* cumulative
  CPU times, and a parallel run passes ten CPU-seconds long before reaching the
  test, at which point it reports `0m45.20s`.

**Fixed** by `wait_for_bg`, which polls `jobs -r` — the one listing that does
not consume what it filters out — instead of sleeping a guess; by splitting the
pipeline handshake into a thread-entry chain (waited on with no timeout, safe
because a closure that never runs is one whose spawn failed, which drops the
sender) and the existing first-command chain (which keeps its bound, now a
genuine measure of a stage blocked before its first command); and by asserting
the `time` format's one fixed-width field rather than all three.

**Standing lesson:** a test that sleeps a fixed margin, or that pins a width
which grows with elapsed time, is measuring the machine. Both failures look
like a regression in whatever landed just before them. And the middle one is
the reason to chase such a flake rather than paper over it: it was the *test*
that was over-asserting, but only because the mechanism under it was timing two
unrelated things with one clock.
