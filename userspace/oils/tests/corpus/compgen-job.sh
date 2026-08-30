# `compgen`'s three job actions read the shell's own job table.
#
# One set of jobs serves the whole case, with lifetimes long enough that they
# are all still running for the questions about running jobs and all finished
# for the one about finished ones. Nothing here reports or waits for a job in
# between, because a job's *lifetime in the table* — when a reported or reaped
# job stops being offered — is `jobs-sweep-line.sh`'s subject and not this
# case's.
#
# The two ends need very different margins, which is why they do not look
# symmetric. Everything between starting the jobs and the last question *about*
# running ones is a builtin, so the shell spawns nothing there and the jobs
# cannot finish early however loaded the machine is. Getting from there to "they
# have all finished" means waiting out a real process, and that is exposed to
# this host's spawn-latency spikes — hence the deliberately generous settle at
# the end.
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
# The settle has to outlast the longest job (1.0s) by more than this host's
# process-spawn latency spikes, which are routinely ~200ms and occasionally
# whole seconds (see known-issues
# TD-OILS-CORPUS-SWEEP-IS-UNRUNNABLE-WHEN-PROCESS-SPAWN-LATENCY-SPIKES). It
# used to be `sleep 1.2` — a 200ms margin, i.e. exactly the size of a routine
# spike — and a spike that landed on one shell's `sleep 1.0` and not the
# other's made the case fail about once per full sweep. `wait` is not the
# answer: it sweeps the table in both shells, so `compgen -A job` would list
# nothing and there would be no finished job left to ask about.
sleep 4
compgen -A job
compgen -A running
echo "rc=$?"
wait
