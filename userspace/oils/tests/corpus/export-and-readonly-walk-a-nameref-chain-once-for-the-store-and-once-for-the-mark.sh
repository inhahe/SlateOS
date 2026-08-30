# `export` and `readonly` follow a nameref operand before they mark anything,
# and they look the name up more than once on the way: once to find it, once to
# mark it, and — when the operand carries a value — once more for the store. A
# circular chain is reported every time it is walked, so those three lookups are
# countable from the outside:
#
#     export c1        2        export -n c1     1
#     export c1=5      3        export -n c1=5   2
#     readonly c1      2        readonly c1=5    3
#
# `export -n` takes the export attribute *off* and so never reaches the marking
# lookup, which is the one walk it saves.
#
# An operand that makes an array is different in kind. `-a`/`-A` create nothing
# for these two builtins — the array comes from the assignment, so `export -a
# fresh` on a name that does not exist leaves a plain `declare -x fresh` — but
# `export -a c1=5` *is* an array write, and a write through a circular chain
# does not give up: it falls back on the reference's own name, drops the nameref
# attribute, and so breaks the cycle. One walk, one warning, and an array.
#
# A chain that resolves costs nothing to walk and is silent however often it is
# followed; the mark and the store both land at the far end.

echo '=== how many times the chain is walked'
( declare -n c1=c2; declare -n c2=c1; export c1; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; export c1=5; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; export c1+=5; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; readonly c1; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; readonly c1=5; echo "rc=$?"; declare -p c1 )

echo '=== …and `export -n` saves the marking lookup'
( declare -n c1=c2; declare -n c2=c1; export -n c1; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; export -n c1=5; echo "rc=$?"; declare -p c1 )

echo '=== the letter alone makes no array, so it changes nothing'
( declare -n c1=c2; declare -n c2=c1; export -a c1; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; readonly -A c1; echo "rc=$?"; declare -p c1 )
( export -a fresh; declare -p fresh )
( readonly -a fresh; declare -p fresh )

echo '=== the letter and a value together are a write, and a write does not give up'
( declare -n c1=c2; declare -n c2=c1; export -a c1=5; echo "rc=$?"; declare -p c1 c2 )
( declare -n c1=c2; declare -n c2=c1; export -A c1=5; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; readonly -a c1=5; echo "rc=$?"; declare -p c1 )
( declare -n c1=c2; declare -n c2=c1; export -a c1+=5; echo "rc=$?"; declare -p c1 )

echo '=== …and the cycle is gone once it has'
( declare -n c1=c2; declare -n c2=c1; export -a c1=5 2>/dev/null; c2[1]=z; declare -p c1 )

echo '=== a longer cycle is no different'
( declare -n a1=a2; declare -n a2=a3; declare -n a3=a1; export a1; echo "rc=$?" )
( declare -n a1=a2; declare -n a2=a3; declare -n a3=a1; export -a a1=5; declare -p a1 )

echo '=== a chain that resolves is silent, and marks the far end'
( w=5; declare -n r=w; export r; declare -p r w )
( w=5; declare -n r=w; readonly r=9; declare -p r w )
( w=5; declare -n r=w; export -a r=9; declare -p r w )
( w=5; declare -n r=w; export -n r; declare -p r w )

echo still here
