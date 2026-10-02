### [E] The option-table gate read a slow WSL as no WSL, and let a push through unjudged -- 2026-09-25 -- FIXED 2026-09-25
**Status:** FIXED 2026-09-25 -- `scripts/getopt-ambiguity-check.py`.
**Superseded 2026-09-26:** lane E's fix (below) and lane A's `54e1c8743`
("gate 5: a GNU side that cannot be reached is a loud skip (exit 3), never a
pass and never a finding", handed over by lane F) changed the same probe at the
same time, and met in lane E's merge of `main`. Lane A's version was kept
whole: it covers this failure and three more (a WSL that hangs mid-sweep, one
that fails a call after the probe passed, and output without proof the GNU
side ran), it gives the startup probe 90 s, and it reports every "could not
reach GNU" as exit 3, which the hook -- changed in the same commit -- lists
as *skipped* rather than *ran*. That also closes the "still open" note at the
end of this entry. The one behaviour that differs from lane E's: a WSL that
times out is now a loud skip that lets the push through, where lane E's
refused it (exit 2). That is lane A's and lane F's call for the shared hook,
made with its test suite in view, and lane E's text below is kept as the
record of how the failure was found.

**In short:** the pre-push hook's gate 5 compares each coreutils option table
with GNU's through WSL. Its probe gave WSL 30 seconds to answer, and on a busy
machine a cold WSL can take longer; the checker then printed "no GNU userland
available; nothing to check", exited 0, and the hook counted the gate as run
and allowed the push. So a table broken on a pushed branch was published
unjudged whenever the host happened to be loaded.

**How it was found.** A lane-E boot test refused to build because
`test-checkers-honour-head.py` failed its gate-5 case "a branch other than
HEAD is still judged" (allowed, where refused was expected). Run alone with
WSL healthy in every shell, it failed again; replayed with the hook's output
printed, the checker was saying "no GNU userland" inside the hook while the
test's own probe, moments earlier, had found one. The host was running
several builds at once.

**The fix.** The probe now waits 30, then 60, then 120 seconds before giving
up. A WSL that answers each probe with an error (none installed, no
distribution) is still the documented skip; one that *times out* every time
is `RunnerUnavailable`, reported as "WSL is installed but did not answer" with
exit 2 -- the hook's no-verdict, which refuses the push rather than publishing
tables nobody compared. The retry is the usual remedy; `ALLOW_GETOPT_DRIFT=1`
remains the loud bypass.

**Still open, and noted rather than done:** the checker reports the real skip
(no GNU userland) with exit 0, so the hook's tally lists gate 5 under "ran"
when it compared nothing. `scripts/run-checker.sh` defines exit 3 for exactly
that ("I could not run, and here is why"); moving this checker to it needs the
hook's gate-5 loop to read `RUN_CHECKER_SKIPPED`, which is a change to the
shared hook best made with its test suite in view.
