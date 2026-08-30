# An associative literal turns two kinds of element away, and both name the
# offending element by the text it was *written* with rather than by what it
# expanded to.
#
#   * An **unsubscripted** element where the first one was subscripted — the
#     first element picks the mode for the whole list. bash refuses this one
#     before expanding it at all, so `m=([a]=1 $s)` reports `$s` and not the
#     `x y` it holds, and a `$(…)` or an assigning `${x=…}` in it never runs.
#   * An **empty key**, which has nowhere to live. Here the element *was*
#     expanded — it had to be, to know the key is empty — so what is named is
#     the expanded text, requoted.
#
# Which spelling of "as written" is used depends on the layer that read it: a
# `declare`/`local` operand is re-parsed by the builtin from a requoted form, so
# its diagnostics are quoted (`'$s'`, `['']='v'`), while a bare `m=(…)` names
# the raw source (`$s`, `[$e]=v`).
#
# Neither is fatal. The element is skipped, every other element still binds, and
# the status stays 0.

s='x y'
e=

echo "=== an unsubscripted element in subscript mode"
{ declare -A k=([a]=1 $s [b]=2); } 2>&1
echo "  s=$? n=${#k[@]} a=${k[a]} b=${k[b]}"
declare -A k2
{ k2=([a]=1 $s [b]=2); } 2>&1
echo "  s=$? n=${#k2[@]}"

echo "=== and it is never expanded"
{ declare -A k3=([a]=1 ${x=set}); } 2>&1
echo "  x=[$x]"
declare -A k4
{ k4=([a]=1 ${y=set}); } 2>&1
echo "  y=[$y]"

echo "=== an empty key, from a declare operand"
{ declare -A m=([a]=1 [""]=v [b]=2); } 2>&1
echo "  s=$? n=${#m[@]} a=${m[a]} b=${m[b]}"
{ declare -A m2=(k1 v1 "" x k2 v2); } 2>&1
echo "  s=$? n=${#m2[@]} k1=${m2[k1]} k2=${m2[k2]}"

echo "=== an empty key, from a bare literal"
declare -A b1
{ b1=([$e]=v); } 2>&1
echo "  s=$? n=${#b1[@]}"
declare -A b2
{ b2=("" v); } 2>&1
echo "  s=$? n=${#b2[@]}"
declare -A b3
{ b3+=([$e]=v); } 2>&1
echo "  s=$? n=${#b3[@]}"

echo "=== the trace shows what each mode did with the element"
set -x
declare -A t=([a]=1 $s)
declare -A t2=("" v)
set +x
echo "=== done"
