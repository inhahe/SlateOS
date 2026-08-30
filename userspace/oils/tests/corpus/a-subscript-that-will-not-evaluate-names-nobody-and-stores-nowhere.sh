# A subscript that will not evaluate: bash names nobody but the expression, and
# stores nowhere.
#
# Two rules travel together here. The diagnostic is *bare* — bash's
# `array_expand_index` clears `this_command_name` around the evaluation, so a
# subscript error is never blamed on the builtin that asked for it, even inside
# `declare`/`let`/`unset`/`printf -v`, while a bad `-i` *value* in the very same
# builtin still is. And the store is *abandoned* — there is no element named, so
# nothing is written; falling back on element 0 would corrupt the array a
# mistyped subscript never meant to touch.

echo '=== a name operand: the diagnostic is bare and the array is untouched'
n=(a b c)
printf -v 'n[1+]' X
echo "printf status=$?"
echo "n=(${n[@]}) len=${#n[@]}"

n=(a b c)
read -r 'n[2+]' <<<Y
echo "read status=$?"
echo "n=(${n[@]}) len=${#n[@]}"

echo '=== a written assignment agrees'
n=(a b c)
n[1+]=W
echo "assign status=$?"
echo "n=(${n[@]})"

echo '=== a declare operand'
n=(a b c)
declare 'n[4+]=Z'
echo "declare status=$?"
echo "n=(${n[@]})"

f() { local 'n[5+]=Z'; echo "local status=$?"; echo "n=(${n[@]})"; }
n=(a b c)
f

echo '=== a read of one'
n=(a b c)
echo "${n[1+]}"
echo "read status=$?"
declare v="${n[2+]}"
echo "in-declare status=$?"

echo '=== unset, let and (( are bare too'
n=(a b c)
unset 'n[9+]'
echo "unset status=$?"
let 'n[7+]=1'
echo "let status=$?"
(( n[8+] = 1 ))
echo "arith status=$?"
echo "n=(${n[@]})"

echo '=== a bad -i value is still tagged with its builtin'
declare -i p=5+
echo "declare status=$?"
declare -i q
export q=6+
echo "export status=$?"

echo '=== an associative array has no arithmetic to fail at'
declare -A m=([1+]=K)
printf -v 'm[2+]' S
echo "printf status=$?"
echo "m[1+]=${m[1+]} m[2+]=${m[2+]}"

echo '=== a substring bound keeps a tag of its own'
v=abcdef
echo "${v:1 z}"
echo "status=$?"
