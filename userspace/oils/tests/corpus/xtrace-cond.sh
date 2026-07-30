# `set -x` traces a `[[ … ]]` one *primary* at a time, not once per command.
#
# bash prints each term where it evaluates it — after expanding both operands,
# before testing them — so the trace shows how the condition was walked: a
# short-circuited `&&` never prints its right half, an `||` that has to look at
# both prints two lines for a single command, and `[[ ( a ) ]]` prints the term
# without the parentheses that only shaped the tree.
#
# Three further rules are visible below:
#
#   * a `!` is printed by the node it binds to. `[[ ! a == b ]]` prints it,
#     because `!` binds to the primary; `[[ ! ( a == b ) ]]` prints nothing,
#     because there the negation sits on the group and the primary inside was
#     never negated. A doubled `!` cancels and prints neither.
#   * a bare-word test prints as the `-n` it means, the same rewrite `declare -f`
#     shows.
#   * operands print with *no* quoting — a space just appears — except that an
#     empty one shows as `''`. A pattern is the exception to the exception: a
#     character a quote made literal prints backslash-escaped, which is how bash
#     shows which metacharacters are still live.
#
# Traces go through `2>&1` in a subshell so they land on stdout in a fixed order.

echo "=== one line per primary"
( set -x; [[ 1 == 1 ]] ) 2>&1
( set -x; [[ abc == a* ]] ) 2>&1
( set -x; [[ -n foo ]] ) 2>&1
( set -x; [[ -o xtrace ]] ) 2>&1

echo "=== a bare word is the -n test it means"
( set -x; [[ foo ]] ) 2>&1
( set -x; e=; [[ $e ]] ) 2>&1
( set -x; [[ 'a b' ]] ) 2>&1

echo "=== && and || print only the primaries they reach"
( set -x; [[ 1 == 1 && 2 == 2 ]] ) 2>&1
( set -x; [[ 1 == 2 && 2 == 3 ]] ) 2>&1
( set -x; [[ 1 == 2 || 2 == 2 ]] ) 2>&1
( set -x; [[ 1 == 1 || 2 == 3 ]] ) 2>&1
( set -x; [[ a == a && b == b && c == c ]] ) 2>&1
( set -x; [[ a == x || b == y || c == c ]] ) 2>&1

echo "=== grouping shaped the tree and is not printed"
( set -x; [[ ( 1 == 1 ) ]] ) 2>&1
( set -x; [[ ( ( 1 == 1 ) ) ]] ) 2>&1
( set -x; [[ ( x == x || y == y ) && z == z ]] ) 2>&1
( set -x; [[ x == y || ( a == a && b == b ) ]] ) 2>&1

echo "=== ! is printed by whatever node it binds to"
( set -x; [[ ! x == y ]] ) 2>&1
( set -x; [[ ! -n foo ]] ) 2>&1
( set -x; [[ ! foo ]] ) 2>&1
( set -x; [[ ! ( x == y ) ]] ) 2>&1
( set -x; [[ ! x == x && 1 == 1 ]] ) 2>&1
( set -x; [[ ! ! x == x ]] ) 2>&1
( set -x; [[ ! x =~ y ]] ) 2>&1

echo "=== operands print as they expanded, unquoted, empty as ''"
( set -x; v='a b'; [[ $v == "a b" ]] ) 2>&1
( set -x; v='a b'; [[ "$v" < "c d" ]] ) 2>&1
( set -x; e=; [[ "$e" == x ]] ) 2>&1
( set -x; e=; [[ x == "$e" ]] ) 2>&1
( set -x; e=; [[ -n "$e" ]] ) 2>&1
( set -x; set -- 'p q' r; [[ "$*" == 'p q r' ]] ) 2>&1
( set -x; [[ "$(echo sub)" == sub ]] ) 2>&1

echo "=== a pattern shows which metacharacters are still live"
( set -x; [[ ab == "a"b ]] ) 2>&1
( set -x; [[ 'a*' == 'a*' ]] ) 2>&1
( set -x; [[ 'a*' == a\* ]] ) 2>&1
( set -x; [[ abc == * ]] ) 2>&1
( set -x; [[ abc == a?c ]] ) 2>&1
( set -x; [[ x == "" ]] ) 2>&1
( set -x; [[ 'a b' == 'a b' ]] ) 2>&1
( set -x; [[ abc == @(abc|x) ]] ) 2>&1

echo "=== =~ escapes only what a quote made literal, and only if it matters"
( set -x; [[ abc =~ ^a ]] ) 2>&1
( set -x; [[ abc =~ "a"bc ]] ) 2>&1
( set -x; [[ a.c =~ 'a.c' ]] ) 2>&1
( set -x; [[ a.c =~ a.c ]] ) 2>&1

echo "=== the other operators, each with the spelling it was written with"
( set -x; [[ a = a ]] ) 2>&1
( set -x; [[ a != b ]] ) 2>&1
( set -x; [[ a < b ]] ) 2>&1
( set -x; [[ b > a ]] ) 2>&1
( set -x; [[ 1 -eq 1 ]] ) 2>&1
( set -x; [[ 1+1 -eq 2 ]] ) 2>&1
( set -x; [[ "1+1" -eq 2 ]] ) 2>&1
( set -x; [[ 1 -ne 2 ]] ) 2>&1
( set -x; [[ 1 -lt 2 ]] ) 2>&1
( set -x; [[ 2 -ge 1 ]] ) 2>&1
( set -x; [[ -h /nosuch ]] ) 2>&1
( set -x; [[ -L /nosuch ]] ) 2>&1
( set -x; [[ -e /nosuch ]] ) 2>&1
( set -x; [[ /nosuch -nt /nosuch2 ]] ) 2>&1
( set -x; [[ /nosuch -ef /nosuch2 ]] ) 2>&1
( set -x; [[ -v novar ]] ) 2>&1

echo "=== both numeric operands are expanded before either is evaluated"
( set -x; [[ $(echo '1 2') -eq $(echo 1) ]] ) 2>&1
echo "st=$?"

echo "=== PS4 and nesting apply as they do to any other trace"
( PS4='T '; set -x; [[ a == a ]] ) 2>&1
( PS4='+${x:+ }'; x=; set -x; [[ a == a ]] ) 2>&1
f() { [[ inner == inner ]]; }
( set -x; f ) 2>&1

echo "=== a condition inside a compound command"
( set -x; if [[ 1 == 1 ]]; then :; fi ) 2>&1
( set -x; while [[ x == y ]]; do :; done ) 2>&1
( set -x; [[ a == a ]] && [[ b == b ]] ) 2>&1
