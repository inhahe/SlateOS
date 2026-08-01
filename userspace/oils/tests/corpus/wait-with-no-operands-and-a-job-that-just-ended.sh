# An operand-less `wait` divides the jobs into the ones it *waited for* and the
# ones it merely found already over. The first kind it has reported, so they are
# purged; the second kind it has not, so the one holding `$!` survives to be
# announced by a later `jobs` — and either kind is still fair game for a
# `wait -n`.
#
# Which kind a job is comes down to whether the shell had been *told* of its
# death before the `wait` was reached, and that is a matter of elapsed time
# rather than of commands: a background job is a forked child, and hearing that
# it ended costs a fork, an exit and a signal. Four builtins in a row do not buy
# that; a `sleep` does. So the middle two sections below differ only in what sits
# between the `&` and the `wait`, and answer differently.
#
# The last section covers the other half of the same bookkeeping: a job that
# *changes* state is owed to the user again, so listing it while it ran does not
# swallow the announcement of its end.
#
# `-p VAR` is reported as set-or-not rather than by value, since the value is a
# pid.
pvar() {
  case ${VAR-unset} in
    unset) echo "unset" ;;
    *[!0-9]*) echo "not a pid: $VAR" ;;
    *) echo "a pid" ;;
  esac
}

echo "=== the wait is the very next command, so the wait is what waited"
VAR=stale; ( exit 3 ) & wait -p VAR; echo "  noargs=$? $(pvar)"
VAR=stale; wait -n -p VAR;           echo "  n=$? $(pvar)"
echo "  jobs:"; jobs; echo "  ."

echo "=== four builtins in between buy no time, so still nothing was known"
VAR=stale; ( exit 3 ) & :; :; :; :; wait -p VAR; echo "  noargs=$? $(pvar)"
VAR=stale; wait -n -p VAR;                       echo "  n=$? $(pvar)"
echo "  jobs:"; jobs; echo "  ."

echo "=== a sleep does, and the job was over before the wait was reached"
VAR=stale; ( exit 3 ) & sleep 0.4; wait -p VAR; echo "  noargs=$? $(pvar)"
VAR=stale; wait -n -p VAR;                      echo "  n=$? $(pvar)"
echo "  jobs:"; jobs; echo "  ."

echo "=== a job that outlives the start of the wait is waited for either way"
VAR=stale; ( sleep 0.4; exit 3 ) & wait -p VAR; echo "  noargs=$? $(pvar)"
VAR=stale; wait -n -p VAR;                      echo "  n=$? $(pvar)"
echo "  jobs:"; jobs; echo "  ."

echo "=== only the last one backgrounded is spared"
( exit 3 ) & ( exit 4 ) & sleep 0.4; wait; echo "  noargs=$?"
VAR=stale; wait -n -p VAR; echo "  n=$? $(pvar)"
echo "  jobs:"; jobs; echo "  ."

echo "=== wait -n on its own does not care which kind the job is"
( exit 5 ) & VAR=stale; wait -n -p VAR; echo "  no grace: n=$? $(pvar)"
( exit 6 ) & sleep 0.4; VAR=stale; wait -n -p VAR; echo "  grace:    n=$? $(pvar)"

echo "=== listing a running job does not swallow the news that it ended"
( sleep 0.3; exit 3 ) & jobs
sleep 0.6
echo "  after it ended:"; jobs
echo "  and again:"; jobs; echo "  ."
