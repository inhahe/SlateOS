# An array element that is not there is an unset parameter like any other, so
# under `set -u` reading one faults. The complaint names the reference as the
# source wrote it — the subscript goes out *unevaluated* (`x[1+1]`, `x[n]`,
# `m[$(echo k)]`) and a nameref is quoted as the writer typed it (`r[5]`, not
# the `a[5]` it resolved to). A subscript already called bad says both things,
# in that order. Whole-array forms (`[@]`, `[*]`), the default-value family and
# a reference that lands on a real element are all unaffected; an indirection
# through an absent element is named `!x[5]`, and `${!@}` with no positionals
# indirects through nothing and so faults even though `$@` never does.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( set -u; eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== an absent element faults, named as written"
q 'x=(a b); echo "[${x[5]}]"'
q 'echo "[${nosuch[1]}]"'
q 'declare -A m; m[k]=v; echo "[${m[j]}]"'
q 'x=(); echo "[${x[0]}]"'
q 'x=(a b); echo "[${x[5]}]"; echo tail'

echo "=== the subscript is quoted from its source, not its value"
q 'x=(a b); echo "[${x[1+1]}]"'
q 'x=(a b); n=5; echo "[${x[n]}]"'
q 'declare -A m; m[k]=v; echo "[${m[$(echo j)]}]"'
q 'x=(a b); i=5; echo "[${x[i++]}]"'

echo "=== a bad subscript says both things"
q 'x=(a b); echo "[${x[-5]}]"'
q 'declare -A m; m[k]=v; echo "[${m[""]}]"'

echo "=== a nameref is quoted as the reference, not as its target"
q 'a=(p q); declare -n r=a; echo "[${r[5]}]"'
q 'declare -n r=nosuch; echo "[${r[0]}]"'
q 'a=(p q); declare -n r=a; echo "[${r[1+1]}]"'
q 'a=(p q); declare -n r=a; echo "[${r[5]^^}]"'
q 'a=(p q); declare -n r=a; echo "[${r[0]}]"'

echo "=== a modifier does not excuse it either"
q 'x=(a b); echo "[${x[5]^^}]"'
q 'declare -A m; m[k]=v; echo "[${m[j]#x}]"'

echo "=== but the default-value family still does"
q 'x=(a b); echo "[${x[5]:-d}]"'
q 'x=(a b); echo "[${x[5]:+y}]"'
q 'x=(a b); echo "[${x[@]:5}]"'

echo "=== and a whole-array or present-element read is unaffected"
q 'x=(a b); echo "[${x[@]}][${x[*]}][${x[1]}]"'
q 'x=(""); echo "[${x[0]}]"'
q 'x=; echo "[${x[0]}]"'
q 'x=scalar; echo "[${x[0]}][${x[@]}]"'
q 'declare -A m; m[k]=v; echo "[${m[k]}][${m[@]}]"'

echo "=== an indirection through an absent element is named as the reference"
q 'x=(a b); echo "[${!x[5]}]"'
q 'x=(a b); echo "[${!x[5]^^}]"'
q 'x=(a b); echo "[${!x[5]:-d}]"'
q 'declare -A m; m[k]=v; echo "[${!m[j]}]"'
q 'set -- ; echo "[${!@}]"'
q 'set -- ; echo "[${!*}]"'
q 'set -- ; echo "[${!@:-d}]"'
q 'v=z; set -- v; echo "[${!@}]"'
echo "=== done"
