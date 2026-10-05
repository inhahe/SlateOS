### TD-OILS-KILL-ANNOUNCE-TIMING. A killed job is announced one command boundary sooner in osh than in bash, because osh's `kill` records the death instead of hearing about it — OPEN 2026-07-27

**Where:** `userspace/oils/src/interp.rs` — `Shell::notify_signalled_jobs` (the
announcement), `Shell::kill_one` (which writes `status`/`signal` down itself),
and the `Job::exit_seen` field that already names the distinction.

**What.** bash announces a background job killed by a signal other than
INT/TERM/PIPE at the first command boundary *after it has heard* the death, and
it hears asynchronously, a moment later. osh's `kill` has no signal to send — it
kills the child (or, for a thread-backed job, cannot) and writes the signalled
status into the job on the spot — so the shell knows immediately and announces
at the very next boundary. Two observable divergences, both requiring the
inspection to follow the kill with no delay at all:

```
$ bash -c 'sleep 5 & sleep 0.1; kill -HUP %1; echo A; echo B'   # nothing
$ osh  -c 'sleep 5 & sleep 0.1; kill -HUP %1; echo A; echo B'   # announces before A
$ bash -c 'sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.5'        # nothing (no later boundary)
$ osh  -c 'sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.5'        # announces before the sleep
```

**Proper fix.** None available on this platform, which is why it is logged rather
than fixed. It would require `kill` to *not* record the outcome and let a later
`poll_jobs` discover it — but on Windows a killed child's exit code carries no
trace of the signal (`Child::kill` yields plain exit 1), so the synthetic record
is the only way `jobs` can word `Terminated` at all. Faking the lag by deferring
the announcement one boundary would be a different wrong answer (bash's lag is
wall-clock, not one command) and would disturb `exit_seen`, which `wait` uses to
decide whether a job counts as one it waited for. Same root as TD-OILS13.

**Impact.** Confined to the no-delay case. Every corpus case settles for 0.1 s
after a kill before looking (`job-death-notice.sh`, `kill-dispositions.sh`), which
is required for reference bash to be reproducible anyway — bash establishes an
asynchronous child's dispositions after forking it — so the two shells agree
throughout the suite. A settle delay is not sufficient on its own, though: see
TD-OILS-CORPUS-JOB-DEATH-LEFTOVER below.
