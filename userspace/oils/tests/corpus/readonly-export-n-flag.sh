# `-n` means something different to `readonly`/`export` than it does to
# `declare`. For `declare` it is the nameref letter; for these two it means
# "do not apply my own attribute", so the operand is still assigned (and still
# takes `-a`/`-A`) but never becomes readonly/exported, and never becomes a
# reference.
p() { echo -n "$1 -> "; declare -p "$2" 2>&1 | sed 's/^.*: line [0-9]*: //'; }

echo "=== readonly -n assigns but does not mark"
readonly -n b1=1; echo "rc=$?"; p 'scalar  ' b1
b1=2; echo "write rc=$? val=$b1"

echo "=== a bare readonly -n on a fresh name is a complete no-op"
readonly -n b2; echo "rc=$?"; p 'bare fresh' b2

echo "=== but it does not un-make an existing readonly"
readonly b3=1; readonly -n b3; echo "rc=$?"; p 'bare over ro' b3
b3=2; echo "write rc=$? val=$b3"

echo "=== and a valued operand over a readonly name is still refused"
readonly b4=1; readonly -n b4=2; echo "rc=$?"; p 'valued over ro' b4

echo "=== the array letters still apply"
readonly -na b5=1; echo "rc=$?"; p '-na scalar' b5
readonly -nA b6=1; echo "rc=$?"; p '-nA scalar' b6
readonly -n b7=(1 2); echo "rc=$?"; p '-n compound' b7
b7[0]=9; echo "write rc=$?"; p 'after write' b7

echo "=== -n does not make a reference, so the value stays literal"
w=hello
readonly -n b8=w; echo "rc=$?"; p '-n b8=w' b8

echo "=== export -n is the same shape, and really does remove an export"
export -n c1=1; echo "rc=$?"; p 'scalar  ' c1
export c2=1; export -n c2; echo "rc=$?"; p 'bare over -x' c2
export -n c3; echo "rc=$?"; p 'bare fresh  ' c3
export -n c4=(1 2); echo "rc=$?"; p '-n compound ' c4
export -na c5=1; echo "rc=$?"; p '-na scalar  ' c5
export -n c6=w; echo "rc=$?"; p '-n c6=w     ' c6

echo "=== +n is not a flag to either of them"
readonly +n d1=1; echo "rc=$?"; p 'readonly +n' d1
export +n d2=1; echo "rc=$?"; p 'export +n  ' d2

echo "=== declare still reads -n as the nameref letter"
declare -n d3=w; echo "rc=$?"; p 'declare -n d3=w' d3

echo "=== and the environment agrees about what is exported"
export -n e1=1; export e2=2; export e3=3; export -n e3
env | grep -E '^e[123]=' | sort

echo "=== done"
