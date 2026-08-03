# An *indexed* array literal expands its elements the way command words are
# expanded — split on `$IFS`, globbed, brace-expanded — so one element can
# become any number. An **associative** one does not: each element is expanded
# in assignment context, one element in and exactly one out, and only the tilde
# survives of the extra expansions. That is not a detail of the diagnostics; it
# decides which keys the array ends up with.
#
#   * `m=(a $s b)` with `s='x y'` is three elements, so it pairs `a` with the
#     whole `x y` and leaves `b` empty. The same text in an indexed literal is
#     four.
#   * `{a,b}`, `x{1..2}` and a glob that matches files right there are all
#     literal key text.
#   * `"${a[@]}"` contributes *one* element holding the joined value, not one
#     per array element.
#   * A `[k]=v` element expands its two halves separately, and in pair mode the
#     reassembled `[k]=v` is then an ordinary key.
#
# The rule is the same for a `declare -A` operand and for a bare `m=(…)`, and
# it is the same for `+=`.
#
# (Key *order* is bash's hash order and osh's is not it, so nothing here prints
# a multi-key `declare -p`; the keys are asked about by name instead.)

s='x y'
a=(p q)

echo "=== one element in, one element out"
declare -A m=(a $s b)
echo "  n=${#m[@]} a=[${m[a]}] b=[${m[b]}]"
declare -a n=(a $s b)
echo "  indexed n=${#n[@]}: ${n[0]}|${n[1]}|${n[2]}|${n[3]}"

echo "=== no brace expansion"
declare -A br=({a,b} 1)
echo "  n=${#br[@]} key=[${br['{a,b}']}]"
declare -A br2=(x{1..2} 1)
echo "  n=${#br2[@]} key=[${br2['x{1..2}']}]"

echo "=== no globbing, even with the files right here"
: > aa.zzq
: > bb.zzq
declare -A g=(*.zzq 1)
echo "  n=${#g[@]} key=[${g['*.zzq']}]"
declare -a gi=(*.zzq)
echo "  indexed n=${#gi[@]}"

echo "=== a quoted array expansion is one element, joined"
declare -A j=("${a[@]}")
echo "  n=${#j[@]} key=[${j['p q']}]"

echo "=== a keyed element expands its halves; in pair mode it is a plain key"
k=dk
declare -A kv=([$k]=$s)
echo "  n=${#kv[@]} dk=[${kv[dk]}]"
declare -A pm=(x 1 [y]=2)
echo "  n=${#pm[@]} lit=[${pm['[y]=2']}]"

echo "=== the bare form and += follow the same rule"
declare -A b1
b1=($s v)
echo "  n=${#b1[@]} key=[${b1['x y']}]"
b1+=(more $s)
echo "  n=${#b1[@]} more=[${b1[more]}]"

echo "=== the trace shows the elements as expanded"
set -x
declare -A t=(a $s b)
set +x
echo "=== done"
