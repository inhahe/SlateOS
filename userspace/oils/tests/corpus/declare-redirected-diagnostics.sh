# A `declare` carrying a compound operand speaks with two voices, and only one
# of them can be redirected: bash binds `name=(…)` while it is still *expanding*
# the words, before the command's own redirections are in place, and runs the
# builtin only afterwards. So the compound-assignment machinery's diagnostics
# escape `2>/dev/null` and the builtin's do not.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== the builtin's own refusals are redirected"
( declare -aA z=(3) 2>/dev/null; echo "rc=$?" ) 2>&1 | e
( declare -a p=(1); declare -A +a p=(2) 2>/dev/null; echo "rc=$?"; declare -p p ) 2>&1 | e
( declare -Z q=(1) 2>/dev/null; echo "rc=$?" ) 2>&1 | e

echo "=== …and the machinery's are not"
( declare -A ci=([k]=v); declare -a ci=(1) 2>/dev/null; echo "rc=$?" ) 2>&1 | e
( declare -n ng=(1 2) 2>/dev/null; echo "rc=$?" ) 2>&1 | e
( declare -i bad=(1+) 2>/dev/null; echo "rc=$?" ) 2>&1 | e

echo "=== a refusal reported by both is silenced by halves"
readonly ra=1 rb=2
f1() { local ra=(1); echo "rc=$?"; }
( f1 ) 2>&1 | e
f2() { local ra=(1) 2>/dev/null; echo "rc=$?"; }
( f2 ) 2>&1 | e
g1() { local GROUPS=(1); echo "rc=$?"; }
( g1 ) 2>&1 | e
g2() { local GROUPS=(1) 2>/dev/null; echo "rc=$?"; }
( g2 ) 2>&1 | e

echo "=== the builtin's half waits for the builtin, so it comes after every other"
# …rather than being interleaved operand by operand.
h() { local ra=(1) rb=(2); echo "rc=$?"; }
( h ) 2>&1 | e
h2() { local GROUPS=(1) FUNCNAME=(2); echo "rc=$?"; }
( h2 ) 2>&1 | e

echo "=== 2>&1 sends the builtin's half down the pipe"
( declare -aA zz=(1) 2>&1; echo "rc=$?" ) | e

echo "=== -p reports a missing name through the redirection too"
( declare -p nosuch q=(1) 2>/dev/null; echo "rc=$?" ) 2>&1 | e

echo "=== done"
