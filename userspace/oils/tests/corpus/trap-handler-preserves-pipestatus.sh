# A trap handler puts `${PIPESTATUS[@]}` back the way it found it.
#
# `$?` survives a DEBUG/ERR/RETURN/signal handler — the handler is an
# interruption, not a command, so it is not allowed to answer for the one that
# fired it. `${PIPESTATUS[@]}` is saved and restored on exactly the same terms,
# and it has to be: nearly every handler body runs *some* command, and any
# command at all rewrites the array, so without the restore a script could not
# read the pipeline it had just run whenever a trap happened to be installed.
#
# What is *not* suppressed is the handler's own view. Inside the body the array
# is live and moves with each command the body runs, so a handler can read the
# pipeline that fired it — and then read its own.
h() { :; }
echo "=== the array a pipeline leaves is the same with a handler and without"
( false | true | false; echo "plain  [${PIPESTATUS[*]}]" )
( trap h DEBUG; false | true | false;         echo "debug  [${PIPESTATUS[*]}]" )
( trap 'true | true' DEBUG; false | true | false; echo "inline [${PIPESTATUS[*]}]" )
( set -o pipefail; trap h ERR; false | true | false; echo "err    [${PIPESTATUS[*]}]" )
( set -T; trap h RETURN; g() { return 0; }; false | true | false; g
  echo "return [${PIPESTATUS[*]}]" )
# The DEBUG trap fires once per simple stage *and* once for the `echo` that
# reads the array, so this is four handler bodies between the pipeline and the
# read — the restore has to survive all of them.
( trap 'true | true | true | true' DEBUG; false | true | false
  echo "many   [${PIPESTATUS[*]}]" )

echo "=== a signal handler leaves it alone too"
# `kill` is itself a command, so the array the handler must put back is the
# one-element one *it* left, not the pipeline's — hence a three-stage handler
# body, which would otherwise be plainly visible in the reading afterwards.
# The signal goes to `$BASHPID` and not `$$`: the trap is installed in this
# subshell and the parent has none, so `$$` would kill the script outright.
( trap 'true | true | true' USR1; false | true | false; kill -USR1 $BASHPID
  echo "signal [${PIPESTATUS[*]}]" )

echo "=== and \$? comes back with it"
( trap h DEBUG; false | true | exit 5; echo "st=$? [${PIPESTATUS[*]}]" )
( set -o pipefail; trap h DEBUG; exit 3 | exit 4 | exit 0
  echo "st=$? [${PIPESTATUS[*]}]" )

echo "=== but the handler's own body sees it live"
# Three firings for the three stages, each still looking at the `trap` command
# that installed the handler, then one for the `:` that finally sees the
# pipeline. The body's second reading is of its own first command, not of
# anything the fired-for command did.
r() { echo "in   [${PIPESTATUS[*]}]"; echo "then [${PIPESTATUS[*]}]"; }
( trap r DEBUG; false | true | false; : )

echo "=== there is never nothing to put back"
# `unset` does not leave the array away for long: it is itself a command, and
# every command writes a one-element array, so the `trap` after it has already
# put one back. What the handler must not do is leave its own two-element one
# in its place.
( unset PIPESTATUS; trap 'true | true' DEBUG; declare -p PIPESTATUS ) 2>&1

echo "=== the EXIT trap has nothing left to disturb"
( false | true | false; trap 'echo "exit [${PIPESTATUS[*]}]"' EXIT )
