### [A] Operational, for all three lanes: stopping a backgrounded shell script does not stop the script -- 2026-09-17

**Status:** OPEN

**In short:** if you background a shell script that runs long jobs and then
stop it, the tool reports success and the job keeps running. Start a
replacement and you now have two, racing each other in the same worktree.
This cost a boot cycle today, and the first symptom looked like a flaky test.

What happened: `TaskStop` returned `Successfully stopped task`, the harness
stopped tracking the job, and the script's descendants ran to completion
anyway. The replacement run overlapped it. Both wrote the same two log
files, which is how it eventually became visible -- a log that is truncated
at every start (`: > "$L"`) contained **two** `BOOT done` lines, and the
boot log two `BOOT_REAL_EXIT=1` lines.

The damage was not subtle once understood: two concurrent `boot-test.sh`
pre-flights ran the git-heavy `scripts/test-*.py` suites against the same
worktree and failed *each other*. `test-checkers-honour-head.py` and
`test-selftests-are-repo-safe.py` both reported failures that do not
reproduce standalone. I read the first as a flake.

**The fix is the tool the project already has.** Run anything long under
`scripts/run-timeout.py`, which puts the child in a Windows Job Object with
`KILL_ON_JOB_CLOSE` (POSIX: a process group it SIGKILLs), so a stop tears
down the whole tree, grandchildren included. `CLAUDE.md` says this about
orphans and coreutils `timeout`; it is equally true of a stopped background
task. Pick the bound from the whole run, not the inner step: a release boot
on 2026-09-17 took 4417s end to end, of which only 581s was QEMU.

**A second, smaller one from the same hour.** I also concluded a healthy run
had died, from a two-minute silence in its log plus `ps | grep -c cargo`
returning 0 -- during the release build, which is legitimately silent for
~10 minutes. It wrote again after I stopped it. A quiet log is not a dead
process, and on this project the quietest phase is also among the longest.
