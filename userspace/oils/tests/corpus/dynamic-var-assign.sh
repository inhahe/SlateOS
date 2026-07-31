# Assigning a dynamic variable fills its value cell and nothing else.
#
# `SECONDS`, `RANDOM`, `LINENO` and the rest hold a slot with two halves — a
# value cell and an attribute set — and a *lookup* fills both (see
# dynamic-var-visible.sh). An *assignment* fills only the value cell:
#
#   SECONDS=7   →   declare -- SECONDS="7"      and no `-i`, so no `declare -i`
#   RANDOM=7    →   declare -i RANDOM="7"       (`-i` was in the slot already)
#
# The listings that walk the variable table print that cell, so they report the
# assigned string. What the assignment does *not* do is take the value function
# away: `$LINENO` still answers with the line it is on, `$RANDOM` still varies,
# `$SECONDS` counts again from the number it was given. So `declare -p NAME` —
# a lookup — disagrees with `declare -p` — a table walk — from the moment of
# the assignment onwards, and goes on disagreeing as the value moves:
#
#   SECONDS=0; sleep 2; declare -p          →  declare -- SECONDS="0"
#   SECONDS=0; sleep 2; declare -p SECONDS  →  declare -i SECONDS="2"
#
# …and the lookup refills the cell as it goes, so the listing agrees again
# afterwards, at the value the lookup happened to see.
#
# Some of the family do not fill the cell at all: their assign function drops
# the string, so `BASHPID=7` is not an error and leaves the slot exactly as it
# was. `PPID` never gets that far — it is readonly.
#
# Values that differ between two shells on the same host are masked. The mask
# spells no double quote and no backslash of its own: osh cannot yet hand
# either to an external command on the Windows host it is developed on — see
# known-issues TD-OILS-WIN-ARG-QUOTING.
m() { sed -E -e 's/=.[0-9.]+.$/=Q/' -e 's/=[0-9.]+$/=Q/'; }
g() { grep -E "^declare -[^ ]+ $1(=|$)"; }

echo "=== the assigned string is what the listings report"
( SECONDS=7;   declare -p | g SECONDS )
( RANDOM=7;    declare -p | g RANDOM )
( SRANDOM=7;   declare -p | g SRANDOM )
( LINENO=7;    declare -p | g LINENO )
( HISTCMD=7;   declare -p | g HISTCMD )
( SECONDS=7;   set | grep '^SECONDS=' )
( LINENO=7;    set | grep '^LINENO=' )

echo "=== …after it has been read as a number, never as the text typed"
# Everything here is a number to the shell, so the string is parsed and the
# parse is what is stored. Which parse depends on the slot's `-i`: with it the
# value is an arithmetic expression, without it a plain decimal and nothing
# else, so anything unreadable — an expression included — is 0.
for val in 7 007 ' 7 ' -3 zz '' 3+4 1/0 9999999999999999999999; do
  printf '%-24s' "[$val]"
  ( SECONDS=$val; declare -p | g SECONDS ) 2>&1 | tr '\n' ' '
  ( LINENO=$val;  declare -p | g LINENO )  2>&1 | tr '\n' ' '
  ( RANDOM=$val;  declare -p | g RANDOM )  2>&1 | tr '\n' ' '
  echo
done
# A bad expression is a diagnostic and nothing else: the slot keeps what it
# had, the command succeeds, and — unlike the same expression assigned to an
# ordinary `-i` variable, which abandons the script — the next command runs.
( RANDOM=1/0; echo "rc=$? and still here" ) 2>&1
( declare -i x=5; x=1/0; echo unreachable ) 2>&1
# One lookup first, and `SECONDS` reads its value the other way.
( : $SECONDS; SECONDS=3+4; declare -p | g SECONDS )
( : $SECONDS; SECONDS=zz;  declare -p | g SECONDS )
( : $LINENO;  LINENO=3+4;  declare -p | g LINENO )

echo "=== += appends to the cell, or adds to it under the -i"
( SECONDS=100; SECONDS+=5; declare -p | g SECONDS; echo "read=$SECONDS" )
( : $SECONDS; SECONDS=100; SECONDS+=5; declare -p | g SECONDS; echo "read=$SECONDS" )
( RANDOM=100;  RANDOM+=5;  declare -p | g RANDOM )
( LINENO=100;  LINENO+=5;  declare -p | g LINENO )
( HISTCMD=100; HISTCMD+=5; declare -p | g HISTCMD )
# …onto an empty cell there is nothing to append to.
( SECONDS+=5; declare -p | g SECONDS )
( RANDOM+=5;  declare -p | g RANDOM )

echo "=== but the attributes stay as they were"
# `SECONDS` keeps its slot's empty attribute set, so it is still out of
# `declare -i`; one lookup afterwards puts it in.
( SECONDS=7; declare -i | g SECONDS; echo "rc=$?" )
( SECONDS=7; : $SECONDS; declare -i | g SECONDS ) | m
( LINENO=7;  declare -i | g LINENO; echo "rc=$?" )
( RANDOM=7;  declare -i | g RANDOM )

echo "=== and these leave the slot untouched altogether"
( BASHPID=7;       declare -p | g BASHPID )
( EPOCHSECONDS=7;  declare -p | g EPOCHSECONDS )
( EPOCHREALTIME=7; declare -p | g EPOCHREALTIME )
( BASH_ARGV0=zz;   declare -p | g BASH_ARGV0 )
( BASH_SUBSHELL=7; declare -p | g BASH_SUBSHELL )
( BASHPID=7;       set | grep -c '^BASHPID=' )
( EPOCHSECONDS=7;  set | grep -c '^EPOCHSECONDS=' )

echo "=== the value function is still there afterwards"
( LINENO=7;       [ "$LINENO" = 7 ] && echo frozen || echo live )
( HISTCMD=7;      [ "$HISTCMD" = 7 ] && echo frozen || echo live )
( BASHPID=7;      [ "$BASHPID" = 7 ] && echo frozen || echo live )
( EPOCHSECONDS=7; [ "$EPOCHSECONDS" = 7 ] && echo frozen || echo live )
( RANDOM=7;       [ "$RANDOM" = 7 ] && echo frozen || echo live )
( SRANDOM=7;      [ "$SRANDOM" = 7 ] && echo frozen || echo live )
# `SECONDS=n` restarts the count from n rather than stopping it, from a
# negative number as readily as from a positive one.
( SECONDS=100; a=$SECONDS; sleep 1; b=$SECONDS
  [ "$a" != "$b" ] && echo climbing || echo "stuck [$a]" )
# The second reading is compared rather than printed: `SECONDS` is whole
# seconds of wall clock since the base, so what a 1 s sleep yields depends on
# where in the current second the assignment landed, and the two shells start a
# few milliseconds apart. Which second it lands on is not the point; that a
# negative base counts *up* is.
( SECONDS=-3; a=$SECONDS; echo "$a"; sleep 1; b=$SECONDS
  [ "$b" -gt "$a" ] && echo climbing || echo "stuck [$b]" )

echo "=== so the named form and the listing disagree, and go on disagreeing"
( SECONDS=0; sleep 2; declare -p SECONDS ) | m
( SECONDS=0; sleep 2; declare -p | g SECONDS )
# …until the lookup refills the cell, after which they agree again.
( SECONDS=0; sleep 2; declare -p SECONDS >/dev/null; declare -p | g SECONDS ) | m

echo "=== an assignment after a lookup fills the cell the same way"
( : $SECONDS; SECONDS=7; declare -p | g SECONDS )
( : $RANDOM;  RANDOM=7;  declare -p | g RANDOM )
( : $SECONDS; SECONDS=7; declare -i | g SECONDS )

echo "=== unset empties the cell, and what comes back is ordinary"
( SECONDS=7; unset SECONDS; declare -p | g SECONDS; echo "rc=$?" )
( SECONDS=7; unset SECONDS; set | grep -c '^SECONDS=' )
( SECONDS=7; unset SECONDS; SECONDS=9; echo "$SECONDS"; declare -p | g SECONDS )
( LINENO=7;  unset LINENO;  LINENO=9;  echo "$LINENO" )

echo "=== readonly still refuses first"
( readonly SECONDS; SECONDS=5; echo unreachable ) 2>&1
( PPID=5; echo unreachable ) 2>&1

echo "=== and none of it escapes the subshell that did it"
( SECONDS=7 ); declare -p | g SECONDS; echo "rc=$?"
( LINENO=7 ); declare -p | g LINENO; echo "rc=$?"
