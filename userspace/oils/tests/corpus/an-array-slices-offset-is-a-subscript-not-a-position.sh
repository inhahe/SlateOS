# `${a[@]:off}` selects every element whose *subscript* is at least `off`, not
# the elements from position `off` onward. The two readings agree on the usual
# dense array and part company the moment the array has a gap: with indices
# 0, 5 and 9, `${s[@]:2}` is `y z` and `${s[@]:6}` is `z`. The length stays a
# count of elements — `${s[@]:1:2}` is two elements six subscripts apart — and a
# negative offset becomes a subscript by adding one past the highest index, so
# `${s[@]: -4}` is subscript 6.

declare -a s=(); s[0]=x; s[5]=y; s[9]=z
declare -a t=(); t[3]=p; t[4]=q; t[7]=r
declare -a d=(a b c d)
declare -a e=()
v=scalar

show() { printf '  %-12s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

echo "### the sparse array's own index space (0 5 9)"
echo "  keys ${!s[@]}"
for o in 0 1 2 4 5 6 8 9 10 99; do
  eval "show 's:$o' \${s[@]:$o}"
done

echo "### a gap that does not start at zero (3 4 7)"
for o in 0 1 2 3 4 5 6 7 8; do
  eval "show 't:$o' \${t[@]:$o}"
done

echo "### the length counts elements, not subscripts"
for o in 0 1 5 6 9; do
  for l in 0 1 2 3; do
    eval "show 's:$o:$l' \${s[@]:$o:$l}"
  done
done

echo "### a negative offset is one past the highest subscript"
for o in "-1" "-2" "-3" "-4" "-5" "-9" "-10" "-11" "-99"; do
  eval "show 's: $o' \${s[@]: $o}"
done
show 's:-2:1'  ${s[@]: -2:1}
show 's:-9:2'  ${s[@]: -9:2}
show 's:-10:1' ${s[@]: -10:1}

echo "### a dense array and the positionals are unchanged by all this"
for o in 0 1 3 4 5; do
  eval "show 'd:$o' \${d[@]:$o}"
done
for o in "-1" "-2" "-4" "-5"; do
  eval "show 'd: $o' \${d[@]: $o}"
done
show 'd:1:2'   ${d[@]:1:2}
show 'd:-2:1'  ${d[@]: -2:1}
set -- p q r
show '@:0'     ${@:0}
show '@:1'     ${@:1}
show '@:2:2'   ${@:2:2}
show '@: -1'   ${@: -1}
show '@: -9'   ${@: -9}
show '*:1'     ${*:1}

echo "### the star spelling, the key list and an empty array"
show 's*:5'    ${s[*]:5}
show 's*:6'    ${s[*]:6}
show '!s:5'    ${!s[@]:5}
show '!s:6'    ${!s[@]:6}
show 'e:0'     ${e[@]:0}
show 'e:1'     ${e[@]:1}
show 'e: -1'   ${e[@]: -1}
show 'nosuch'  ${zz[@]:0}

echo "### a quoted slice, and one glued to text"
show 'q s:2'   "${s[@]:2}"
show 'A s:2 B' A${s[@]:2}B
a=${s[@]:2};   echo "  a=\${s[@]:2} [$a]"

echo "### a negative length is still fatal"
(echo ${s[@]:0:-1}); echo "  rc=$?"

echo "### the offset and the length are arithmetic"
i=5
show 's:i'     ${s[@]:i}
show 's:i-3'   ${s[@]:i-3}
show 's:0:i-4' ${s[@]:0:i-4}
