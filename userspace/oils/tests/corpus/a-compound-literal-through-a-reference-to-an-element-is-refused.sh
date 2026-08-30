# A compound literal makes a whole array, and a nameref designating one array
# **element** — or an array *whole* — names none. bash refuses the store,
# quoting the target as the reference holds it:
#
#     n=(a b c); declare -n r='n[1]'; r=(x y)   # `n[1]': not a valid identifier
#
# The refusal is the same one a declaration builtin's compound operand gives
# (see declare-compound-nameref.sh), and it comes before everything else the
# store would do with a subscripted target: before the base of an element
# destination is unreferenced (a-nameref-base-is-bound-as-written-on-a-store.sh),
# before a whole-array target is called a bad subscript, before the subscript is
# evaluated at all, before the readonly guard, and before the literal's own
# words are expanded.
#
# The abort it raises is the ordinary one — the same abort `a[-9]=x` raises, and
# measured against it in every shape. The rest of the list goes with it, a
# *function* takes its caller's list down too, and a subshell — whose body is a
# list of its own — exits. An `eval` confines it and reports 1, but only at the
# top level: inside a subshell the same `eval` does not, and the subshell exits
# 1 all the same. Both shapes are below, each written twice so the reference
# case can be read beside it.
#
# Every other case has to sit in a subshell of its own so the one after it is
# reached at all — which is why the two depths look alike unless you go looking.

echo '=== a reference to one element'
( n=(a b c); declare -n r='n[1]'; r=(x y); echo NOT; declare -p n r 2>&1 )
echo '=== …appending'
( n=(a b c); declare -n r='n[1]'; r+=(x y); echo NOT; declare -p n r 2>&1 )
echo '=== element 0, and an empty literal'
( n=(a b c); declare -n r='n[0]'; r=(); echo NOT )
echo '=== an associative key'
( declare -A mm=([k]=K); declare -n r='mm[k]'; r=(x y); echo NOT; declare -p mm 2>&1 )
echo '=== a reference to a whole array is quoted the same way'
( n=(a b c); declare -n r='n[@]'; r=(x y); echo NOT )
( n=(a b c); declare -n r='n[*]'; r=(x y); echo NOT )
( declare -A mm=([k]=K); declare -n r='mm[@]'; r=(x y); echo NOT )
echo '=== a base that does not exist is still the base'
( declare -n r='nosuch[1]'; r=(x y); echo NOT; declare -p nosuch 2>&1 )
echo '=== a nameref base is not unreferenced first'
( n=(a b c); declare -n base=n; declare -n r='base[1]'
  r=(x y); echo NOT; declare -p n base r 2>&1 )
echo '=== through a chain of references'
( n=(a b c); declare -n mid='n[1]'; declare -n r=mid; r=(x y); echo NOT )
echo '=== the subscript is never evaluated'
( n=(a b c); declare -n r='n[1+]'; r=(x y); echo NOT )
( n=(a b c); declare -n r='n[-9]'; r=(x y); echo NOT )
echo '=== the readonly guard is never reached'
( n=(a b c); readonly n; declare -n r='n[1]'; r=(x y); echo NOT )
echo '=== the words are never expanded'
( f() { echo RAN; echo z; }; n=(a b c); declare -n r='n[1]'; r=($(f)); echo NOT )
echo '=== the rest of the parse unit goes with it'
( n=(a b c); declare -n r='n[1]'; r=(x y); echo NOT1
echo NOT2 )
echo '=== at the top level an eval confines it, and reports 1'
n=(a b c); declare -n r='n[1]'
{ eval 'r=(x y); echo NOT'; echo "s=$?"; echo AFTER; }
a=(1 2); { eval 'a[-9]=x; echo NOT'; echo "s=$?"; echo AFTER; }
unset -n r; unset n a
echo '=== inside a subshell the same eval does not, and the subshell exits'
( n=(a b c); declare -n r='n[1]'
  { eval 'r=(x y); echo NOT'; echo "s=$?"; }; echo AFTER )
echo "s=$?"
( a=(1 2); { eval 'a[-9]=x; echo NOT'; echo "s=$?"; }; echo AFTER )
echo "s=$?"
echo '=== a function takes its caller down with it'
( f() { declare -n r='n[1]'; r=(x y); echo NOT1; }; n=(a b c); f; echo NOT2 )
echo '=== a subshell exits, its body being a list of its own'
( n=(a b c); declare -n r='n[1]'; ( r=(x y); echo NOT ); echo "s=$?"; echo AFTER )
echo '=== set -e and posix change nothing'
( set -e; n=(a b c); declare -n r='n[1]'; r=(x y); echo NOT ); echo "s=$?"
( set -o posix; n=(a b c); declare -n r='n[1]'; r=(x y); echo NOT ); echo "s=$?"
echo '=== an ordinary reference is followed as ever'
( n=(a b c); declare -n r=n; r=(x y); echo "s=$?"; declare -p n r 2>&1 )
( declare -n r=nosuch; r=(x y); echo "s=$?"; declare -p nosuch 2>&1 )
echo '=== and a scalar store through the same reference still lands'
( n=(a b c); declare -n r='n[1]'; r=x; echo "s=$?"; declare -p n 2>&1 )
( n=(a b c); declare -n r='n[1]'; r+=x; echo "s=$?"; declare -p n 2>&1 )
