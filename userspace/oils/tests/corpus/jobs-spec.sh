# A `%…` operand names a job. Only the first character after the `%` decides
# what kind of spec it is, so `%+x` is still the current job and `%1x` is *not*
# job 1 — it falls through to a name match. Every job here outlives the builtin
# that names it, so what a spec resolves to is never in doubt.

echo "=== a name matches the start of the command, %?str anywhere in it"
sleep 0.7 & jobs %sleep
wait
sleep 0.7 & jobs "%?lee"
wait
sleep 0.7 & kill %sle; echo "rc=$?"
wait

echo "=== the current and previous job have four spellings between them"
sleep 0.7 & sleep 0.8 & jobs %%; jobs %+; jobs %-; jobs "%"
wait
# Only the first character is read, so a suffix is ignored rather than making
# the spec a name to match.
sleep 0.7 & sleep 0.8 & jobs %+x; jobs %-x
wait

echo "=== a job number has to be all digits"
sleep 0.7 & jobs %1
wait
# `%0` is not a job number and matches no command, so it is simply unknown…
sleep 0.7 & jobs %0; echo "rc=$?"
wait
# …and `%1x` is read as a *name* beginning `1x`, not as job 1.
sleep 0.7 & jobs %1x; echo "rc=$?"
wait

echo "=== a spec matching two jobs is an error, not a pick"
# The builtins do not agree on what to say after it: `disown` follows the
# ambiguity with its usual "no such job", `kill` and `wait` stop there.
#
# `jobs %sleep` is deliberately not exercised here: reference bash's answer to
# it is not stable. Merely having `PS1` or `LC_ALL` in the environment — values
# the shell never looks at on this path — flips it between "ambiguity + no such
# job, status 1" and "ambiguity alone, status 0". osh gives the former, which is
# what bash does when run plainly and what its own `disown` does either way.
sleep 0.7 & sleep 0.8 & disown %sleep; echo "rc=$?"
wait
sleep 0.7 & sleep 0.8 & kill %sleep; echo "rc=$?"
wait
sleep 0.7 & sleep 0.8 & wait %sleep; echo "rc=$?"
wait

echo "=== an unresolvable %spec is 'no such job', whichever builtin read it"
# For `kill` that is a different diagnostic from the one a stale *pid* gets,
# even though both are unfindable.
sleep 0.7 & jobs %nosuch; echo "rc=$?"
wait
sleep 0.7 & kill %nosuch; echo "rc=$?"
wait
sleep 0.7 & wait %nosuch; echo "rc=$?"
wait
sleep 0.7 & disown %nosuch; echo "rc=$?"
wait
kill %5; echo "rc=$?"
kill -0 %5; echo "rc=$?"
kill %%; echo "rc=$?"
kill %-; echo "rc=$?"

echo "=== …but a bare number is a pid, and an unknown one is a stale process"
kill 999999; echo "rc=$?"
