# The shape of a `jobs` line: `[N]M  STATE                   COMMAND`, where M is
# the current/previous-job marker. Every job here is given a lifetime long
# enough that its state at the moment `jobs` runs is not in doubt — "short" jobs
# are already over by the time anything looks, "long" ones are still running —
# so the listing is reproducible.

echo "=== one job is the current job"
sleep 0.7 & jobs
wait

echo "=== the second-newest job is the previous job"
sleep 0.7 & sleep 0.8 & jobs
wait
# The marker is history, not a property of the table as it stands: a job that
# was *running when the next one started* keeps the `-` even after it finishes,
# which is why the finished job below is still marked.
sleep 0.05 & sleep 0.8 & sleep 0.4; jobs
wait

echo "=== …and a job that had already finished by then is not marked at all"
sleep 0.05 & sleep 0.4; sleep 0.7 & jobs
wait

echo "=== only two jobs are ever marked"
sleep 0.7 & sleep 0.8 & sleep 0.9 & jobs
wait

echo "=== a running job's command keeps its \`&\`; a finished one loses it"
sleep 0.05 & sleep 0.7 & sleep 0.4; jobs
wait

echo "=== the state column words a clean exit, a dirty one and a signal apart"
( exit 0 ) & sleep 0.4; jobs
( exit 1 ) & sleep 0.4; jobs
( exit 255 ) & sleep 0.4; jobs
# bash is built with DONT_REPORT_SIGTERM/SIGPIPE and skips SIGINT, so these
# three deaths are not announced asynchronously and are still the listing's to
# report.
sleep 5 & kill -TERM %1; sleep 0.4; jobs
sleep 5 & kill -INT %1; sleep 0.4; jobs
sleep 5 & kill -PIPE %1; sleep 0.4; jobs
wait

echo "=== the command is the source text, whatever shape it had"
sleep 0.7 | cat & jobs
wait
{ sleep 0.7; } & jobs
wait

echo "=== reaping a job does not disturb the others' markers"
sleep 0.05 & sleep 0.7 & sleep 0.8 & wait %1; jobs
wait
# …but reaping a *marked* one does: the previous job falls back to the current
# one when no older job is still running, and both markers go away when nothing
# is running at all.
sleep 0.05 & sleep 0.7 & sleep 0.8 & wait %2; jobs
wait
sleep 0.05 & sleep 0.1 & sleep 0.15 & wait %3; jobs
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
# `-s` selects stopped jobs, of which a script shell has none.
sleep 0.05 & sleep 0.7 & sleep 0.4; jobs -s; jobs
wait
# A jobspec listing forgets only the job it named.
sleep 0.05 & sleep 0.7 & sleep 0.4; jobs %2; jobs
wait

echo "=== an unknown option is rejected with the usage line"
sleep 0.7 & jobs -z; echo "rc=$?"
wait
