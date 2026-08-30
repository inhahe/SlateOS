# When a finished job's row leaves the table. Reporting a job does not remove
# it; the boundary between two units of the *reader* does — which is the loop
# that reads, parses and executes one logical line at a time, so an `eval` or a
# sourced file has one of its own and a function body, being a single parsed
# unit, has none inside it.
#
# Every job here is over long before anything looks at it, so only the sweep is
# in question and never the timing — but "long before" has to hold at *both*
# ends. A job that exits instantly can be reaped before bash has even built its
# table row, and `stop_pipeline` (`jobs.c`) then computes
#
#     newjob->state = any_running ? JRUNNING : (any_stopped ? JSTOPPED : JDEAD);
#
# from the child's already-recorded status and calls `reset_current ()` right
# after. Born JDEAD, the job leaves that call with no running job to find, so
# `j_current` and `j_previous` both become `NO_JOB` and the row prints with no
# `+` at all. The row is still listed either way — JDEAD is not deleted — so the
# only thing that race changes is the marker.
#
# Each job is therefore `sleep 0.05` rather than `true` — guaranteed to still be
# alive while its own row is being built, since that happens microseconds after
# the fork. The foreground gap is then `sleep 0.5` rather than the 0.2 s that
# suffices on an idle machine: what has to be beyond doubt at the far end is
# that the job is *finished*, and both halves of that comparison cost a process
# spawn, which on a loaded machine can itself run to a large fraction of a
# second. 0.05 against 0.5 leaves ~450 ms of slack in the direction that matters.
# TIMEOUT: 60

echo "== the row outlives the listing that reported it, within the line"
sleep 0.05 & sleep 0.5; jobs; jobs %1; echo "rc=$?"
echo "== …but not the line itself"
sleep 0.05 & sleep 0.5
jobs
jobs %1
echo "rc=$?"

echo "== the same for a wait that read the status"
sleep 0.05 & sleep 0.5; wait %1; echo "1=$?"; wait %1; echo "2=$?"; wait %1; echo "3=$?"
sleep 0.05 & sleep 0.5
wait %1; echo "1=$?"
wait %1; echo "2=$?"

echo "== an eval body sweeps between its own lines"
sleep 0.05 & sleep 0.5
eval 'jobs
jobs %1; echo "rc=$?"'

echo "== …and reaching the end of one is not a boundary"
# On one line, so that the only boundary in question is the eval's own end.
sleep 0.05 & sleep 0.5; eval 'jobs'; jobs %1; echo "rc=$?"

echo "== a function body is one unit, so nothing is swept inside it"
sleep 0.05 & sleep 0.5
g() {
  jobs
  jobs %1; echo "rc=$?"
}
g

echo "== a job nobody reported survives the boundary"
sleep 0.05 & sleep 0.5
:
jobs
echo "rc=$?"

echo "== a swept job's status still answers to its pid"
( sleep 0.05; exit 5 ) & p=$!
sleep 0.5
jobs
wait $p; echo "rc=$?"
wait $p; echo "again=$?"
echo "== but an argument-less wait forgets it"
wait
# The diagnostic carries the pid, which no two runs agree on, so only the
# status is checked here; the wording is pinned by a unit test.
wait $p 2>/dev/null; echo "rc=$?"
