# A circular nameref names nothing, so a declaration about it has nothing to
# declare and is dropped. An operand that *makes an array*, though, still has an
# array to make, and it makes the operand's own — taking the nameref attribute
# off it on the way, exactly as an element assignment through the same chain
# does. That is what turns `declare -a` on a cycle from a no-op into the thing
# that breaks the cycle: once `c1` is no longer a reference, `c2` names an
# ordinary array and the shell is usable again.
#
# What counts as making an array is narrow, and the narrowness is visible:
#
#   * `-a`/`-A` make one for `declare`/`typeset`, whether or not a value comes
#     with them, and so does a subscript.
#
#   * `export -a` and `readonly -a` do *not*, because for those two builtins the
#     array is created by the assignment rather than by the letter — `export -a
#     fresh` on a name that does not exist leaves a plain `declare -x fresh`.
#
# The same line divides how much the chain *costs*. An ordinary operand walks it
# twice, and so reports a circular one twice; an array-making operand stops at
# the first walk, having already learnt that the chain leads nowhere, and
# reports it once.
#
# A chain that resolves is untouched by all of this: the array lands on the name
# the walk ends at, and the reference stays a reference.

echo '=== the operand makes its own, and stops being a reference'
( declare -n c1=c2; declare -n c2=c1; declare -a c1; echo "rc=$?"; declare -p c1 c2 )
( declare -n c1=c2; declare -n c2=c1; declare -A c1; echo "rc=$?"; declare -p c1 c2 )
( declare -n c1=c2; declare -n c2=c1; declare -ax c1; echo "rc=$?"; declare -p c1 c2 )
( declare -n c1=c2; declare -n c2=c1; typeset -a c1; echo "rc=$?"; declare -p c1 c2 )

echo '=== …so the far end names an ordinary array afterwards'
( declare -n c1=c2; declare -n c2=c1; declare -a c1; c2[1]=z; declare -p c1 )

echo '=== a value comes along with it'
( declare -n c1=c2; declare -n c2=c1; declare -a c1=x; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; declare -a c1=(x y); declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; declare -A c1=([k]=v); declare -p c1 )

echo '=== and `+n` is not what did it — the letter is'
( declare -n c1=c2; declare -n c2=c1; declare -a +n c1; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; declare +n c1; declare -p c1 )

echo '=== the two letters together still argue, once there is a name to argue about'
( declare -n c1=c2; declare -n c2=c1; declare -aA c1; echo "rc=$?"; declare -p c1 )

echo '=== an operand that makes nothing is dropped'
( declare -n c1=c2; declare -n c2=c1; declare c1; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; declare -i c1; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; declare -x c1; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; declare c1=5; echo "rc=$?"; declare -p c1 )

echo '=== the letter makes the array only for `declare` and its kin'
( export -a fresh; declare -p fresh )
( readonly -a fresh; declare -p fresh )
( declare -a fresh; declare -p fresh )
( export -a fresh=5; declare -p fresh )

echo '=== a longer cycle is no different'
( declare -n a1=a2; declare -n a2=a3; declare -n a3=a1; declare -a a1; declare -p a1 )

echo '=== a chain that resolves declares what it points at'
( w=5; declare -n r=w; declare -a r; declare -p r w )
( w=5; declare -n r=w; declare -A r; declare -p r w )
( declare -n r=zz; declare -a r; declare -p r zz )

echo still here
