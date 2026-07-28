# `disown` forgets jobs. Each of its operands is resolved and acted on before
# the next one is looked at, which is what makes the order-dependent cases below
# come out the way they do. Every job outlives the `disown` that names it, so
# nothing here turns on timing.

echo "=== with no operand it forgets the current job, not the last row"
sleep 0.7 & sleep 0.8 & disown; jobs
wait

echo "=== -a takes every job, -r only the ones still running"
sleep 0.7 & sleep 0.8 & disown -a; jobs; echo "rc=$?"
wait
sleep 0.05 & sleep 0.7 & sleep 0.4; disown -r; jobs
wait

echo "=== an operand names a job; -h keeps it in the table"
sleep 0.7 & sleep 0.8 & disown %1; jobs
wait
sleep 0.7 & disown -h %1; jobs
wait
# …so with `-h` the same job can be named twice; without it the second attempt
# has nothing left to find.
sleep 0.7 & disown -h %1 %1; echo "rc=$?"; jobs
wait
sleep 0.7 & sleep 0.8 & disown %2 %2; echo "rc=$?"; jobs
wait

echo "=== the markers move as it goes, so %+ can name a second job"
# Forgetting job 2 makes job 1 the current job, which is what `%+` then finds.
sleep 0.7 & sleep 0.8 & disown %2 %+; echo "rc=$?"; jobs
wait

echo "=== a bad operand is reported and skipped, not fatal"
sleep 0.7 & sleep 0.8 & disown %nosuch %1; echo "rc=$?"; jobs
wait
sleep 0.7 & sleep 0.8 & disown %1 %nosuch; echo "rc=$?"; jobs
wait

echo "=== an unknown option is rejected with the usage line"
sleep 0.7 & disown -z; echo "rc=$?"
wait
