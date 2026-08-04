# An associative array has no subscripts to count, and bash does not count its
# positions the ordinary way either. A non-negative offset is one-based — except
# that 0 and 1 both name the first element, so the run skipped is `max(off-1,0)`
# — a negative one is measured against `n+1` rather than `n`, and a length of 0
# yields *one* element rather than none. The two quirks are independent:
# `${m[@]:2:0}` is one element, and it is the second.
#
# The order the elements come in is a different question, answered in
# `an-associative-array-iterates-in-bashs-hash-order.sh` — but now that it *is*
# answered, everything below reports the values as well as the count, so a slice
# of a four-key array names which four and not merely how many. (It could not,
# while osh iterated its map by key and bash walked its hash.)

declare -A one=([solo]=s)
declare -A two=([a]=1 [b]=2)
declare -A four=([k1]=v1 [k2]=v2 [k3]=v3 [k4]=v4)
declare -A empty=()

# `j++` in the length runs only if the length was evaluated.
# `set --` replaces the function's own positionals, so the labels are saved
# before the slice is taken — otherwise `$2` below would report a *value*, and
# the case would be asking about bash's hash order after all.
ran() { j=0; eval ": \${$1:$2:j++}"; printf ' %s:j=%d' "$2" "$j"; }
cnt() { local o=$2; eval "set -- \${$1:$2}"; printf ' %s=%d' "$o" "$#"; printf '<%s>' "$@"; }
cl()  { local o=$2 l=$3; eval "set -- \${$1:$2:$3}"; printf ' %s:%s=%d' "$o" "$l" "$#"; printf '<%s>' "$@"; }

echo "### a one-key array, where the order is not a question"
show() { printf '  %-14s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }
for o in 0 1 2 3; do eval "show 'one:$o' \${one[@]:$o}"; done
for o in "-1" "-2" "-3" "-4"; do eval "show 'one: $o' \${one[@]: $o}"; done
show 'one:0:0'  ${one[@]:0:0}
show 'one:1:0'  ${one[@]:1:0}
show 'one:0:1'  ${one[@]:0:1}
show 'one:1:9'  ${one[@]:1:9}
show 'one:2:0'  ${one[@]:2:0}
show 'one: -1:0' ${one[@]: -1:0}
show 'one: -2:0' ${one[@]: -2:0}
show 'q one:1'  "${one[@]:1}"
show 'q one:2'  "${one[@]:2}"
show 'one*:1'   ${one[*]:1}

echo "### which fields, for two and four keys"
printf '  two  '; for o in 0 1 2 3 4; do cnt 'two[@]' "$o"; done; echo
printf '  two  '; for o in "-1" "-2" "-3" "-4" "-5"; do cnt 'two[@]' " $o"; done; echo
printf '  four '; for o in 0 1 2 3 4 5 6 99; do cnt 'four[@]' "$o"; done; echo
printf '  four '; for o in "-1" "-2" "-4" "-5" "-6" "-7" "-9"; do cnt 'four[@]' " $o"; done; echo

echo "### a length of zero still yields one"
for o in 0 1 2 3 4 5; do
  printf '  four o=%-2s' "$o"
  for l in 0 1 2 3 9; do cl 'four[@]' "$o" "$l"; done
  echo
done
for o in "-1" "-2" "-4" "-5"; do
  printf '  four o=%-3s' "$o"
  for l in 0 1 2 9; do cl 'four[@]' " $o" "$l"; done
  echo
done

echo "### the offset decides whether the length is read at all"
printf '  one  '; for o in 0 1 2 3; do ran 'one[@]' "$o"; done; echo
printf '  one  '; for o in "-1" "-2" "-3"; do ran 'one[@]' " $o"; done; echo
printf '  two  '; for o in 0 1 2 3 4; do ran 'two[@]' "$o"; done; echo
printf '  two  '; for o in "-1" "-3" "-4" "-5"; do ran 'two[@]' " $o"; done; echo
printf '  four '; for o in 0 3 4 5 6 99; do ran 'four[@]' "$o"; done; echo
printf '  four '; for o in "-4" "-5" "-6" "-7"; do ran 'four[@]' " $o"; done; echo
printf '  empt '; for o in 0 1 2; do ran 'empty[@]' "$o"; done; echo

echo "### so a negative length is fatal only from inside"
for o in 0 1 2 4 5; do (eval "echo \${four[@]:$o:-1}"); echo "    four:$o rc=$?"; done
for o in "-5" "-6"; do (eval "echo \${four[@]: $o:-1}"); echo "    four: $o rc=$?"; done
for o in 0 1; do (eval "echo \${empty[@]:$o:-1}"); echo "    empty:$o rc=$?"; done

echo "### the star spelling joins, so it is one field or none"
for o in 0 2 4 5; do eval "set -- \"\${four[*]:$o:2}\""; printf ' %s=%d' "$o" "$#"; printf '<%s>' "$@"; done; echo
for o in 0 2 4 5; do eval "set -- \"\${four[@]:$o:2}\""; printf ' %s=%d' "$o" "$#"; printf '<%s>' "$@"; done; echo

echo "### an empty and a missing associative array"
show 'empty:0'   ${empty[@]:0}
show 'q empty:0' "${empty[@]:0}"
show 'nosuch:0'  ${nope[@]:0}

echo "### the offset and the length are arithmetic"
k=3
printf '  four '; cl 'four[@]' 'k' '1'; cl 'four[@]' 'k-1' 'k-2'; echo
