# `-v` asks whether a *name* is set, and the shell's punctuation parameters do
# not have names it can ask about: `-v @`, `-v *`, `-v #`, `-v ?`, `-v $`,
# `-v !` and `-v -` are all false however much of a value the parameter has.
# What it does answer for is an identifier, an array or one of its elements, a
# positional parameter — `$0` included, and `_`, which is an ordinary identifier
# whatever it looks like. Both spellings agree, `[[ -v ]]` and `test -v` alike.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== the punctuation parameters are never what -v is about"
q 'set -- x y; for p in @ "*" "#" "?" "$" "!" - _ 0 1 2 3; do [ -v "$p" ]; echo "$p=$?"; done'
q 'set -- ; for p in @ "*" "#" "?" "$" "!" - _ 0 1; do [ -v "$p" ]; echo "$p=$?"; done'
q 'set -- ""; [ -v @ ]; echo "at=$?"'
q 'true & wait; [ -v "!" ]; echo "bang=$?"'
q 'set -- x; [[ -v @ ]]; echo "at=$?"'
q 'set -- x; [[ -v "*" ]]; echo "star=$?"'
q 'set -- x; test -v "@"; echo "at=$?"'
q 'set -- x; [ -v "@" ] || echo no'

echo "=== a positional is, and it tracks shift and set"
q 'set -- a b; [ -v 1 ]; echo "1=$?"; [ -v 2 ]; echo "2=$?"; [ -v 3 ]; echo "3=$?"'
q 'set -- a b; shift; [ -v 1 ]; echo "1=$?"; [ -v 2 ]; echo "2=$?"'
q 'set -- ; [ -v 1 ]; echo "1=$?"; [ -v 0 ]; echo "0=$?"'
q 'set -- ""; [ -v 1 ]; echo "1=$?"'

echo "=== and so is an ordinary name, an array and an element"
q 'v=x; [ -v v ]; echo "v=$?"; [ -v nosuch ]; echo "nosuch=$?"'
q 'v=; [ -v v ]; echo "v=$?"'
q 'a=(p q); [ -v a ]; echo "a=$?"; [ -v "a[1]" ]; echo "a1=$?"; [ -v "a[5]" ]; echo "a5=$?"'
q 'declare -A m; m[k]=v; [ -v "m[k]" ]; echo "mk=$?"; [ -v "m[j]" ]; echo "mj=$?"'
q 'a=(p q); [ -v "a[@]" ]; echo "aat=$?"'
q 'v=x; declare -n r=v; [ -v r ]; echo "r=$?"'
q 'declare -n r=nosuch; [ -v r ]; echo "r=$?"'
echo "=== done"
