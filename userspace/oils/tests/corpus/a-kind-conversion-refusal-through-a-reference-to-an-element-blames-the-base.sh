# The array-kind conversion refusal names the operand as the script wrote it,
# reference and all: `declare -n r=t; declare -A r` on an indexed `t` reports
# `r`, not `t`. A reference to an **element** is the exception — there the
# refusal names the base array, because the base is the array whose kind is
# being asked to change and the reference designates no variable of its own.
#
# This is the scalar half of
# the-kind-of-a-compound-through-a-reference-to-an-element-is-the-base-arrays,
# and it shows with no literal in sight: a valueless `declare -A r` through such
# a reference is refused just as loudly, and with the same name.
#
# An operand carrying a subscript of its own reports its base too, so
# `declare -A 'n[1]'` is the same line — nothing is named `n[1]`, and the kind
# was always the base's.
#
# Where the base has no kind to conflict with, the declaration makes it one and
# says nothing; and inside a function, where the declaration binds a local named
# by the whole spelling, there is nothing to convert at all.

echo '=== a reference to an element is blamed on the base'
( n=(a b c); declare -n r='n[1]'; declare -A r; echo "s=$?"; declare -p n )
( declare -A m=([k]=K); declare -n r='m[k]'; declare -a r; echo "s=$?"; declare -p m )
( n=(a b c); declare -n r='n[@]'; declare -A r; echo "s=$?"; declare -p n )
( n=(a b c); declare -n q='n[1]'; declare -n r=q; declare -A r; echo "s=$?"; declare -p n )
echo '=== a reference to a plain name is blamed on the operand'
( t=(a b c); declare -n r=t; declare -A r; echo "s=$?"; declare -p t )
( declare -A t=([k]=K); declare -n r=t; declare -a r; echo "s=$?"; declare -p t )
echo '=== an operand written with a subscript is blamed on its base'
( n=(a b c); declare -A 'n[1]'; echo "s=$?"; declare -p n )
( declare -A m=([k]=K); declare -a 'm[k]'; echo "s=$?"; declare -p m )
echo '=== a plain operand is blamed on itself'
( n=(a b c); declare -A n; echo "s=$?"; declare -p n )
echo '=== the -g form through a reference to an element asks it too'
( n=(a b c); declare -n r='n[1]'
  f() { declare -gA r; echo "s=$?"; declare -p n; }
  f )
echo '=== a base with no kind to convert is given one, quietly'
( declare -n r='nosuch[1]'; declare -A r; echo "s=$?"; declare -p nosuch; declare -p 'nosuch[1]' 2>&1 )
( declare -n r='nos2[1]'; declare -a r; echo "s=$?"; declare -p nos2 )
( n=(a b c); declare -n r='n[1]'; declare -a r; echo "s=$?"; declare -p n )
echo '=== inside a function there is nothing to convert'
( n=(a b c)
  f() { declare -n r='n[1]'; declare -A r; echo "s=$?"; declare -p 'n[1]'; declare -p n; }
  f )
( declare -A m=([k]=K)
  f() { declare -n r='m[k]'; declare -a r; echo "s=$?"; declare -p 'm[k]'; declare -p m; }
  f )
