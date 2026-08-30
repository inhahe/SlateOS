# `jobs -x cmd …` runs `cmd` with each operand that names a job replaced by a
# process id, so that something which only understands pids can still be handed
# a job. The substituted value is deliberately not printed here: bash answers
# with the job's process *group*, which under a shell that is not doing job
# control is the shell's own group and so names no job in particular. What the
# two shells do agree on is which words get replaced at all, and what running
# the command amounts to. Every job outlives the line that names it.

echo "=== only a whole word is a job spec"
sleep 0.3 & jobs -x echo x%1y; wait
sleep 0.3 & jobs -x export A=%1; echo "A=$A"; wait
# A spec that names nothing is left alone without complaint — the command may
# well have meant it literally.
sleep 0.3 & jobs -x echo %nosuch; wait
sleep 0.3 & jobs -x echo %0; wait
# An ambiguous one is reported, and also left alone.
sleep 0.3 & sleep 0.4 & jobs -x echo %sleep; wait

echo "=== the command runs in this shell, and its status is the result"
v=1; sleep 0.3 & jobs -x eval "v=2"; echo "v=$v"; wait
sleep 0.3 & jobs -x false; echo "rc=$?"; wait
sleep 0.3 & jobs -x nosuchcmd; echo "rc=$?"; wait
# With nothing to run there is nothing to report.
jobs -x; echo "rc=$?"

echo "=== -x is not a listing option, and will not share the line with one"
sleep 0.3 & jobs -l -x echo; echo "rc=$?"; wait
# …whereas one that comes after it arrives too late to shape anything.
sleep 0.3 & jobs -x -l echo after; echo "rc=$?"; wait
jobs -xy echo; echo "rc=$?"

echo "=== running a command reports no job, and so sweeps none"
sleep 0.05 & sleep 0.3; jobs -x echo z%1; jobs; echo "still listed once"
