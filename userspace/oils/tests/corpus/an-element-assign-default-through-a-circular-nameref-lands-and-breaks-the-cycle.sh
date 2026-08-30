# `${name:=word}` through a circular chain parts along the same seam the
# arithmetic writes part along: what the store has to fall back on.
#
# An **unsubscripted** destination has nothing. bash walks the chain once more
# looking for a variable, finds the cycle again, and gives up — the value is
# never stored and the expansion itself is impossible, so the command goes the
# way a division by zero goes. Two walks in all: one for the read, one for the
# store.
#
# A **subscripted** one has the array it would have had to make. bash walks
# twice more — once to find that array, once to bind the element — lands the
# store on the name the walk started from, and takes the nameref attribute off
# it in the doing. So `${c1[0]:=v}` really does assign, four warnings deep, and
# the chain that caused them is gone by the time the expansion returns its
# value: `c1` is a plain array afterwards, and `c2` now points at it for real.
#
# `[@]` and `[*]` are the third case: the store looks for the array before it
# judges the subscript, so the walk is paid — and it is paid *after* the default
# word, which the side-effect case below shows.
#
# Nothing here is a count for its own sake: the subscripted form changes what is
# stored, not just how often bash complains about the way there.

cyc='declare -n c1=c2; declare -n c2=c1;'

echo '=== an unsubscripted store has nowhere to fall back on, so nothing happens'
( eval "$cyc"; echo "[${c1:=v}]"; echo not-reached )
( eval "$cyc"; echo "[${c1=v}]"; echo not-reached )

echo '=== a subscripted one lands, and takes the cycle with it'
( eval "$cyc"; echo "[${c1[0]:=v}]"; declare -p c1 c2 )
( eval "$cyc"; echo "[${c1[3]:=v}]"; declare -p c1 c2 )
( eval "$cyc"; echo "[${c1[0]=v}]"; declare -p c1 c2 )

echo '=== so the far end of the chain can read it back'
( eval "$cyc"; echo "[${c1[0]:=v}]"; echo "c2=[${c2[0]}]" )
( eval "$cyc"; echo "[${c1[3]:=v}]"; echo "c2=[${c2[3]}]" )

echo '=== and a second write is an ordinary one, warning about nothing'
( eval "$cyc"; echo "[${c1[0]:=v}]"; echo "[${c1[0]:=w}]"; declare -p c1 )
( eval "$cyc"; echo "[${c1[0]:=v}]"; c1[1]=w; declare -p c1 )

echo '=== [@] and [*] pay the walk and then refuse the subscript'
( eval "$cyc"; echo "[${c1[@]:=v}]"; echo not-reached )
( eval "$cyc"; echo "[${c1[*]:=v}]"; echo not-reached )

echo '=== and pay it after the default word, not before'
( eval "$cyc"; echo "[${c1[@]:=$(echo SIDE >&2)}]" )

echo '=== a chain that resolves is silent, and stores where it points'
( declare -n r=t; echo "[${r:=v}]"; declare -p t )
( declare -n r=t; echo "[${r[1]:=w}]"; declare -p t )
( u=(); declare -n r=u; echo "[${r[0]:=v}]"; declare -p u )

echo still here
