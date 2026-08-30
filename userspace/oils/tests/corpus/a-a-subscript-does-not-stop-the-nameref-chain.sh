# A write target that carries a subscript still follows the nameref chain.
#
# Nothing is *named* `arr[0]`, so a subscripted target is never itself a
# reference — but its base is an ordinary name and is one as readily as any
# other. `declare -n r=arr; read 'r[0]'` writes element 0 of `arr`, and it
# reaches everything else that belongs to `arr` on the way: the array's `-i`
# and `-u` value attributes apply to what lands in the element, a readonly
# `arr` refuses the write outright, and a name the shell maintains refuses it
# silently.
#
# The one thing the chain does not carry back is the name to blame. A refusal
# is spelled with the name the *writer* wrote — `r`, however many links deep
# `arr` is — where a *bare* target's refusal is spelled with the name the chain
# landed on. So the same readonly array is named two different ways depending
# on whether the operand had a subscript.

echo "=== the chain is followed, and the element lands on the target"
declare -a arr=(x y); declare -n r=arr
read -r 'r[1]' <<< NEW; echo "rc=$?"; declare -p arr

echo "=== an associative target is reached by key"
declare -A m=([k]=old); declare -n rm=m
read -r 'rm[k]' <<< NEW; echo "rc=$?"; declare -p m

echo "=== the target's value attributes apply"
declare -ai ia=(0 0); declare -n ri=ia
read -r 'ri[1]' <<< '3+4'; echo "rc=$?"; declare -p ia
declare -au ua=(a a); declare -n ru=ua
read -r 'ru[1]' <<< zz; echo "rc=$?"; declare -p ua

echo "=== a readonly target refuses the write"
declare -a ro=(x y); readonly ro; declare -n rr=ro
read -r 'rr[1]' <<< NEW; echo "rc=$?"; declare -p ro
declare -A rom=([k]=old); readonly rom; declare -n rrm=rom
read -r 'rrm[k]' <<< NEW; echo "rc=$?"; declare -p rom

echo "=== the refusal names the operand, not what it resolved to"
declare -a q=(x); readonly q; declare -n l1=q; declare -n l2=l1
read -r 'l2[0]' <<< NEW; echo "rc=$?"
read -r l2 <<< NEW; echo "rc=$?"

echo "=== a name the shell maintains refuses silently"
declare -n rg=GROUPS
read -r 'rg[0]' <<< NEW; echo "rc=$?"

echo "=== and a plain subscripted target is unchanged"
declare -a p=(x y); read -r 'p[1]' <<< NEW; echo "rc=$?"; declare -p p
declare -a pr=(x y); readonly pr; read -r 'pr[1]' <<< NEW; echo "rc=$?"
