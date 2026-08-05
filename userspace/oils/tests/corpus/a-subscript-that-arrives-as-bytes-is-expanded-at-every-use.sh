# A subscript can reach the shell as **bytes** rather than as a parsed word —
# through a *name operand* (`printf -v 'a[i]'`, `read 'a[i]'`, `declare
# 'a[i]=v'`) or through a *nameref target* (`declare -n r='a[i]'`). bash
# expands both, and expands them **at every use**, so a reference is late-bound:
# `declare -n r='n[$i]'` names a different element as `$i` moves, and a command
# substitution inside one runs again for each read.
#
# Which language the subscript is written in depends on the array, exactly as it
# does for a subscript that was *written*:
#
#   * indexed — bash's `expand_arith_string` in a double-quoted context, so
#     `$…` expands and `"` is removed, while `'` and `\` stay and the
#     arithmetic evaluator then refuses them;
#   * associative — ordinary word expansion, so `m['kk']` and `m["kk"]` are
#     both the key `kk`.
#
# An expression that will not evaluate is reported and stores/reads nothing,
# dropping the rest of the parse unit as any other subscript error does.

p() { printf '%-14s ' "$1"; shift; printf '<%s>' "$@"; echo; }

echo "=== a reference is late-bound: the subscript is read again each time"
n=(a b c d e)
declare -n r='n[$i]'
i=1; echo "i=1 -> $r"
i=3; echo "i=3 -> $r"
i=0; echo "i=0 -> $r"

echo "=== …including the command substitution in it"
declare -n q='n[$(echo 2 >&2; echo 2)]'
echo "first  -> $q"
echo "second -> $q"

echo "=== and its side effects really happen"
declare -n e='n[j=4]'
echo "-> $e j=$j"

echo "=== arithmetic reads it the same way"
v=(10 20 30 40)
declare -n a='v[$i]'
i=1; echo "A $((a))"
i=3; echo "B $((a))"
i=2; (( a += 5 )); p 'v' "${v[@]}"

echo "=== an indexed subscript is an arithmetic string, not a word"
p 'dollar'  "$(declare -n t='n[$((1+1))]'; echo "$t")"
p 'dquote'  "$(declare -n t='n["1"]'; echo "$t")"
declare -n sq="n['1']"
echo "single -> ${sq}"; echo "not reached 1"
echo "line after"
declare -n bs='n[\61]'
echo "esc -> ${bs}"; echo "not reached 2"
echo "line after"

echo "=== an associative key is an ordinary word, so both quotes come off"
declare -A m=([kk]=K [zz]=Z [k k]=S)
k=zz
declare -n md='m[$k]'
declare -n mq='m["kk"]'
declare -n ms="m['kk']"
declare -n msp='m["k k"]'
p 'by var'  "$md"
p 'dquoted' "$mq"
p 'squoted' "$ms"
p 'spaced'  "$msp"
k=kk; p 'rebound' "$md"

echo "=== a name operand carries one too"
printf -v 'n[$i]' X; echo "printf status=$?"; p 'n' "${n[@]}"
i=4
read -r 'n[$i]' <<< Y; echo "read status=$?"; p 'n' "${n[@]}"
printf -v 'n["0"]' Z; echo "printf status=$?"; p 'n' "${n[@]}"
printf -v 'm[$k]' W; echo "printf status=$?"; p 'm[kk]' "${m[kk]}"

echo "=== so does a declare operand"
i=2
declare 'n[$i]=D'; echo "declare status=$?"; p 'n' "${n[@]}"
declare 'n["3"]=E'; echo "declare status=$?"; p 'n' "${n[@]}"
f() { local 'n[$i]=L'; p 'in f' "${n[@]}"; }
f

echo "=== a write through a reference lands where the subscript now points"
i=1; r=RR; p 'n' "${n[@]}"
i=4; r=SS; p 'n' "${n[@]}"

echo "=== an expression that will not evaluate reads nothing and reports"
declare -n bad='n[1+]'
echo "got <$bad>"; echo "not reached 3"
echo "line after"

echo "=== a whole-array token is still refused before anything is expanded"
declare -n g='n[*]'
echo "join <${g}>"
s='*'
declare -n st='n[$s]'
echo "star <${st}>"; echo "not reached 4"
echo "line after"

echo '=== ${#r} and set -u, with the subscript live'
i=1
p 'len' "${#r}"
( set -u; p 'len -u' "${#r}" )
i=9
p 'unset'  "${r-DEF}"
