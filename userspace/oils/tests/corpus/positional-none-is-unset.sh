# A shell with no positional parameters at all does not have an empty `$@` — it
# has no `$@`. The two read alike when you just expand them, but every question
# that tells set-from-unset says so: `${@-U}` and `${*-U}` give the default,
# `${@+s}` gives nothing and `${@?msg}` faults. One empty positional
# (`set -- ""`) is set-but-empty, so what counts is having none rather than
# joining to nothing. `set -u` is a separate question — `$@` and `$*` are
# exempt from it, so a plain `"$@"` is still safe in a shell with none — and so
# is `${#@}`, which counts the positionals and answers 0.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== with no positionals they are unset, not empty"
q 'set -- ; echo "[${*-U}][${@-U}]"'
q 'set -- ; echo "[${*:-U}][${@:-U}]"'
q 'set -- ; echo "[${*+s}][${@+s}]"'
q 'set -- ; echo "[${*:+s}][${@:+s}]"'
q 'set -- ; echo "[${*?msg}]"'
q 'set -- ; echo "[${@?msg}]"'
q 'set -- ; echo "[${@:?msg}]"'
q 'set -- ; echo "[${*-U}]" "[${@-U}]"'
q 'set -- ; echo "${@-U}" | cat -A'
q 'set -- ; printf "<%s>" ${@-U}; echo'
q 'set -- ; a=(${@-U}); declare -p a'

echo "=== one empty positional is set, just empty"
q 'set -- ""; echo "[${*-U}][${@-U}][${*+s}][${@+s}]"'
q 'set -- "" ""; echo "[${*-U}][${@-U}]"'
q 'set -- x; echo "[${*-U}][${@-U}][${*+s}][${@+s}]"'

echo "=== a function has its own, so the caller's emptiness does not carry in"
q 'set -- ; f() { echo "[${@-U}][${*-U}]"; }; f'
q 'set -- ; f() { echo "[${@-U}]"; }; f a'
q 'set -- a; f() { echo "[${@-U}]"; }; f'
q 'set -- ; f() { g() { echo "[${@-U}]"; }; g "$@"; }; f x'

echo "=== shift and set can take them away again"
q 'set -- x; shift; echo "[${@-U}]"'
q 'set -- x y; shift 2; echo "[${@-U}]"'
q 'set -- x; set -- ; echo "[${@-U}]"'
q 'set -- x; set --; echo "[${*+s}]"'

echo "=== but nounset is a separate question, and the count is always there"
q 'set -- ; set -u; echo "[$*][$@][${#*}][${#@}]"'
q 'set -- ; set -u; for i in "$@"; do echo "[$i]"; done; echo none'
q 'set -- ; echo "[${#*}][${#@}][$#]"'

echo "=== and a modifier still reads them as the empty join"
q 'set -- ; echo "[${*#x}][${@#x}]"'
q 'set -- ; echo "[${*^^}][${@^^}]"'
q 'set -- ; echo "[${*/x/y}]"'
q 'set -- ; echo "[${@@Q}]"'
q 'set -- ; echo "[${@:1}][${@:1:2}]"'

echo "=== an indirection through one asks the same question"
q 'set -- ; r=@; echo "[${!r}][${!r-U}]"'
q 'set -- ; r=*; echo "[${!r-U}]"'
q 'set -- x; r=@; echo "[${!r-U}]"'
echo "=== done"
