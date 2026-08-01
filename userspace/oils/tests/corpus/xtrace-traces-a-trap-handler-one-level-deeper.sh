# A trap handler's body is a level of *indirection* to `set -x`, the same way a
# command substitution is: every line it traces gets one more copy of `PS4`'s
# first character than the command that fired it. So a DEBUG handler announces
# itself at `++` and only then is the command it was announcing traced at `+`.
#
# It is the trap that adds the level, not the call: an inline handler counts as
# much as a function one, and calling that same function ordinarily traces at
# the plain depth. A `( … )` subshell adds nothing either — of the things that
# are not traps, only a command substitution counts.
#
# The `EXIT` trap is the exception. bash runs it at shutdown rather than through
# the pending-trap path, and traces its body at the ordinary depth.
b() { echo B; }

echo "=== a DEBUG handler traces one level deeper"
set -x
trap b DEBUG
echo one
trap - DEBUG
set +x

echo "=== so does an inline one"
set -x
trap 'echo I' DEBUG
echo two
trap - DEBUG
set +x

echo "=== and an ERR handler"
set -x
trap 'echo E' ERR
false
trap - ERR
set +x

echo "=== and a RETURN handler"
f() { echo in; }
set -x -T
trap 'echo R' RETURN
f
trap - RETURN
set +x +T

echo "=== a plain call does not, nor does a subshell"
set -x
b
( b )
set +x

echo "=== but a command substitution does"
set -x
x=$(b)
set +x

# The handler and the substitution are each one level, and neither is inside
# the other: the announcement is traced at `++`, then the substitution's body at
# `++` as well, and only then the assignment at `+`. (Nothing announces `echo
# two` — the substitution's subshell has no DEBUG trap without `functrace`.)
echo "=== a handler and a substitution each count once"
set -x
trap b DEBUG
y=$(echo two)
trap - DEBUG
set +x

echo "=== the EXIT trap is traced at the ordinary depth"
set -x
trap 'echo X' EXIT
echo three
