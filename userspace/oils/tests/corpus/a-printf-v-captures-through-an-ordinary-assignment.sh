# `printf -v name` does not have a store of its own. The capture is an ordinary
# scalar assignment and nothing less, so everything that shapes one shapes it.
#
# The name's value attributes are applied, so `declare -i n; printf -v n %s 3+4`
# leaves 7 and not `3+4`, and a malformed expression aborts the command the way
# `n=3+` would. A nameref is followed, to a variable or to the single element
# one may designate. A bare name that already names an array writes *element 0*
# of it — indexed or associative — rather than replacing it. And the dynamic
# names the shell keeps elsewhere are written through their hooks.
#
# The refusals are the store's too, and are spelled its way: a readonly target
# is blamed by the name a bare operand's nameref chain landed on, but by the
# name a *subscripted* operand wrote; a name the shell maintains refuses
# silently; a chain that names nothing warns and refuses. All of them are worth
# status 1 and leave the target as it was.

echo "=== the name's value attributes apply"
declare -i n; printf -v n '%s' '3+4'; declare -p n
declare -u u; printf -v u '%s' abc; echo "u=$u"
declare -l l; printf -v l '%s' ABC; echo "l=$l"
declare -c c; printf -v c '%s' abc; echo "c=$c"
( declare -i b; printf -v b '%s' 'q+'; echo NOT-REACHED ) 2>&1; echo "rc=$?"

echo "=== a nameref is followed"
t=orig; declare -n r=t; printf -v r '%s' via; echo "t=$t"
a1=(x y); declare -n re='a1[1]'; printf -v re '%s' via; declare -p a1
declare -n l1=l2; declare -n l2=dest; printf -v l1 '%s' far; echo "dest=$dest"

echo "=== a bare name on an array writes element 0"
declare -a a2=(x y); printf -v a2 '%s' z; declare -p a2
declare -A m1; printf -v m1 '%s' v; declare -p m1
# (an already-populated one gains a `0` key beside the one it had, so it can
# report the pair: the order is bash's hash order, which osh shares.)
declare -A m2=([k]=old); printf -v m2 '%s' v; echo "n=${#m2[@]} 0=${m2[0]} k=${m2[k]}"; declare -p m2

echo "=== a subscript is a shell word and keeps its bytes"
declare -A m3; printf -v 'm3[a b]' '%s' v; declare -p m3
declare -a a3=(x y z); printf -v 'a3[-1]' '%s' v; declare -p a3
declare -a a4=(x y); printf -v 'a4[1+1]' '%s' v; declare -p a4

echo "=== a readonly target is refused, and named the store's way"
ro1=(x); readonly ro1; declare -n rr1=ro1
printf -v rr1 '%s' z; echo "rc=$?"; declare -p ro1
printf -v 'rr1[0]' '%s' z; echo "rc=$?"; declare -p ro1
readonly ros=orig; printf -v ros '%s' z; echo "rc=$? ros=$ros"

echo "=== a name the shell maintains refuses silently"
printf -v GROUPS '%s' x; echo "rc=$?"
printf -v 'GROUPS[0]' '%s' x; echo "rc=$?"

echo "=== a chain that names nothing warns and refuses"
declare -n c1=c2; declare -n c2=c1
printf -v c1 '%s' x; echo "rc=$?"

echo "=== a subscript that names nowhere fails the builtin"
declare -A m4; k=''; printf -v "m4[$k]" '%s' v; echo "rc=$?"; declare -p m4

echo "=== an empty result still creates the name"
unset f1; printf -v f1 ''; echo "rc=$? set=${f1+yes} f1=[$f1]"

echo "=== a bad format writes nothing but still reports"
unset h1; printf -v h1 '%z' 2>&1; echo "rc=$? set=${h1+yes}"
i1=old; printf -v i1 '%d' zz 2>&1; echo "rc=$? i1=[$i1]"
