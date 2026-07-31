# `compgen`'s three job actions read the shell's own job table.
#
# One set of jobs serves the whole case, with lifetimes long enough that they
# are all still running for the questions about running jobs and all finished
# for the one about finished ones. Nothing here reports or waits for a job in
# between, because a job's *lifetime in the table* — when a reported or reaped
# job stops being offered — is `jobs-sweep-line.sh`'s subject and not this
# case's.
# TIMEOUT: 60

echo "== with no jobs there is nothing to offer"
compgen -A job
echo "rc=$?"

echo "== newest job first, and only the first word of each command"
sleep 0.7 & sleep 0.8 & { sleep 0.9; } & true | sleep 1.0 & compgen -A job
echo "rc=$?"

echo "== -j is the same action, and the word narrows it like any other"
compgen -j s
echo "rc=$?"
compgen -j zz
echo "rc=$?"

echo "== running takes them all while they are all still running"
compgen -A running
echo "== stopped never answers: a script shell has no stopped jobs"
compgen -A stopped
echo "rc=$?"

echo "== -P/-S decorate and -X filters, as for every other source"
compgen -A job -P '<' -S '>'
compgen -A job -X 't*'

echo "== the job actions take their place in bash's own generation order"
compgen -A job -W 'wl' -k -X '[dfw]*'

echo "== once they have all finished they are still completions; running is not"
sleep 1.2
compgen -A job
compgen -A running
echo "rc=$?"
wait
