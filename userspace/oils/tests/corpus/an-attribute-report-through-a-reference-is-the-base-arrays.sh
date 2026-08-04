# `${!r@a}` and `${!r@A}` ask about the *variable* the reference resolved to,
# and that name may carry a subscript of its own — `r='n[1]'` resolves to the
# text `n[1]`. The attributes reported are then the base array's, and `@A`
# renders the element the reference read under the base's name:
# `declare -a n='by'`. A nameref base is followed first, and a whole-array
# referent (`r='n[@]'`) is the array itself, one report per element.

declare -a n=(ax by cz)
declare -A m=([k]=v)
declare -a x=(p q)
declare -n b=c
declare -a c=(u v)
declare -n g=n
declare -n h='n[1]'
s=plain
declare -i i=7
declare -x X=exported
declare -r RO=1

show() { printf '  %-16s' "$1"; shift; printf '<%s>' "$@"; printf '\n'; }

echo "### written out, for comparison"
show 'n[1]@a'    ${n[1]@a}
show 'n[1]@A'    ${n[1]@A}
show 'm[k]@a'    ${m[k]@a}
show 'm[k]@A'    ${m[k]@A}
show 'n@a'       ${n@a}
show 'n@A'       ${n@A}
show 'b[1]@a'    ${b[1]@a}
show 'b[1]@A'    ${b[1]@A}

echo "### through a reference"
for ref in 'n[1]' 'm[k]' 'n' 's' 'i' 'X' 'RO' 'nosuch' 'nosuch[1]' 'n[9]' 'n[-1]' 'b[1]'; do
  r=$ref
  printf '  %-12s' "$ref"
  printf 'a<%s>' ${!r@a}
  printf ' A<%s>' ${!r@A}
  printf '\n'
done

echo "### the other transforms still read the element"
r='n[1]'
show '!r@Q'      ${!r@Q}
show '!r@P'      ${!r@P}
show '!r@U'      ${!r@U}
show '!r@E'      ${!r@E}
show '!r'        ${!r}

echo "### a whole-array referent reports once per element"
for ref in 'n[@]' 'n[*]' 'm[@]'; do
  r=$ref
  printf '  %-12s' "$ref"
  printf 'a<%s>' ${!r@a}
  printf ' A<%s>' ${!r@A}
  printf '\n'
done

echo "### namerefs"
show 'g@a'       ${g@a}
show '!g@a'      ${!g@a}
show 'g@A'       ${g@A}
show '!g@A'      ${!g@A}
show 'h@a'       ${h@a}
show '!h@a'      ${!h@a}
show 'h@A'       ${h@A}
show '!h@A'      ${!h@A}

echo "### quoted, and a pointer that reaches nothing"
r='n[1]'
show 'q !r@a'    "${!r@a}"
show 'q !r@A'    "${!r@A}"
show 'nowhere@a' ${!x[5]@a}
show 'nowhere@A' ${!x[5]@A}
show 'nowhere@Q' ${!x[5]@Q}

echo "### set -u names the reference, not the variable"
r='n[9]'
(set -u; echo "  [${!r@a}]"); echo "  rc=$?"
(set -u; echo "  [${!r@A}]"); echo "  rc=$?"
r='n[1]'
(set -u; echo "  [${!r@a}]"); echo "  rc=$?"
