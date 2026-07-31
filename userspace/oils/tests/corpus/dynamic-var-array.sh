# Giving a dynamic variable an array kind is what finally takes it away.
#
# `SECONDS`, `RANDOM`, `LINENO` and the rest answer every read by calling a
# value function. `declare -a NAME`, `declare -A NAME` and even a bare
# `NAME[1]=9` widen the name the way they widen a scalar — and there is no
# stored scalar to carry, so bash reads the name one last time and carries what
# it read into element 0:
#
#   declare -a SECONDS   →   declare -ai  SECONDS=([0]="0")
#   declare -a PPID      →   declare -air PPID=([0]="1234")
#   declare -a LINENO    →   declare -a   LINENO=([0]="7")
#
# The letters come with it. They are the ones `declare -p NAME` reports — the
# slot's own set, not the half-filled one a listing sees — and from here they
# are *real* attributes of an ordinary array, which is why the carried `-i` then
# evaluates whatever a literal supplies and why a converted `PPID` is still
# readonly.
#
# What does not come with it is the value function. Element 0 is frozen at the
# reading the conversion took, so a converted `LINENO` reports the same line for
# ever after and a converted `RANDOM` stops varying. The binding is gone in
# exactly the sense `unset` means it (see dynamic-var-unset.sh): `unset` on the
# converted name leaves nothing at all behind.
#
# Values that differ between two shells on the same host are masked. The mask
# spells no double quote and no backslash of its own: osh cannot yet hand either
# to an external command on the Windows host it is developed on — see
# known-issues TD-OILS-WIN-ARG-QUOTING. `[[]0]` is a bracket expression holding
# `[`, which is how the element-0 mask avoids one.
m() { sed -E -e 's/[[]0]=.[0-9.]+./[0]=Q/' -e 's/=.[0-9.]+.$/=Q/'; }
g() { grep -E "^declare -[^ ]+ $1(=|$)"; }

echo "=== the conversion carries a last reading, and the slot's own letters"
( declare -a SECONDS;       declare -p SECONDS ) 2>&1 | m
( declare -A SECONDS;       declare -p SECONDS ) 2>&1 | m
( declare -a RANDOM;        declare -p RANDOM )  2>&1 | m
( declare -a SRANDOM;       declare -p SRANDOM ) 2>&1 | m
( declare -a BASHPID;       declare -p BASHPID ) 2>&1 | m
( declare -a PPID;          declare -p PPID )    2>&1 | m
( declare -a EPOCHSECONDS;  declare -p EPOCHSECONDS )  2>&1 | m
( declare -a EPOCHREALTIME; declare -p EPOCHREALTIME ) 2>&1 | m
# …and these read the same in both shells, so the value is shown as it is.
( declare -a LINENO;        declare -p LINENO ) 2>&1
( declare -a HISTCMD;       declare -p HISTCMD ) 2>&1
( declare -a BASH_SUBSHELL; declare -p BASH_SUBSHELL ) 2>&1
( declare -a BASH_ARGV0;    declare -p BASH_ARGV0 ) 2>&1
( declare -A LINENO;        declare -p LINENO ) 2>&1
# The call-stack arrays are arrays already, so there is nothing to convert —
# and nothing to convert them *to*, either.
( declare -a BASH_SOURCE;   declare -p BASH_SOURCE ) 2>&1
( declare -a BASH_LINENO;   declare -p BASH_LINENO ) 2>&1
( declare -A BASH_SOURCE;   declare -p BASH_SOURCE ) 2>&1

echo "=== the reading is taken now, not lifted out of the value cell"
# `LINENO=7` fills the cell with 7; the conversion two lines later carries the
# line it is on instead.
( LINENO=7
  declare -a LINENO
  declare -p LINENO ) 2>&1
( : $LINENO
  declare -a LINENO
  declare -p LINENO ) 2>&1

echo "=== and the value function does not survive it"
( declare -a LINENO; echo "[$LINENO]"; echo "[$LINENO]" ) 2>&1
( declare -a RANDOM; a=$RANDOM; b=$RANDOM
  [ "$a" = "$b" ] && echo stable || echo varying )
( declare -a BASHPID; a=$BASHPID; b=$BASHPID
  [ "$a" = "$b" ] && echo stable || echo varying )
( declare -a HISTCMD; echo "[$HISTCMD]" ) 2>&1

echo "=== a bare subscripted assignment converts it too"
( LINENO[1]=9
  declare -p LINENO ) 2>&1
( HISTCMD[1]=9; declare -p HISTCMD ) 2>&1
( SECONDS[1]=9; declare -p SECONDS ) 2>&1 | m
( RANDOM[1]=9;  declare -p RANDOM )  2>&1 | m
( BASHPID[1]=9; declare -p BASHPID ) 2>&1 | m
# …and `+=(…)`, which appends after the carried element.
( HISTCMD+=(9); declare -p HISTCMD ) 2>&1
( LINENO+=(9)
  declare -p LINENO ) 2>&1
# Element 0 itself is simply overwritten.
( HISTCMD[0]=9; declare -p HISTCMD ) 2>&1

echo "=== the carried -i reaches the very element being assigned"
( HISTCMD[1]=zz;  declare -p HISTCMD ) 2>&1
( HISTCMD[1]=3+4; declare -p HISTCMD ) 2>&1
( LINENO[1]=zz
  declare -p LINENO ) 2>&1
# …and the elements a literal supplies, which replace the carried one outright.
( declare -a HISTCMD=(zz 3+4); declare -p HISTCMD ) 2>&1
( declare -a LINENO=(zz 3+4);  declare -p LINENO ) 2>&1
( declare -A HISTCMD=([k]=3+4); declare -p HISTCMD ) 2>&1
( declare -a HISTCMD=5; declare -p HISTCMD ) 2>&1
( export -a HISTCMD=5;   declare -p HISTCMD ) 2>&1
( readonly -a HISTCMD=5; declare -p HISTCMD ) 2>&1

echo "=== a malformed subscript converts nothing"
( HISTCMD[zz zz]=9 ) 2>&1; declare -p HISTCMD 2>&1
( HISTCMD[]=9; declare -p HISTCMD ) 2>&1
( HISTCMD[-9]=9; declare -p HISTCMD ) 2>&1

echo "=== a readonly one converts, and goes on refusing"
( declare -a PPID; PPID[1]=3; echo unreachable ) 2>&1
( readonly HISTCMD; declare -a HISTCMD; declare -p HISTCMD ) 2>&1
( readonly HISTCMD; declare -a HISTCMD=(1 2); echo unreachable ) 2>&1

echo "=== afterwards the name is ordinary, in every listing"
( declare -a HISTCMD; declare -p | g HISTCMD ) 2>&1
( declare -a HISTCMD; declare -a | g HISTCMD ) 2>&1
( declare -a HISTCMD; declare -i | g HISTCMD ) 2>&1
( declare -a HISTCMD; set | grep '^HISTCMD=' ) 2>&1
( declare -a LINENO;  declare -i | grep -c ' LINENO' ) 2>&1
( declare -a HISTCMD; echo "${!HISTC@}" ) 2>&1

echo "=== …and unset really does leave nothing behind"
( declare -a HISTCMD; unset HISTCMD; declare -p HISTCMD; echo "rc=$?" ) 2>&1
( declare -a HISTCMD; unset HISTCMD; HISTCMD=zz; declare -p HISTCMD ) 2>&1
( declare -a LINENO;  unset LINENO;  echo "[$LINENO]" ) 2>&1

echo "=== a local -a builds a fresh array and shadows it instead"
( f() { local -a HISTCMD; declare -p HISTCMD; }; f; declare -p HISTCMD ) 2>&1
( f() { local -a HISTCMD; HISTCMD[0]=zz; declare -p HISTCMD; }; f
  declare -p HISTCMD ) 2>&1
( f() { local HISTCMD; HISTCMD[1]=9; declare -p HISTCMD; }; f ) 2>&1

echo "=== an ordinary scalar widens the same way"
( w=5; w[1]=9;  declare -p w )
( w=5; w[-1]=9; declare -p w )
( w=5; declare -a w; declare -p w )
( declare -i w=5; w[1]=zz; declare -p w )

echo "=== and none of it escapes the subshell that did it"
( declare -a HISTCMD ); declare -p HISTCMD
( HISTCMD[1]=9 );       declare -p HISTCMD
