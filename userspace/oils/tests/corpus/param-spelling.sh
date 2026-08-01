# `$name` and `${name}` expand alike, but the spelling the source used survives
# into what the shell prints and says: a nounset diagnostic names the parameter
# exactly as written, and `declare -f` reproduces a function body verbatim. The
# two forms read the same for an identifier — only a positional or special
# parameter has an unbraced spelling that carries the `$` along.
n() { sed 's/^.*: line [0-9]*: //'; }

echo "=== the nounset diagnostic names the parameter as written"
( set -u; f() { echo "[$1]"; }; f ) 2>&1 | n
( set -u; f() { echo "[${1}]"; }; f ) 2>&1 | n
# `$12` is `$1` followed by a literal `2`, so it is `$1` that is named.
( set -u; f() { echo "[$12]"; }; f ) 2>&1 | n
( set -u; f() { echo "[${12}]"; }; f ) 2>&1 | n
# `${#name}` has no unbraced spelling, so it never carries the `$`.
( set -u; f() { echo "[${#1}]"; }; f ) 2>&1 | n
( set -u; echo "[$nosuch]" ) 2>&1 | n
( set -u; echo "[${nosuch}]" ) 2>&1 | n
( set -u; echo "[${#nosuch}]" ) 2>&1 | n

echo "=== a default-value operator still suppresses it"
( set -u; f() { echo "[${1:-d}]"; }; f; echo "rc=$?" ) 2>&1 | n
( set -u; echo "[${nosuch+s}]"; echo "rc=$?" ) 2>&1 | n

echo "=== declare -f reproduces the spelling"
g() { echo "${1}" "$1" "${#}" "$#" "${x}" "$x" "${12}" "${nosuch:-d}"; }
declare -f g
echo "=== done"
