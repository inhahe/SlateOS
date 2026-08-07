# A finished job is not forgotten the moment it is reported. It lingers until
# the next thing that sweeps the job table, and that delay is visible: a job can
# still be named after the listing that announced it. `disown -a` between lines
# empties the table, so each line starts from the same state.
#
# What is *measured* here is the sweeping, never the timing — but the setup is
# timed, and has to be read as such. A `sleep S &` followed by a foreground
# `sleep G` is a gate asserting "that job is finished by now"; a second, longer
# background job asserts "and this one is not". Every gate therefore needs
# margin on the side(s) it asserts, and what eats margin is not the sleeps but
# the **process spawns** between them — dear on Windows, dearer under a full
# corpus sweep where two shells fork every case. A 0.25 s margin was not enough:
# `jobs` once read `Running` where bash read `Done`. The gates below leave at
# least 0.5 s on every side they assert.
#
# That, and the sheer number of spawns, is why the case states a budget of its
# own rather than living inside the harness's 20s default.
# TIMEOUT: 60

echo "=== reporting a job by name loses it at once, in a bare listing it lingers"
# The jobspec form lists first and sweeps after, so there is nothing left for
# the `kill` to find…
sleep 0.05 & sleep 0.6; jobs %1; kill %1; echo "rc=$?"; disown -a
# …while the bare form sweeps first, so the job it reported outlives it.
sleep 0.05 & sleep 0.6; jobs; kill %1; echo "rc=$?"; disown -a
# Which is why a bare listing never announces the same job twice, and naming it
# twice fails the second time.
sleep 0.05 & sleep 0.6; jobs; jobs; echo "listed once"; disown -a
sleep 0.05 & sleep 0.6; jobs %1; jobs %1; echo "rc=$?"; disown -a

echo "=== naming a job in kill puts it back on the list of fates still owed"
# Even for signal 0, which delivers nothing.
sleep 0.05 & sleep 0.6; jobs; kill -0 %1; jobs; echo "listed twice"; disown -a
# `disown -h`, which also leaves the job in place, does not.
sleep 0.05 & sleep 0.6; jobs; disown -h %1; jobs; echo "listed once"; disown -a

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
sleep 0.05 & sleep 0.6; wait; jobs; disown -a
sleep 0.05 & sleep 0.6; wait; wait; jobs %1; echo "rc=$?"; disown -a
# With a second job to be last, job 1 loses that protection. Two-sided: job 1
# has to be finished before the `wait` and job 2 still running, or the `wait`
# waits for nothing and job 2 survives the sweep the way job 1 does above.
sleep 0.05 & sleep 1.5 & sleep 0.6; wait; jobs; echo "nothing left"; disown -a
# A job the `wait` really waited for is discarded however it ended.
sleep 0.3 & wait; jobs %1; echo "rc=$?"; disown -a

echo "=== numbering restarts once the table is swept, not when a job merely ends"
sleep 0.2 & wait %1; sleep 0.4 & jobs; wait; disown -a
sleep 0.05 & sleep 0.6; sleep 0.4 & jobs; wait; disown -a

echo "=== losing a marked job costs both markers, losing an unmarked one neither"
# Dropping the `-` job leaves the `+` job in place, yet the listing that follows
# shows no marker at all: with nothing running there is nothing to re-mark.
sleep 0.05 & sleep 0.06 & sleep 0.6; jobs %1; echo "--"; jobs; disown -a
# With something still running, that running job takes both markers. Two-sided
# again: the gate has to outlast `sleep 0.05` and be outlasted by the first job,
# which is why the first job is 1.2 s — ~0.55 s of margin below the gate and
# ~0.6 s above it. The trailing `wait` pays the remainder in wall clock.
sleep 1.2 & sleep 0.05 & sleep 0.6; jobs %2; echo "--"; jobs; wait; disown -a
# Naming the running job reports it without dropping it, so nothing moves.
sleep 1.2 & sleep 0.05 & sleep 0.6; jobs %1; echo "--"; jobs; wait; disown -a

echo "=== forgetting jobs wholesale gives up the markers before selecting any"
# `-r` finds nothing running to forget, and still leaves the job unmarked…
sleep 0.05 & sleep 0.6; disown -r; jobs; disown -a
# …whereas `-h -r`, which forgets nothing at all, does not.
sleep 0.05 & sleep 0.6; disown -h -r; jobs; disown -a
sleep 0.05 & sleep 0.6; disown -h -a; jobs; disown -a

echo "=== reaping the newest job hands the + back to an older running one"
# …and that is the job a bare `disown` then takes. The `wait %2` is the gate
# here — it returns exactly when job 2 ends — so job 1 only has to outlive it:
# 1.5 s against job 2's 0.3 s leaves ~1.2 s for the spawns in between.
sleep 1.5 & sleep 0.3 & wait %2; disown; jobs; echo "rc=$?"
wait
