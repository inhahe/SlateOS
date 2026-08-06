# The shape of a `jobs` line: `[N]M  STATE                   COMMAND`, where M is
# the current/previous-job marker. Every job here is given a lifetime long
# enough that its state at the moment `jobs` runs is not in doubt — "short" jobs
# are already over by the time anything looks, "long" ones are still running —
# so the listing is reproducible.
#
# The marker sections need a *second* fact to be beyond doubt: whether a job was
# still alive at the moment the **next** one was spawned, which is what earns it
# the `-`. A lifetime shorter than a process spawn decides that by race, so the
# short job in every section that reads a `-` off a *finished* job is 0.6s rather
# than 0.05s — comfortably longer than a spawn even on a loaded machine, and
# still comfortably shorter than the 1.0s the foreground `sleep` then waits
# before `jobs` looks. (bash decides it the same way and is racy in the same
# place: `set_current_job` in `jobs.c` falls back to `job_last_running`, which
# skips a job already reaped, so a job that died before the next one spawned
# earns no marker in either shell.)
#
# Those deliberate lifetimes add up, and each `sleep` costs a process spawn on
# top, so the case runs long enough to trip the harness's 20s default when the
# machine is busy. It states a budget of its own instead.
# TIMEOUT: 60

echo "=== one job is the current job"
sleep 0.7 & jobs
wait

echo "=== the second-newest job is the previous job"
sleep 0.7 & sleep 0.8 & jobs
wait
# The marker is history, not a property of the table as it stands: a job that
# was *running when the next one started* keeps the `-` even after it finishes,
# which is why the finished job below is still marked.
sleep 0.6 & sleep 2 & sleep 1.0; jobs
wait

echo "=== …and a job that had already finished by then is not marked at all"
sleep 0.05 & sleep 0.4; sleep 0.7 & jobs
wait

echo "=== only two jobs are ever marked"
sleep 0.7 & sleep 0.8 & sleep 0.9 & jobs
wait

echo "=== a running job's command keeps its \`&\`; a finished one loses it"
sleep 0.6 & sleep 2 & sleep 1.0; jobs
wait

echo "=== the state column words a clean exit, a dirty one and a signal apart"
( exit 0 ) & sleep 0.4; jobs
( exit 1 ) & sleep 0.4; jobs
( exit 255 ) & sleep 0.4; jobs
# bash is built with DONT_REPORT_SIGTERM/SIGPIPE, so these two deaths are not
# announced asynchronously and are still the listing's to report. `Interrupt`
# cannot be shown alongside them: an asynchronous job is handed SIGINT already
# ignored, so no background job ever reaches that state. (See
# `kill-dispositions.sh`; the job is given a moment to settle first, because the
# ignore is established after the fork.)
sleep 5 & sleep 0.1; kill -TERM %1; sleep 0.4; jobs
sleep 5 & sleep 0.1; kill -PIPE %1; sleep 0.4; jobs
wait

echo "=== the command is the source text, whatever shape it had"
sleep 0.7 | cat & jobs
wait
{ sleep 0.7; } & jobs
wait

echo "=== reaping a job does not disturb the others' markers"
# These want the opposite fact from the section above — job 1 already *dead*
# when job 2 is spawned, so it is never marked — and it is the same race, so it
# gets the same treatment: an explicit gap rather than a lifetime that merely
# tends to be shorter than a process spawn.
sleep 0.05 & sleep 0.4; sleep 0.7 & sleep 1.5 & wait %1; jobs
wait
# …but reaping a *marked* one does: the previous job falls back to the current
# one when no older job is still running, and both markers go away when nothing
# is running at all.
sleep 0.05 & sleep 0.4; sleep 0.7 & sleep 1.5 & wait %2; jobs
wait
sleep 0.05 & sleep 0.4; sleep 0.05 & sleep 0.4; sleep 0.05 & wait %3; jobs
wait

echo "=== %+, %% and %- name exactly those two jobs"
sleep 0.7 & sleep 0.8 & jobs %+; jobs %%; jobs %-
wait
sleep 0.7 & sleep 0.8 & sleep 0.9 & jobs %1 %3
wait
sleep 0.7 & jobs %9; echo "rc=$?"
wait

echo "=== a finished job is reported once, then forgotten"
sleep 0.05 & sleep 0.4; jobs; jobs; echo "rc=$?"
# Numbering restarts at 1 once the table has drained.
sleep 0.7 & jobs
wait

echo "=== …but only if the listing actually reported it"
# `-r` filters the finished job out, so the next `jobs` still has it to report.
sleep 0.05 & sleep 0.4; jobs -r; jobs
wait
# `-s` selects stopped jobs, of which a script shell has none. These two read a
# `-` off job 1, so they take the long-short lifetimes the header describes:
# job 1 must outlive job 2's spawn to earn the marker, and still be over by the
# time `jobs` looks.
sleep 0.6 & sleep 2 & sleep 1.0; jobs -s; jobs
wait
# A jobspec listing forgets only the job it named.
sleep 0.6 & sleep 2 & sleep 1.0; jobs %2; jobs
wait

echo "=== an unknown option is rejected with the usage line"
sleep 0.7 & jobs -z; echo "rc=$?"
wait
