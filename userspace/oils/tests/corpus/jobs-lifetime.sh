# A finished job is not forgotten the moment it is reported. It lingers until
# the next thing that sweeps the job table, and that delay is visible: a job can
# still be named after the listing that announced it. Nothing here turns on
# timing — every `sleep` either finishes long before it is looked at or outlives
# the whole line that starts it. `disown -a` between lines empties the table, so
# each line starts from the same state.

echo "=== reporting a job by name loses it at once, in a bare listing it lingers"
# The jobspec form lists first and sweeps after, so there is nothing left for
# the `kill` to find…
sleep 0.05 & sleep 0.3; jobs %1; kill %1; echo "rc=$?"; disown -a
# …while the bare form sweeps first, so the job it reported outlives it.
sleep 0.05 & sleep 0.3; jobs; kill %1; echo "rc=$?"; disown -a
# Which is why a bare listing never announces the same job twice, and naming it
# twice fails the second time.
sleep 0.05 & sleep 0.3; jobs; jobs; echo "listed once"; disown -a
sleep 0.05 & sleep 0.3; jobs %1; jobs %1; echo "rc=$?"; disown -a

echo "=== naming a job in kill puts it back on the list of fates still owed"
# Even for signal 0, which delivers nothing.
sleep 0.05 & sleep 0.3; jobs; kill -0 %1; jobs; echo "listed twice"; disown -a
# `disown -h`, which also leaves the job in place, does not.
sleep 0.05 & sleep 0.3; jobs; disown -h %1; jobs; echo "listed once"; disown -a

echo "=== so does waiting on it — but only from the second wait onwards"
# The first `wait` reports the status; the second reports it again and is the
# one whose sweep drops the row; only the third has nothing to name.
{ sleep 0.2; exit 7; } & wait %1; echo "a=$?"; wait %1; echo "b=$?"; wait %1; echo "c=$?"
disown -a
# A pid outlives the row it named, because the status is remembered separately.
{ sleep 0.2; exit 7; } & wait $!; echo "a=$?"; wait $!; echo "b=$?"; wait $!; echo "c=$?"
disown -a
# A waited-for job is missing from a bare listing yet still nameable, so asking
# for it by name works right up until the bare listing sweeps it away.
sleep 0.2 & wait %1; jobs %1; echo "rc=$?"; jobs; echo "nothing left"; disown -a

echo "=== an operand-less wait discards every finished job but the one \$! names"
# Job 1 had already finished, so the `wait` did not wait for it — and it is the
# last job backgrounded, so it survives to be announced.
sleep 0.05 & sleep 0.3; wait; jobs; disown -a
sleep 0.05 & sleep 0.3; wait; wait; jobs %1; echo "rc=$?"; disown -a
# With a second job to be last, job 1 loses that protection.
sleep 0.05 & sleep 0.4 & sleep 0.2; wait; jobs; echo "nothing left"; disown -a
# A job the `wait` really waited for is discarded however it ended.
sleep 0.3 & wait; jobs %1; echo "rc=$?"; disown -a

echo "=== numbering restarts once the table is swept, not when a job merely ends"
sleep 0.2 & wait %1; sleep 0.4 & jobs; wait; disown -a
sleep 0.05 & sleep 0.3; sleep 0.4 & jobs; wait; disown -a

echo "=== losing a marked job costs both markers, losing an unmarked one neither"
# Dropping the `-` job leaves the `+` job in place, yet the listing that follows
# shows no marker at all: with nothing running there is nothing to re-mark.
sleep 0.05 & sleep 0.06 & sleep 0.3; jobs %1; echo "--"; jobs; disown -a
# With something still running, that running job takes both markers.
#
# These two lines are the only ones in the file that need a *timed* gate to land
# strictly between two live jobs: the `sleep 0.3` has to outlast `sleep 0.05` and
# be outlasted by the first job. 0.5 s left only ~0.2 s of margin on the second
# half, and three process spawns on a loaded machine can eat that — the job then
# reads `Done` instead of `Running`. 1.2 s buys ~0.9 s of margin on both sides at
# the cost of ~0.7 s of wall clock each (the trailing `wait` pays it).
sleep 1.2 & sleep 0.05 & sleep 0.3; jobs %2; echo "--"; jobs; wait; disown -a
# Naming the running job reports it without dropping it, so nothing moves.
sleep 1.2 & sleep 0.05 & sleep 0.3; jobs %1; echo "--"; jobs; wait; disown -a

echo "=== forgetting jobs wholesale gives up the markers before selecting any"
# `-r` finds nothing running to forget, and still leaves the job unmarked…
sleep 0.05 & sleep 0.3; disown -r; jobs; disown -a
# …whereas `-h -r`, which forgets nothing at all, does not.
sleep 0.05 & sleep 0.3; disown -h -r; jobs; disown -a
sleep 0.05 & sleep 0.3; disown -h -a; jobs; disown -a

echo "=== reaping the newest job hands the + back to an older running one"
# …and that is the job a bare `disown` then takes.
sleep 0.9 & sleep 0.3 & wait %2; disown; jobs; echo "rc=$?"
wait
