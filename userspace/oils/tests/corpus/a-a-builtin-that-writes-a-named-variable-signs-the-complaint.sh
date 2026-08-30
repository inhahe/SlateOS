# The complaints a *variable* makes are signed by whoever named it.
#
# A malformed `-i` value and a subscript put on a nameref that already carries
# one are both refusals by the store, which does not know who asked for the
# write. bash signs them with the builtin whose operand named the variable, so
# one store speaks under a dozen names: `declare -i n; read n <<< 3+` reports
# `read: 3+: syntax error`, and the same store reached through `export` reports
# `export: 3+: syntax error`.
#
# The signature is the builtin's *own* name rather than the one that did the
# storing, which is why `typeset` and `readarray` sign as themselves rather
# than as the `declare` and `mapfile` they are otherwise indistinguishable from.
#
# What goes unsigned is a write nobody's operand asked for: a bare `n=3+`
# assignment, the same as a temporary prefix, and a `for` loop's control
# variable. There the complaint stands on its own.

echo "=== a malformed -i value, signed"
r() { echo "--- $1"; ( declare -i n; eval "$1" ) 2>&1; echo "rc=$?"; }
r 'read -r n <<< 3+'
r 'declare n=3+'
r 'typeset n=3+'
r 'export n=3+'
r 'readonly n=3+'
r 'getopts a n'
r 'let "n = 3+"'
r '(( n = 3+ ))'

echo "=== …and unsigned where nobody's operand named it"
r 'n=3+'
r 'for n in 3+; do :; done'
r 'n=3+ true'

echo "=== a subscript on a nameref that already designates one, signed"
s() { echo "--- $1"; ( declare -a q=(x y); declare -n rr='q[1]'; eval "$1" ) 2>&1; echo "rc=$?"; }
s 'read -r "rr[0]" <<< v'
s 'mapfile -t rr <<< v'
s 'readarray -t rr <<< v'
s 'let "rr[0] = 5"'
s '(( rr[0] = 5 ))'

echo "=== …and unsigned from a bare assignment"
s 'rr[0]=v'
s 'x=$(( rr[0] = 5 )); echo "x=$x"'

echo "=== a builtin that runs commands lends its name to none of them"
( declare -i n; eval 'read -r n <<< 3+' ) 2>&1
( declare -i n; eval 'n=3+' ) 2>&1
