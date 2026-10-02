## `C-OILS-WAIT-N-TEST-FLAKES-UNDER-LOAD` — FIXED (lane B, 2026-08-20)

**Fixed 2026-08-20 by lane B, as lane C described.** The failing stanza now
calls `settle_jobs(&mut sh)`; the `( sleep 0.1; exit 9 ) &` on the last line
stayed, for the reason given below. The requested sweep found one other
settle-by-sleep in the file — `compgen_job_actions_read_the_job_table` wrote
`true & sleep 0.3; compgen -A running`, whose assertion turns on the shell
having *noticed* the exit — and it was converted the same way. Every remaining
`sleep 0.<n>` in the file is a job body that must still be alive when the next
command runs, i.e. the subject rather than a stand-in. `cargo test -p oils`:
1484 + 64 passed, 0 failed. Reply filed as
`requests/b-c-oils-settle-by-sleep-swept.md`.


**In short:** one test in the shell (`oils`) fails now and then during a full
`cargo test --workspace` run, and passes every time when run on its own. It is
not a real bug in the shell — it is a test that waits a fixed fifth of a second
and assumes a background job has reached a particular state by then. On a busy
machine it has not, and the test reports a failure that means nothing.

**Where:** `userspace/oils/src/interp.rs`, in
`interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported` — the
third stanza, at the line asserting `listing(&mut sh, "jobs").lines().count()
== 1`. Lane **B**'s tree; filed to them as
`requests/c-b-oils-wait-n-test-flakes-under-a-loaded-workspace-run.md`.

**How to confirm:** run `cargo test --workspace`. Observed once as
`left: 0, right: 1`. `cargo test -p oils --lib` alone was green 5/5
(1484 passed each run), and the single test alone green 3/3, so the trigger is
contention from the other test binaries, not the shell.

**The proper fix:** the two stanzas above the failing one call
`settle_jobs(&mut sh)`, which waits out `JOB_EXIT_NOTICE_GRACE` and then polls
`poll_jobs()` until every job has a status. The failing stanza instead writes
`sh.run_source("( exit 7 ) & sleep 0.2")` — a wall-clock guess in place of the
helper written for exactly this hazard. Use `settle_jobs` there too. (The
`( sleep 0.1; exit 9 ) &` on the last line is a different thing and should
stay: there the sleep is the live job under test, not a stand-in for settling.)

**Why it is filed rather than fixed:** `userspace/**` is lane B's. A flaky test
in another lane cannot be cleared by work in this one, and it is not a reason
to hold a green lane back — but an intermittent red that nobody has written
down teaches everyone to ignore workspace failures, which is worse than the
flake.
