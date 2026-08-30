# `local` outside a function is refused by the *builtin*, which bash reaches
# only once the whole word list has expanded — so a compound `name=(…)` operand
# has already bound, globally, by the time the refusal is raised, and the
# refusal itself is redirectable.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== the operand binds before the refusal"
( local q=(1); echo "rc=$?"; declare -p q ) 2>&1 | e
( local -A m=([k]=v); echo "rc=$?"; declare -p m ) 2>&1 | e
( local -x -a q=(1); echo "rc=$?"; declare -p q ) 2>&1 | e
( local -g q=(1); echo "rc=$?"; declare -p q ) 2>&1 | e
( local -p q=(1); echo "rc=$?"; declare -p q ) 2>&1 | e

echo "=== …and the refusal is the command's own to redirect"
( local q=(1) 2>/dev/null; echo "rc=$?"; declare -p q ) 2>&1 | e
( local q=1 2>/dev/null; echo "rc=$?" ) 2>&1 | e
( local q=1; echo "rc=$?" ) 2>&1 | e

echo "=== none of the builtin's attributes land"
( local -n r=(1); echo "rc=$?" ) 2>&1 | e
( declare -A ci=([k]=v); local q=(1) ci=(2); echo "rc=$?"; declare -p q ) 2>&1 | e

echo "=== a readonly target takes the plain path, not the local-shadow one"
readonly ro=1
( local ro=(1); echo "unreached" ) 2>&1 | e

echo "=== the trace line is the builtin's, and comes after the operand's"
( set -x; local q=(1) ) 2>&1 | e

echo "=== done"
