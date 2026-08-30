# `${v[@]:off:len}` on a variable that is no array does not make a list of one —
# bash falls through to the plain `${v:off:len}` substring operator, so
# `v=scalar` answers `${v[@]:1}` with `calar` and `${v[@]: -1}` with `r`, and a
# negative length is an absolute end position rather than the fatal error an
# array's slice makes of it. The offset is measured in characters and decides
# whether there is a field at all: a start exactly at the end is one *empty*
# field, a start past it is *no* field.

v=scalar
w=
declare -i i=12345
declare -x X=exported
declare -n g=v
declare -a a=(x y z)

show() { printf '  %-14s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

echo "### the written-out substring, for comparison"
show 'v:1'      ${v:1}
show 'v:1:3'    ${v:1:3}
show 'v: -3'    ${v: -3}
show 'v:1:-1'   ${v:1:-1}

echo "### the same answers through [@] and [*]"
for o in 0 1 2 5 6 7 8 99; do
  eval "show 'v:$o' \${v[@]:$o}"
done
for o in "-1" "-3" "-5" "-6" "-7" "-8" "-99"; do
  eval "show 'v: $o' \${v[@]: $o}"
done
show 'v:1:3'    ${v[@]:1:3}
show 'v*:1:3'   ${v[*]:1:3}
show 'v:0:0'    ${v[@]:0:0}
show 'v:2:99'   ${v[@]:2:99}
show 'v: -4:2'  ${v[@]: -4:2}

echo "### a negative length is an end offset, not a fatal error"
show 'v:1:-1'   ${v[@]:1:-1}
show 'v:0:-2'   ${v[@]:0:-2}
show 'v: -4:-1' ${v[@]: -4:-1}
show 'v*:1:-1'  ${v[*]:1:-1}
echo "  but one that puts the end before the start still is:"
(echo ${v[@]:0:-99}); echo "  rc=$?"
(echo ${v[*]:0:-99}); echo "  rc=$?"
(echo ${v:0:-99});    echo "  rc=$?"
echo "  and an offset past the end never looks at the length:"
(echo ${v[@]:99:-99}); echo "  rc=$?"
(echo ${v[@]: -99:-99}); echo "  rc=$?"
j=0; : ${v[@]:99:j++};  echo "  v:99 j=$j"
j=0; : ${v[@]:6:j++};   echo "  v:6  j=$j"
j=0; : ${v[@]:7:j++};   echo "  v:7  j=$j"
j=0; : ${v[@]: -99:j++}; echo "  v:-99 j=$j"

echo "### quoted, where the empty-field boundary shows"
show 'q v:5'    "${v[@]:5}"
show 'q v:6'    "${v[@]:6}"
show 'q v:7'    "${v[@]:7}"
show 'q v: -6'  "${v[@]: -6}"
show 'q v: -7'  "${v[@]: -7}"
show 'q v:0:0'  "${v[@]:0:0}"
show 'q v*:99'  "${v[*]:99}"
show 'q a:99'   "${a[@]:99}"
show 'q a:0:0'  "${a[@]:0:0}"

echo "### an empty scalar has one position, not none"
show 'q w:0'    "${w[@]:0}"
show 'q w:1'    "${w[@]:1}"
show 'q w: -1'  "${w[@]: -1}"
show 'u w:0'    ${w[@]:0}

echo "### an integer, an export and a nameref are scalars too"
show 'i:1:2'    ${i[@]:1:2}
show 'X:0:2'    ${X[@]:0:2}
show 'g:1'      ${g[@]:1}
show 'g:1:3'    ${g[@]:1:3}

echo "### an unset name is still nothing"
show 'zz:0'     ${zz[@]:0}
show 'zz:1'     ${zz[@]:1}
show 'q zz:0'   "${zz[@]:0}"

echo "### an explicit subscript on a scalar reads the value, then slices it"
show 'v0:1'     ${v[0]:1}
show 'v0:1:3'   ${v[0]:1:3}

echo "### assignment and glued contexts"
b=${v[@]:1:3};  echo "  b=\${v[@]:1:3} [$b]"
show 'A..B'     A${v[@]:1:3}B

echo "### the offset and the length are arithmetic"
k=2
show 'v:k'      ${v[@]:k}
show 'v:k-2:k'  ${v[@]:k-2:k}
