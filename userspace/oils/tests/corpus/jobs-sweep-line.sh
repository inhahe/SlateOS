# When a finished job's row leaves the table. Reporting a job does not remove
# it; the boundary between two units of the *reader* does — which is the loop
# that reads, parses and executes one logical line at a time, so an `eval` or a
# sourced file has one of its own and a function body, being a single parsed
# unit, has none inside it.
#
# Every job here is `true`, over long before anything looks at it, so only the
# sweep is in question and never the timing.
# TIMEOUT: 60

echo "== the row outlives the listing that reported it, within the line"
true & sleep 0.2; jobs; jobs %1; echo "rc=$?"
echo "== …but not the line itself"
true & sleep 0.2
jobs
jobs %1
echo "rc=$?"

echo "== the same for a wait that read the status"
true & sleep 0.2; wait %1; echo "1=$?"; wait %1; echo "2=$?"; wait %1; echo "3=$?"
true & sleep 0.2
wait %1; echo "1=$?"
wait %1; echo "2=$?"

echo "== an eval body sweeps between its own lines"
true & sleep 0.2
eval 'jobs
jobs %1; echo "rc=$?"'

echo "== …and reaching the end of one is not a boundary"
# On one line, so that the only boundary in question is the eval's own end.
true & sleep 0.2; eval 'jobs'; jobs %1; echo "rc=$?"

echo "== a function body is one unit, so nothing is swept inside it"
true & sleep 0.2
g() {
  jobs
  jobs %1; echo "rc=$?"
}
g

echo "== a job nobody reported survives the boundary"
true & sleep 0.2
:
jobs
echo "rc=$?"

echo "== a swept job's status still answers to its pid"
( exit 5 ) & p=$!
sleep 0.2
jobs
wait $p; echo "rc=$?"
wait $p; echo "again=$?"
echo "== but an argument-less wait forgets it"
wait
# The diagnostic carries the pid, which no two runs agree on, so only the
# status is checked here; the wording is pinned by a unit test.
wait $p 2>/dev/null; echo "rc=$?"
