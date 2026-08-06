# `${name:=word}` writes through a nameref like any other store, and a reference
# designating an array **element** is a perfectly good destination for it: the
# element is created, and the base is bound where it is written (see
# a-nameref-base-is-bound-as-written-on-a-store.sh).
#
# What is refused is a subscript on a subscript — `${ref[1]:=v}` where `ref`
# already designates one element — and a reference naming an array *whole*,
# which is the bad subscript it looks like.
#
# The `=` and `:=` forms differ over an element that is there but empty, and
# neither stores when the element already has a value.
#
# Every case here subscripts element **0**. bash 5.2.37 *segfaults* on this
# shape for any other subscript — `n=(); declare -n r='n[1]'; : "${r:=D}"` and
# every associative key crash outright, and `n=(a); declare -n r='n[1]'`
# expands to `a` — so there is no behaviour there to be byte-exact with. See
# TD-OILS-BASH-CRASHES-ON-A-DEFAULTING-ASSIGNMENT-THROUGH-A-REFERENCE.

echo '=== the element is created'
( n=(); declare -n r='n[0]'; echo "[${r:=D}]"; declare -p n 2>&1 )
( declare -a n; declare -n r='n[0]'; echo "[${r:=D}]"; declare -p n 2>&1 )
echo '=== a base that is wholly absent is made'
( declare -n r='nosuch[0]'; echo "[${r:=D}]"; declare -p nosuch 2>&1 )
echo '=== nothing is stored when the element is already there'
( n=(a); declare -n r='n[0]'; echo "[${r:=D}]"; declare -p n 2>&1 )
echo '=== an empty element: := stores, = does not'
( n=(""); declare -n r='n[0]'; echo "[${r:=D}]"; declare -p n 2>&1 )
( n=(""); declare -n r='n[0]'; echo "[${r=D}]"; declare -p n 2>&1 )
echo '=== through a nameref base, which is bound as written'
( n=(); declare -n base=n; declare -n r='base[0]'
  echo "[${r:=D}]"; declare -p base n 2>&1 )
echo '=== through a chain of references'
( n=(); declare -n b1='n[0]'; declare -n a1=b1; echo "[${a1:=D}]"; declare -p n 2>&1 )

echo '=== what is still refused'
echo '--- a subscript on a subscript'
( n=(a b c); declare -n r='n[0]'; echo "[${r[1]:=D}]"; echo "s=$?"; declare -p n 2>&1 )
echo '--- a reference naming a whole array'
( declare -a k=(); declare -n g='k[*]'; echo "[${g:=v}]"; echo "s=$?"; declare -p k 2>&1 )
( declare -a k=(); declare -n g='k[@]'; echo "[${g:=v}]"; echo "s=$?"; declare -p k 2>&1 )
echo '--- a readonly base'
( n=(); readonly n; declare -n r='n[0]'; echo "[${r:=D}]"; echo "s=$?"; declare -p n 2>&1 )
echo '--- a bad subscript'
( n=(a b c); declare -n r='n[1+]'; echo "[${r:=D}]"; echo "s=$?"; declare -p n 2>&1 )

echo '=== the other operators keep their own answers'
( n=(); declare -n r='n[0]'; echo "[${r:?}]"; echo "s=$?" )
( n=(); declare -n r='n[0]'; echo "[${r-U}]"; echo "s=$?"; declare -p n 2>&1 )
( n=(); declare -n r='n[0]'; echo "[${r+S}]"; echo "s=$?"; declare -p n 2>&1 )
( n=(); declare -n r='n[0]'; echo "[${r:-U}]"; echo "s=$?"; declare -p n 2>&1 )
