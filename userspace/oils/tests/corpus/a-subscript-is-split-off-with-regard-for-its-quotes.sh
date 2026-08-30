# Where a subscript ends is a question about quoting, not about the last byte.
#
# bash matches a name-with-subscript with `skipsubscript`, which steps *over*
# quoted runs, backslashes and `$(…)`/`${…}`/`` `…` `` rather than through them.
# So a `]` inside quotes does not close the subscript, and a quote that never
# ends means the subscript never closes at all -- which makes the whole operand
# not a name, and it is refused as one before anything is expanded. That is why
# these say `not a valid identifier` rather than complaining about arithmetic.

n=(a b c)
declare -A m=([k]=K)

echo '=== a quote that never ends closes nothing'
printf -v 'n["1]' X
echo "printf status=$?  n=(${n[@]})"
printf -v 'n[$(echo 1]' Y
echo "printf status=$?  n=(${n[@]})"
read -r 'n["1]' <<<T
echo "read status=$?  n=(${n[@]})"
declare 'n["1]=U'
echo "declare status=$?  n=(${n[@]})"
export 'n["1]=E'
echo "export status=$?  n=(${n[@]})"
readonly 'n["1]=R'
echo "readonly status=$?  n=(${n[@]})"
mapfile 'n["1]' </dev/null
echo "mapfile status=$?  n=(${n[@]})"
getopts 'a' 'n["1]' -a
echo "getopts status=$?"

echo '=== a nameref target is refused the same way'
declare -n r='n["1]'
echo "declare -n status=$?  r=[${r}]"

echo '=== so is an indirect target'
q='n["1]'
echo "[${!q}]"
echo "status=$?"

echo '=== unset checks the spelling only under -v'
unset 'n["1]'
echo "unset status=$?  n=(${n[@]})"
unset -v 'n["1]'
echo "unset -v status=$?  n=(${n[@]})"

echo '=== a ] inside quotes is an ordinary byte of the subscript'
printf -v 'n["]"]' Z
echo "printf status=$?  n=(${n[@]})"
printf -v "m[']']" Q
echo "printf status=$?  m[]]=${m[']']}"
declare -A m2=()
declare 'm2["]"]=S'
echo "declare status=$?  m2[]]=${m2["]"]}"

echo '=== a well-quoted key with a space still works everywhere'
printf -v 'm["k k"]' V
echo "printf status=$?  m[k k]=${m["k k"]}"
declare -n g='m["k k"]'
echo "g=$g"
export 'm["k k"]=W'
echo "export status=$?  m[k k]=${m["k k"]}"

echo '=== a subscript may nest brackets and expansions'
n2=(p q r)
printf -v "n[${#n2[@]}]" D
echo "printf status=$?  n=(${n[@]})"
printf -v 'n[${#n2[@]}]' F
echo "printf status=$?  n=(${n[@]})"

echo '=== an unbalanced or trailing-junk operand is still refused'
printf -v 'n[1' G
echo "printf status=$?  n=(${n[@]})"
printf -v 'n[1]x' H
echo "printf status=$?  n=(${n[@]})"
printf -v 'n[]' I
echo "printf status=$?  n=(${n[@]})"
