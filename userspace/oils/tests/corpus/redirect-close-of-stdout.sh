# `>&-` closes a descriptor, and closing one is not the same as leaving it
# alone. The distinction is easy to lose because a shell that has never
# redirected fd 1 and a shell that has just *closed* fd 1 both have "nothing
# recorded for fd 1" — and yet the first must print to the terminal and the
# second must answer its own writes with `EBADF`. A shell that conflates them
# turns `exec 1>&-` into a reset to the terminal, which is its exact opposite.
#
# So this asks what a command with no fd 1 does, in each of the places fd 1 can
# be taken away:
#
#   * for one command (`echo hi >&-`), where the write fails and the status is
#     the write's;
#   * for a whole body (`{ …; } >&-`, a function, a `$( … )`), where every
#     command inside inherits the absence;
#   * for the rest of the shell (`exec 1>&-`), where it also stops being a
#     descriptor one can dup *from*, so `exec 3>&1` fails at the `exec` rather
#     than at some later write through the alias.
#
# Order matters as much as the close does: a redirect list is applied left to
# right and the last mention of fd 1 wins, so `>&- 1>f` writes the file and
# `1>f >&-` does not. Both spellings of the operator name the same descriptor —
# `1<&-` closes fd 1 exactly as `1>&-` does, because which side of the `&` the
# `-` is written on says nothing about which descriptor is meant.
#
# An *external* `cmd >&-` is the last section: there the shell has nothing to
# report, because the failure is the child's own. bash forks and leaves the
# child without an fd 1; osh spawns and arranges the same absence, so the two
# children fail alike — `cat: standard output: Bad file descriptor`, status 1 —
# and a child that never writes is untroubled by it.
#
# Deliberately absent: a dup *from* fd 1 later in the same list that closed it
# (`>&- 3>&1`, `>&- 2>&1` — bash fails these with `1: Bad file descriptor`),
# which osh knowingly answers differently. Which is why the capture below writes
# `2>&1 >&-` rather than `>&- 2>&1`: fd 2 must be given fd 1 while fd 1 is still
# there. See TD-OILS-TRANSIENT-CLOSE-OF-A-STD-FD-IS-A-NO-OP.
#
# Every persistent probe runs in a subshell so the close cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
exec 4>&2 2>err

echo "=== one command, with its stdout taken away"
( echo hi >&- );        echo "  echo rc=$?"
( printf 'x\n' >&- );   echo "  printf rc=$?"
( echo hi >&- >&- );    echo "  twice rc=$?"
( echo hi 1>&- );       echo "  1>&- rc=$?"
( echo hi 1<&- );       echo "  1<&- rc=$?"

echo "=== a whole body"
( { echo A; echo B; } >&- ); echo "  group rc=$?"
( f() { echo C; }; f >&- );  echo "  func rc=$?"
( eval 'echo D' >&- );       echo "  eval rc=$?"

echo "=== and what a capture of one collects"
v=$( { echo E; } 2>&1 >&- ); echo "  cap rc=$? v=[$v]"

echo "=== last mention of fd 1 wins"
( echo hi >&- 1>f1 );  echo "  reopen rc=$? f1=[$(cat f1)]"
( echo hi 1>f2 >&- );  echo "  close-after rc=$? f2=[$(cat f2)]"

echo "=== a close is not a reset to the terminal"
( exec 1>&-; echo hi );            echo "  exec rc=$?"
( exec 1>&-; printf 'y\n' );       echo "  exec printf rc=$?"
( exec 1>&-; exec 3>&1 );          echo "  dup-from-closed rc=$?"
( exec 1<&-; echo hi );            echo "  exec 1<&- rc=$?"

echo "=== nor does it survive being replaced"
( exec 1>&-; exec 1>f3; echo hi ); echo "  reopened rc=$? f3=[$(cat f3)]"

echo "=== a dup of fd 1 follows the exec, not the capture around it"
w=$( exec 1>f4; exec 3>&1; echo hi >&3 ); echo "  w=[$w] f4=[$(cat f4)]"

echo "=== a pipeline stage whose stdout is closed"
( echo hi >&- | cat ); echo "  pipe rc=$?"

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err

# The child's own diagnostic names the command as it was started, and on this
# host that is the resolved path rather than the word (TD-OILS-HOST-ARGV0, a
# `std::process` limitation, not a target one), so the leading directories are
# filtered off. Everything goes down one pipe so write order is one order.
printf 'x\n' > in
{
  echo "=== an external child is given the closed descriptor, not an empty one"
  ( cat in >&-;  echo "  cmd rc=$?" )
  ( cat in 1>&-; echo "  1>&- rc=$?" )
  ( cat in 1<&-; echo "  1<&- rc=$?" )
  ( exec 1>&-; cat in; echo "  exec rc=$?" )

  echo "=== the last mention of fd 1 wins there too"
  ( cat in >&- 1>g1; echo "  reopen rc=$?" )
  ( cat in 1>g2 >&-; echo "  close-after rc=$?" )
  ( exec 1>&-; exec 1>g3; cat in; echo "  exec reopen rc=$?" )

  echo "=== a pipeline stage with no fd 1"
  cat in | tr a-z A-Z >&-; echo "  tail rc=$?"

  echo "=== a child that never writes is untroubled"
  # `cat` is not one of those even with nothing to copy: it closes its stdout
  # at exit, which is the failing write. `sleep` never touches fd 1 at all.
  ( sleep 0 >&-;        echo "  sleep rc=$?" )
  ( cat /dev/null >&-;  echo "  empty-cat rc=$?" )

  echo "=== a capture of one collects nothing"
  ( v=$(cat in >&-); echo "  cap rc=$? v=[$v]" )

  echo "=== and the shell's own fd 1 is left as it was"
  ( cat in >&-; cat in; echo "  after rc=$?" )
} 2>&1 | sed 's#^[^:]*/##'
echo "=== the files the reopens wrote"
echo "g1=[$(cat g1)] g2=[$(cat g2)] g3=[$(cat g3)]"
