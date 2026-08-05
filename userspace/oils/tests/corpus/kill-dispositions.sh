# What a signal *does* to a job, as distinct from whether `kill` reaches it.
# Delivery succeeds either way, so the answer is read off the `jobs` line that
# follows.
#
# Every job is given a moment to settle before it is signalled. That is not
# padding: bash establishes an asynchronous child's signal dispositions after
# forking it, so a signal delivered immediately can land while the child still
# has the disposition it was forked with, and the shell answers differently from
# one run to the next.
#
# A job that survives its signal has to still be running when `jobs` asks. What
# it is racing is not the *nominal* delay in front of the listing but the wall
# clock that delay costs, and each `sleep` is a process spawn: on a machine busy
# enough — a full-corpus sweep — a nominal 0.2 s of settling has taken over a
# second, and `jobs` read `Done` where the run before read `Running`. Two things
# widen the margin. A surviving job sleeps 3 s rather than 1, and it is listed
# straight after the `kill` rather than after a second settling delay: that
# delay is only there so a job that *dies* is reaped before the listing looks,
# which is the killing loop's business and not this one's.
#
# Only signals bash reports through `jobs` are used: a job killed by anything
# other than INT, PIPE or TERM is announced the moment it is reaped, in a
# message carrying a pid the two shells cannot agree on.
#
# Twelve surviving jobs at 3 s apiece is most of a minute per shell, so the case
# says how long it needs rather than living inside the default budget.
# TIMEOUT: 180

echo "=== a signal whose default is to be ignored leaves the job running"
for s in CHLD URG WINCH CONT; do
  { sleep 3; echo alive; } & sleep 0.1; kill -"$s" %1; echo "$s rc=$?"; jobs; wait
done

echo "=== so do the two an asynchronous job is given to ignore"
# With job control off bash hands an asynchronous child SIGINT and SIGQUIT
# already ignored, so that an interrupt meant for the foreground cannot reach
# into the background. The child keeps the ignore across its exec, so this holds
# for every shape a job can take — a background job can never be listed as
# `Interrupt` or `Quit` at all.
for s in INT QUIT; do
  sleep 3 & sleep 0.1; kill -"$s" %1; echo "$s rc=$?"; jobs; wait
  { sleep 3; echo alive; } & sleep 0.1; kill -"$s" %1; echo "$s rc=$?"; jobs; wait
  ( sleep 3; echo alive ) & sleep 0.1; kill -"$s" %1; echo "$s rc=$?"; jobs; wait
  sleep 3 | cat & sleep 0.1; kill -"$s" %1; echo "$s rc=$?"; jobs; wait
done

echo "=== and one whose default is to terminate kills every shape"
# The job is listed as dead well before its own `sleep` would have ended, which
# is what says the signal landed. Nothing is echoed from inside a job here: a
# pipeline is left out because bash signals only its leading stage, so the job
# takes the last stage's clean exit and reads `Done` — and osh runs a compound
# job in a thread it cannot cancel, so a killed one still finishes its body.
for s in TERM PIPE; do
  sleep 1 & sleep 0.1; kill -"$s" %1; echo "$s rc=$?"; sleep 0.1; jobs; wait
  { sleep 1; } & sleep 0.1; kill -"$s" %1; echo "$s rc=$?"; sleep 0.1; jobs; wait
  ( sleep 1 ) & sleep 0.1; kill -"$s" %1; echo "$s rc=$?"; sleep 0.1; jobs; wait
done
