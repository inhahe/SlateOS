# The same "once per walk" rule arithmetic follows, on the expansion side. A
# circular chain is reported once for every time bash walks it, and what decides
# the count is the *shape* of the read:
#
#   a plain read      walks once     `${c1}`, and every modifier on one
#   a subscripted one walks twice    once to find the array, once to read it
#
# `[@]` and `[*]` are subscripts like any other here, however the read is
# spelled — quoted or not, joined or split, sliced, transformed, or reached
# through a pointer.
#
# Two shapes sit outside that, and they are the two that ask about the array
# rather than about its contents: `${#a[@]}` (how many) and `${!a[@]}` (which
# keys) walk once. That makes `${#…}` read the opposite way round from
# everything else — `${#c1}` walks *twice* where `${c1}` walks once, and
# `${#c1[0]}` walks once where `${c1[0]}` walks twice — because the length of a
# whole parameter and the length of one element are answered by different
# routes, and each route walks what it walks.
#
# Nothing here changes a value: every one of these reads is empty, and the
# counts are the whole of the difference. Each case is a subshell so every one
# starts from the same untouched cycle.

cyc='declare -n c1=c2; declare -n c2=c1;'

echo '=== a plain read walks once, and a modifier on it adds nothing'
( eval "$cyc"; echo "[$c1]" )
( eval "$cyc"; echo "[${c1}]" )
( eval "$cyc"; echo "[${c1:-d}]" )
( eval "$cyc"; echo "[${c1-d}]" )
( eval "$cyc"; echo "[${c1:+x}]" )
( eval "$cyc"; echo "[${c1#x}]" )
( eval "$cyc"; echo "[${c1^^}]" )
( eval "$cyc"; echo "[${c1@Q}]" )
( eval "$cyc"; echo "[${c1/a/b}]" )
( eval "$cyc"; echo "[${c1:0:1}]" )

echo '=== a subscript costs a second walk, whatever the modifier'
( eval "$cyc"; echo "[${c1[0]}]" )
( eval "$cyc"; echo "[${c1[1]}]" )
( eval "$cyc"; echo "[${c1[0]:-d}]" )
( eval "$cyc"; echo "[${c1[0]:+x}]" )
( eval "$cyc"; echo "[${c1[0]#x}]" )
( eval "$cyc"; echo "[${c1[0]^^}]" )
( eval "$cyc"; echo "[${c1[0]@Q}]" )
( eval "$cyc"; echo "[${c1[0]/a/b}]" )
( eval "$cyc"; echo "[${c1[0]:0:1}]" )

echo '=== [@] and [*] are subscripts too, however the read is spelled'
( eval "$cyc"; echo "[${c1[@]}]" )
( eval "$cyc"; echo [${c1[@]}] )
( eval "$cyc"; echo "[${c1[*]}]" )
( eval "$cyc"; echo [${c1[*]}] )
( eval "$cyc"; x=("${c1[@]}"); declare -p x )
( eval "$cyc"; for i in "${c1[@]}"; do echo "i=$i"; done; echo loop-done )
( eval "$cyc"; echo "[${c1[@]:0:1}]" )
( eval "$cyc"; echo "[${c1[@]#x}]" )
( eval "$cyc"; echo "[${c1[@]@Q}]" )
( eval "$cyc"; echo "[${c1[@]@}]" )
( eval "$cyc"; echo "[${c1[@]:-d}]" )
( eval "$cyc"; p="c1[@]"; echo "[${!p}]" )
( eval "$cyc"; case "${c1[@]}" in *) echo matched;; esac )

echo '=== the two questions about the array itself keep the single walk'
( eval "$cyc"; echo "[${#c1[0]}]" )
( eval "$cyc"; echo "[${#c1[@]}]" )
( eval "$cyc"; echo "[${#c1[*]}]" )
( eval "$cyc"; echo "[${!c1[@]}]" )
( eval "$cyc"; echo "[${!c1[*]}]" )

echo '=== …which is why the unsubscripted length is the one that walks twice'
( eval "$cyc"; echo "[${#c1}]" )

echo '=== @a and @A ask after the variable, which is two more walks on top'
( eval "$cyc"; echo "[${c1@a}]" )
( eval "$cyc"; echo "[${c1@A}]" )
( eval "$cyc"; echo "[${c1[0]@a}]" )
( eval "$cyc"; echo "[${c1[0]@A}]" )
( eval "$cyc"; echo "[${c1[@]@a}]" )
( eval "$cyc"; echo "[${c1[@]@A}]" )

echo '=== a chain that resolves is silent, at every one of those shapes'
( t=v; declare -n r=t; echo "[$r][${#r}][${r[0]}][${#r[0]}][${r@a}][${r@A}]" )
( u=(p q); declare -n r=u; echo "[${r[@]}][${#r[@]}][${!r[@]}][${r[1]}][${r[@]@a}]" )

echo still here
