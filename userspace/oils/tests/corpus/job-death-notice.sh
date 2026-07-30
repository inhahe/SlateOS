# What the shell does with the news that a background job was *killed*, as
# opposed to merely finished. Most signals are announced the moment the shell
# hears of the death, without waiting to be asked — on stderr, in a message
# carrying the job's pid, which the two shells cannot agree on. So every section
# below runs with stderr discarded and reads the announcement off its
# consequences instead: announcing is reporting, so the job leaves the table
# there and then, `jobs` has nothing left to say about it, and its number is
# free again.
#
# INT, TERM and PIPE are the exceptions — bash is built with
# DONT_REPORT_SIGTERM/DONT_REPORT_SIGPIPE, and an interrupt is the user's own
# doing — which is why those are the only signals a `jobs` listing can ever
# name. See jobs-listing.sh.
#
# Every kill is followed by a settle delay before anything looks: bash hears of
# a death asynchronously, so a job killed and inspected in the same breath may
# not have been noticed yet.

echo "=== an announced job is gone from the table"
{ sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.1; jobs; echo "j=$?"; } 2>/dev/null

echo "=== a subshell announces its own jobs"
( sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.1; jobs; echo "j=$?" ) 2>/dev/null

echo "=== but a command substitution keeps the news to itself"
# The message would land on the parent's stderr, unrelated to the value being
# collected, so it is suppressed there — and the job stays in the substitution's
# table for its own `jobs` to report. (These two come before anything that
# leaves a row behind: bash's substitution runs in a fork and so inherits the
# jobs standing at the time, where osh's starts with an empty table.)
x=$( { sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.1; jobs; } ); echo "[$x]"
# The silence belongs to the substitution itself, not to everything nested under
# it: a `( … )` group inside one announces again, on the parent's stderr, and so
# has nothing left to list.
x=$( ( sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.1; jobs ) 2>/dev/null ); echo "[$x]"

echo "=== nothing can name an announced job afterwards"
{ sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.1; :; } 2>/dev/null; wait %1; echo "rc=$?"
{ sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.1; jobs -n; echo "n=$?"; } 2>/dev/null

echo "=== the announcement is the report, so a wait is left with nothing to say"
{ sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.1; wait; echo "rc=$?"; jobs; echo "j=$?"; } 2>/dev/null
{ sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.1; wait -n; echo "rc=$?"; } 2>/dev/null

echo "=== …and its number is free for the next job"
{ sleep 5 & sleep 0.1; kill -HUP %1; sleep 0.1; :; } 2>/dev/null; sleep 5 & jobs; kill %1
# …and then reap it, because a TERM is *not* announced: nothing else forces the
# shell to notice this death, so without the `wait` the row would linger in the
# table for an unbounded time and every section below would be numbered around
# it or not, depending on how quickly the shell got round to it. That is a real
# race and not a theoretical one — bash still lists this job as `Running` a fifth
# of a second after the kill on roughly a third of runs under load.
wait %1 2>/dev/null

echo "=== a signal the listing can word is not announced at all"
# These stay in the table, still owed to whoever asks — the contrast that says
# the announcement above was a report and not merely a death. Each block runs in
# a subshell and makes *both* of the jobs it names, so it starts from an empty
# table and leaves nothing behind: the survivor of a block like this is precisely
# the row that would otherwise make the next section's numbering a race.
( sleep 5 & sleep 5 & sleep 0.1; kill -TERM %2; sleep 0.1; jobs ) 2>/dev/null
( sleep 5 & sleep 5 & sleep 0.1; kill -PIPE %1; sleep 0.1; jobs ) 2>/dev/null

echo "=== only the killed job goes; the rest keep their numbers"
{ sleep 5 & sleep 5 & sleep 5 & sleep 0.1; kill -HUP %2; sleep 0.1; :; } 2>/dev/null; jobs
kill %1 %3
