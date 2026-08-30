# A declaration builtin that has been given *nothing to do* — no flag in either
# direction, no `-g`, no value — does not reach for a reference's base at all.
# bash has nothing to build a new name out of, so it falls through to an
# ordinary `bind_variable` of the operand, which follows the reference and lands
# in `assign_array_element` on the target's own spelling.
#
# So the subscript is really *evaluated*, and what comes back is
# `assign_array_element`'s own answer, in its own order:
#
#   - the whole-array tokens are refused before anything is evaluated;
#   - an associative base takes the subscript as a *key*, and minds only an
#     empty one;
#   - an indexed base evaluates it as arithmetic, then minds a negative that
#     still points before the start.
#
# An arithmetic error is fatal, the way every arithmetic error in an expansion
# is — it jumps straight out of the builtin, so no later operand is looked at.
# The other two only complain, and the caller's own refusal still follows them.
#
# Any flag at all takes the operand off this path, which is why the same letters
# that silence the whole-array line also stop the evaluation: it *is* that line.

echo '=== an indexed base evaluates the subscript as arithmetic'
( n=(a b c); declare -n r='n[1]';   declare r; echo "rc=$?" )
( n=(a b c); declare -n r='n[1+1]'; declare r; echo "rc=$?" )
( n=(a b c); declare -n r='n[c]';   declare r; echo "rc=$?" )

echo '=== …and an arithmetic error there is fatal'
( n=(a b c); declare -n r='n[1+]';  declare r; echo "rc=$?" )
( n=(a b c); declare -n r='n[9/0]'; declare r; echo "rc=$?" )
( n=(a b c); declare -n r='n[(]';   declare r; echo "rc=$?" )

echo '=== so no operand after it is looked at'
( n=(a b c); declare -n r='n[1+]'; declare -n s='n[@]'; declare r s; echo "rc=$?" )

echo '=== a negative that still points before the start is only complained about'
( n=(a b c); declare -n r='n[-1]'; declare r; echo "rc=$?" )
( n=(a b c); declare -n r='n[-3]'; declare r; echo "rc=$?" )
( n=(a b c); declare -n r='n[-9]'; declare r; echo "rc=$?" )

echo '=== the whole-array tokens are refused before anything is evaluated'
( n=(a b c); declare -n r='n[@]'; declare r; echo "rc=$?" )
( n=(a b c); declare -n r='n[*]'; declare r; echo "rc=$?" )

echo '=== an associative base takes the subscript as a key, not an expression'
( declare -A m=([k]=v); declare -n r='m[1+]'; declare r; echo "rc=$?" )
( declare -A m=([k]=v); declare -n r='m[k]';  declare r; echo "rc=$?" )
( declare -A m=([k]=v); declare -n r='m[]';   declare r; echo "rc=$?" )

echo '=== any flag takes the operand off the path entirely'
( n=(a b c); declare -n r='n[1+]'; declare -i r; echo "rc=$?" )
( n=(a b c); declare -n r='n[1+]'; declare +l r; echo "rc=$?" )
( n=(a b c); declare -n r='n[1+]'; declare -g r; echo "rc=$?" )
( n=(a b c); declare -n r='n[1+]'; export r; echo "rc=$?" )

echo '=== a value does not: it keeps the operand on the path, error and all'
( n=(a b c); declare -n r='n[1+]'; declare r=x; echo "rc=$?" )
( n=(a b c); declare -n r='n[1+]'; declare r=(x y); echo "rc=$?" )

echo '=== a subscript that does evaluate still falls through to the ordinary path'
( declare -n q='nope[1]';   declare q; echo "rc=$?"; declare -p nope )
( declare -n q='nope[1+1]'; declare q; echo "rc=$?"; declare -p nope )

echo '=== the two halves disagree about which letters count'
( n=(a b c); declare -n r='n[1+]'; declare -r r; echo "rc=$?" )
( n=(a b c); declare -n r='n[1+]'; declare -r r=(x y); echo "rc=$?" )

echo '=== the array is left alone throughout'
( n=(a b c); declare -n r='n[-9]'; declare r; declare -p n )
( n=(a b c); declare -n r='n[@]';  declare r; declare -p n )

echo '=== done'
