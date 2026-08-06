# `<&-` closes fd 0, and a closed fd 0 is not an fd 0 that was never
# redirected. The two look alike from inside the shell — neither has anything
# recorded for fd 0 — and yet the first must answer a `read` with `EBADF` and
# the second must read the terminal. A shell that conflates them turns
# `exec 0<&-` into a reset to the terminal, which is its exact opposite.
#
# Nor is a closed descriptor one sitting at end of file, which is the easier
# mistake to make here: both leave `read` with nothing to assign and status 1,
# so only two things tell them apart —
#
#   * EOF *assigns* the empty record it did not find, overwriting whatever the
#     names held, while a read error assigns nothing and leaves them alone;
#   * EOF is silent, while a read error says
#     `read: read error: 0: Bad file descriptor`.
#
# `mapfile` is the exception: it answers a read error with an empty array,
# status 0 and no message at all — the same thing it answers EOF with.
#
# The write side of fd 0 has its own distinction to keep. A `< file` fd 0 is
# open but has no write half, so `>&0` fails at the *write*
# (`echo: write error: Bad file descriptor`); a closed fd 0 is no descriptor at
# all, so `>&0` fails at the *redirect* (`0: Bad file descriptor`, before the
# command runs). `exec 3<&0` and `exec 3>&0` likewise cannot copy a descriptor
# that is not there.
#
# Order matters as much as the close does: a redirect list is applied left to
# right and the last mention of fd 0 wins, so `<&- <in` reads the file and
# `<in <&-` does not. Both spellings of the operator name the same descriptor —
# `0>&-` closes fd 0 exactly as `0<&-` does.
#
# An *external* `cmd <&-` is the last section: there the shell has nothing to
# report, because the failure is the child's own. bash forks and leaves the
# child without an fd 0; osh spawns and arranges the same absence, so the two
# children fail alike. See TD-OILS-AN-EXTERNAL-CHILD-CANNOT-BE-GIVEN-A-CLOSED-FD-0.
#
# Every persistent probe runs in a subshell so the close cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
printf 'one\ntwo\n' > in
exec 4>&2 2>err

echo "=== a read with no fd 0"
( read -r l <&- );                     echo "  rc=$? l=[$l]"
( read -r l 0<&- );                    echo "  0<&- rc=$? l=[$l]"
( read -r l 0>&- );                    echo "  0>&- rc=$? l=[$l]"
( { read -r l; } <&- );                echo "  group rc=$? l=[$l]"
( exec 0<&-; read -r l );              echo "  exec rc=$? l=[$l]"
( exec 0>&-; read -r l );              echo "  exec 0>&- rc=$? l=[$l]"

echo "=== every way of asking for a record answers alike"
( read -r -t 1 l <&- );                echo "  -t rc=$? l=[$l]"
( read -r -N 2 l <&- );                echo "  -N rc=$? l=[$l]"
( read -r -n 2 l <&- );                echo "  -n rc=$? l=[$l]"
( read -r -d : l <&- );                echo "  -d rc=$? l=[$l]"
( IFS= read -r l <&- );                echo "  IFS= rc=$? l=[$l]"

echo "=== an error is not end of input"
( l=keep; read -r l < /dev/null; echo "  eof rc=$? l=[$l]" )
( l=keep; read -r l <&-;         echo "  closed rc=$? l=[$l]" )

echo "=== mapfile answers both the same way, and says nothing"
( mapfile -t a <&-;      echo "  rc=$? n=${#a[@]}" )
( mapfile -t a < /dev/null; echo "  eof rc=$? n=${#a[@]}" )

echo "=== the last mention of fd 0 wins"
( read -r l <&- <in );                 echo "  reopen rc=$? l=[$l]"
( read -r l <in <&- );                 echo "  close-after rc=$? l=[$l]"
( read -r l <&- <<< hs );              echo "  here-string rc=$? l=[$l]"
( read -r l <<< hs <&- );              echo "  close-after-hs rc=$? l=[$l]"
( exec 0<&-; exec 0<in; read -r l );   echo "  exec reopen rc=$? l=[$l]"

echo "=== a close is not a reset to the terminal, and the ambient fd is left alone"
( exec 0<in; read -r a <&-; read -r b; echo "  a=[$a] b=[$b]" )

echo "=== fd 0 as a write descriptor"
( { echo Q >&0; } <&- );               echo "  >&0 rc=$?"
( { echo Q >&0; } <in );               echo "  read-only >&0 rc=$?"
( exec 0<&-; exec 3<&0 );              echo "  3<&0 rc=$?"
( exec 0<&-; exec 3>&0 );              echo "  3>&0 rc=$?"

echo "=== fd 1 is untouched by any of it"
( echo G <&- );                        echo "  echo rc=$?"
( exec 0<&-; echo H );                 echo "  exec echo rc=$?"

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err

# The child's own diagnostic names the command as it was started, and on this
# host that is the resolved path rather than the word (TD-OILS-HOST-ARGV0, a
# `std::process` limitation, not a target one), so the leading directories are
# filtered off. Everything goes down one pipe so write order is one order.
{
  echo "=== an external child is given the closed descriptor, not an empty one"
  ( exec 0<&-; cat; echo "  exec rc=$?" )
  ( cat <&-; echo "  cmd rc=$?" )
  ( cat 0<&-; echo "  0<&- rc=$?" )
  ( cat 0>&-; echo "  0>&- rc=$?" )

  echo "=== the last mention of fd 0 wins there too"
  ( cat <&- <in; echo "  reopen rc=$?" )
  ( cat <in <&-; echo "  close-after rc=$?" )
  ( exec 0<&-; exec 0<in; cat; echo "  exec reopen rc=$?" )

  echo "=== a pipeline head with no fd 0"
  cat <&- | tr a-z A-Z; echo "  head rc=${PIPESTATUS[0]}"

  echo "=== a child that never reads is untroubled"
  ( exec 0<&-; echo E; echo "  echo rc=$?" )

  echo "=== and the shell's own fd 0 is left as it was"
  printf 'p\n' | { cat <&-; cat; echo "  after rc=$?"; }
} 2>&1 | sed 's#^[^:]*/##'
