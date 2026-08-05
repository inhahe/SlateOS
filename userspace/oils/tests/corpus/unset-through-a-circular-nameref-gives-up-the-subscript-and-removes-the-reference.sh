# `unset NAME` looks the name up twice — once to find out what it is unsetting,
# once to unset it — so a circular nameref is reported twice and the *reference*
# is what goes, leaving the rest of the cycle in place. `unset -n` asks nothing
# of the chain and says nothing.
#
# A *subscripted* operand through the same cycle gives up on the element
# entirely: there is no array for the subscript to apply to, so bash unsets the
# reference whole, exactly as the unsubscripted form does. Two things make that
# visible rather than merely plausible —
#
#   * it succeeds where the element removal would have failed. `unset 'c1[@]'`
#     does not complain that `c1` is no array, and `unset 'c1[5]'` does not
#     either, though both complaints are what the same subscripts earn on an
#     ordinary scalar.
#
#   * the subscript is never even read. `unset 'c1[x y]'` is silent, where the
#     same word anywhere a subscript is really evaluated is a syntax error.
#
# A chain that resolves keeps all of this at arm's length: the element goes, or
# the target does, and the reference survives either way.

echo '=== the reference goes, and the rest of the cycle stays'
( declare -n c1=c2; declare -n c2=c1; unset c1; echo "rc=$?"; declare -p c1 c2 )
( declare -n c1=c2; declare -n c2=c1; unset -v c1; echo "rc=$?"; declare -p c1 c2 )
( declare -n a1=a2; declare -n a2=a3; declare -n a3=a1; unset a1; echo "rc=$?"; declare -p a1 a2 a3 )

echo '=== `-n` asks nothing of the chain'
( declare -n c1=c2; declare -n c2=c1; unset -n c1; echo "rc=$?"; declare -p c1 c2 )

echo '=== a subscript is given up on, whatever it says'
( declare -n c1=c2; declare -n c2=c1; unset 'c1[0]'; echo "rc=$?"; declare -p c1 c2 )
( declare -n c1=c2; declare -n c2=c1; unset 'c1[5]'; echo "rc=$?"; declare -p c1 c2 )
( declare -n c1=c2; declare -n c2=c1; unset 'c1[-1]'; echo "rc=$?"; declare -p c1 c2 )
( declare -n c1=c2; declare -n c2=c1; unset 'c1[@]'; echo "rc=$?"; declare -p c1 c2 )
( declare -n c1=c2; declare -n c2=c1; unset 'c1[*]'; echo "rc=$?"; declare -p c1 c2 )

echo '=== …and never read'
( declare -n c1=c2; declare -n c2=c1; unset 'c1[x y]'; echo "rc=$?"; declare -p c1 c2 )

echo '=== which is not what those subscripts earn on an ordinary scalar'
( t=v; unset 't[5]'; echo "rc=$?"; declare -p t )
( t=v; unset 't[@]'; echo "rc=$?"; declare -p t )
( t=v; unset 't[0]'; echo "rc=$?"; declare -p t )

echo '=== a chain that resolves keeps the reference'
( w=5; declare -n r=w; unset r; declare -p r w )
( w=(a b); declare -n r=w; unset 'r[0]'; declare -p r w )
( w=5; declare -n r=w; unset -n r; declare -p r w )

echo still here
