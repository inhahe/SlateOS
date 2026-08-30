# `unset` on a nameref that designates an array **element** removes the
# element — and it reaches the array the way a *read* does, following the base
# through a nameref chain of its own. It is the store side, not this one, that
# binds the base where it is written (see
# a-nameref-base-is-bound-as-written-on-a-store.sh).
#
# One walk, so a circular base is reported once, removes nothing, and is not an
# error. A base that resolves nowhere — unset, or itself designating an element
# — removes nothing and is not an error either.
#
# The readonly guard is not consulted on this path at all: bash refuses
# `unset 'n[0]'` on a readonly `n` and performs the very same removal through
# `declare -n r='n[0]'`.

echo '=== the base is followed'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  unset -v r; echo "s=$?"; declare -p base n 2>&1 )
echo '=== two links deep'
( n=(a b c); declare -n mid=n; declare -n base=mid; declare -n r='base[1]'
  unset -v r; echo "s=$?"; declare -p base mid n 2>&1 )
echo '=== an associative base, by key'
( declare -A mm=([k]=K [j]=J); declare -n base=mm; declare -n r='base[k]'
  unset -v r; echo "s=$?"; declare -p base mm 2>&1 )
echo '=== a scalar base is removed whole at index 0'
( s=hello; declare -n base=s; declare -n r='base[0]'
  unset -v r; echo "s=$?"; declare -p base s 2>&1 )
echo '=== a whole-array subscript empties the array it found'
( n=(a b c); declare -n base=n; declare -n r='base[@]'
  unset -v r; echo "s=$?"; declare -p base n 2>&1 )
echo '=== without the base being a reference at all'
( n=(a b c); declare -n r='n[1]'
  unset -v r; echo "s=$?"; declare -p n 2>&1 )

echo '=== a base that resolves nowhere removes nothing'
echo '--- unset base'
( declare -n base=nosuch; declare -n r='base[0]'
  unset -v r; echo "s=$?"; declare -p base nosuch 2>&1 )
echo '--- base designating an element'
( n=(a b c); declare -n base='n[1]'; declare -n r='base[0]'
  unset -v r; echo "s=$?"; declare -p base n 2>&1 )
echo '--- circular base, reported once'
( declare -n c1=c2; declare -n c2=c1; declare -n r='c1[0]'
  unset -v r; echo "s=$?"; declare -p c1 c2 r 2>&1 )

echo '=== readonly is not consulted through a reference'
echo '--- written directly, it is'
( n=(a b c); readonly n; unset -v 'n[0]'; echo "s=$?"; declare -p n 2>&1 )
echo '--- through a reference straight to the element'
( n=(a b c); readonly n; declare -n r='n[0]'
  unset -v r; echo "s=$?"; declare -p n 2>&1 )
echo '--- through a reference with a nameref base'
( n=(a b c); readonly n; declare -n base=n; declare -n r='base[0]'
  unset -v r; echo "s=$?"; declare -p base n 2>&1 )
echo '--- readonly on the base rather than the array'
( n=(a b c); declare -n base=n; readonly base; declare -n r='base[0]'
  unset -v r; echo "s=$?"; declare -p base n 2>&1 )

echo '=== a written subscript follows its base too'
( n=(a b c); declare -n base=n; unset -v 'base[0]'; echo "s=$?"; declare -p base n 2>&1 )
echo '=== and -n unsets the reference itself, touching nothing else'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  unset -n r; echo "s=$?"; declare -p base n 2>&1; declare -p r 2>&1 )
