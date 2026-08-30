# `@` and `*` are perfectly good *bytes* in an arithmetic subscript. bash's
# parser accepts them; it is the array lookup that has no index to make of them,
# so the refusal is a complaint rather than an error — `a[@]: bad array
# subscript`, untagged, with the operand worth 0 and the expression carrying on.
#
# Three things follow from where the check sits:
#
#   * an **associative** array is unaffected, because there the subscript is a
#     string key and `@` is an ordinary one;
#   * the match is on the *exact* bytes, so `a[ @]` and `a['@']` are ordinary
#     expressions and fail as ordinary syntax errors;
#   * it is a *lookup*, so it happens once per read and once per store — and it
#     happens after the name has been resolved, which is why a circular nameref
#     gets its own warnings in first.

a=(1 2 3)
declare -A m=([k]=4 [@]=7)

echo '=== a read is worth 0 and says so once'
echo "[$(( a[@] ))] rc=$?"
echo "[$(( a[*] ))] rc=$?"
echo "[$(( a[@]+1 ))] rc=$?"
echo "[$(( a[a[@]] ))] rc=$?"
echo "[$(( nosuch[@] ))] rc=$?"
(( a[@] )); echo "rc=$?"

echo '=== a store is dropped, and the array is untouched'
(( a[@] = 5 )); echo "rc=$?"; declare -p a
(( a[*] = 9 )); echo "rc=$?"; declare -p a
let "a[@] = 5"; echo "rc=$?"

echo '=== a read-modify-write pays for both halves'
(( a[@]++ )); echo "rc=$?"; declare -p a
(( a[@] += 2 )); echo "rc=$?"; declare -p a

echo '=== an unevaluated operand is silent'
echo "[$(( 1 ? 7 : a[@] ))] rc=$?"
echo "[$(( 0 && a[@] ))] rc=$?"

echo '=== an associative array takes it as an ordinary key'
echo "[$(( m[@] ))] rc=$?"
(( m[@] = 5 )); echo "rc=$?"; declare -p m

echo '=== only the exact bytes, so these are syntax errors'
echo "[$(( a[ @] ))] rc=$?"
echo "[$(( a[@ ] ))] rc=$?"
echo "[$(( a['@'] ))] rc=$?"
echo "[$(( a[@@] ))] rc=$?"
k=@; echo "[$(( a[k] ))] rc=$?"

echo '=== the name is blamed as written, not as a reference resolves it'
declare -n r=a
echo "[$(( r[@] ))] rc=$?"
(( r[@] = 5 )); echo "rc=$?"; declare -p a
( declare -a q=(1 2); declare -n e=q[0]; echo "[$(( e[@] ))] rc=$?" )

echo '=== a circular chain is reported by the lookup, before the refusal'
( declare -n c1=c2; declare -n c2=c1; echo "[$(( c1[@] ))] rc=$?" )
( declare -n c1=c2; declare -n c2=c1; (( c1[@] = 5 )); echo "rc=$?"; declare -p c1 )

echo '=== an empty subscript is a different rule, and keeps it'
echo "[$(( a[] ))] rc=$?"
(( a[] = 5 )); echo "rc=$?"

echo still here
